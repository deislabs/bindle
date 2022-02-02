//! A least recently used cache implementation
use std::convert::TryInto;
use std::sync::Arc;

use ::lru::LruCache as Lru;
use tempfile::NamedTempFile;
use tokio::fs::File;
use tokio::io::AsyncSeekExt;
use tokio::sync::Mutex;
use tokio_stream::{Stream, StreamExt};
use tokio_util::{
    codec::{BytesCodec, FramedRead},
    io::StreamReader,
};
use tracing::{debug, instrument, trace};
use tracing_futures::Instrument;

use super::*;
use crate::provider::{Provider, ProviderError, Result};
use crate::{Id, Invoice};

// Type alias for shorthanding a locked cache
type LockedCache<K, V> = Arc<Mutex<Lru<K, V>>>;

/// A least recently used cache implementation that stores cached invoices in memory and cached
/// parcels on disk. Any mutating operations (like creating or yanking) will pass through to the
/// configured remote provider.
///
/// The cache will store invoices in memory and parcels on disk. Parcels will be automatically
/// cleaned up from disk when they are ejected from the cache
#[derive(Clone)]
pub struct LruCache<Remote: Provider + Clone> {
    invoices: LockedCache<Id, Invoice>,
    parcels: LockedCache<String, NamedTempFile>,
    remote: Remote,
}

impl<Remote: Provider + Clone> LruCache<Remote> {
    /// Return a new LruCache with the given cache size and remote provider for fetching items that
    /// don't exist in the cache. The given cache size will be used to configure the cache size for
    /// both invoices and parcels
    pub fn new(cache_size: usize, remote: Remote) -> Self {
        LruCache {
            invoices: Arc::new(Mutex::new(Lru::new(cache_size))),
            parcels: Arc::new(Mutex::new(Lru::new(cache_size))),
            remote,
        }
    }
}

impl<Remote> Cache for LruCache<Remote> where Remote: Provider + Send + Sync + Clone {}

#[async_trait::async_trait]
impl<Remote> Provider for LruCache<Remote>
where
    Remote: Provider + Send + Sync + Clone,
{
    #[instrument(level = "trace", skip(self, inv))]
    async fn create_invoice<I>(&self, inv: I) -> Result<(crate::Invoice, Vec<crate::Label>)>
    where
        I: Signed + Verified + Send + Sync,
    {
        // In this case, the cache itself does not sign the invoice, though perhaps
        // it should at least check to see if the invoice has been signed with the
        // local proxy key.
        self.remote.create_invoice(inv).await
    }

    #[instrument(level = "trace", skip(self, id), fields(invoice_id))]
    async fn get_yanked_invoice<I>(&self, id: I) -> Result<Invoice>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
    {
        let parsed_id = id.try_into().map_err(|e| e.into())?;
        tracing::span::Span::current().record("invoice_id", &tracing::field::display(&parsed_id));
        trace!("Checking for invoice in cache");
        let mut invoices = self.invoices.lock().await;
        match invoices.get(&parsed_id) {
            Some(i) => Ok(i.clone()),
            None => {
                async {
                    debug!("Did not find invoice in cache, attempting to fetch from remote");
                    let inv = self.remote.get_yanked_invoice(&parsed_id).await?;
                    invoices.put(parsed_id.clone(), inv.clone());
                    Ok(inv)
                }
                .instrument(tracing::trace_span!("get_invoice_cache_miss", invoice_id = %parsed_id))
                .await
            }
        }
    }

    #[instrument(level = "trace", skip(self, id), fields(invoice_id))]
    async fn yank_invoice<I>(&self, id: I) -> Result<()>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
    {
        // Delete the invoice from the local cache as it will be no longer valid
        let parsed_id = id.try_into().map_err(|e| e.into())?;
        tracing::span::Span::current().record("invoice_id", &tracing::field::display(&parsed_id));
        debug!("Removing local cache entry for yanked invoice");
        self.invoices.lock().await.pop(&parsed_id);
        self.remote.yank_invoice(parsed_id).await
    }

    #[instrument(level = "trace", skip(self, bindle_id, data), fields(invoice_id))]
    async fn create_parcel<I, R, B>(&self, bindle_id: I, parcel_id: &str, data: R) -> Result<()>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
        R: Stream<Item = std::io::Result<B>> + Unpin + Send + Sync + 'static,
        B: bytes::Buf + Send,
    {
        let parsed_id = bindle_id.try_into().map_err(|e| e.into())?;
        tracing::span::Span::current().record("invoice_id", &tracing::field::display(&parsed_id));
        self.validate_parcel(&parsed_id, parcel_id).await?;
        debug!("Passing through create parcel request to remote");
        self.remote.create_parcel(parsed_id, parcel_id, data).await
    }

    #[instrument(level = "trace", skip(self, bindle_id), fields(invoice_id))]
    async fn get_parcel<I>(
        &self,
        bindle_id: I,
        parcel_id: &str,
    ) -> Result<Box<dyn Stream<Item = Result<bytes::Bytes>> + Unpin + Send + Sync>>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
    {
        let parsed_id = bindle_id.try_into().map_err(|e| e.into())?;
        tracing::span::Span::current().record("invoice_id", &tracing::field::display(&parsed_id));
        // TODO: Should we be worrying about checking the parcel exists in the invoice here? As
        // this is a cache, the remote it fetches from should cover this, but I could also see
        // someone misusing this by fetching from a parcel they have access to and then being
        // able to fetch the parcel without a bindle reference, but maybe that is just paranoia
        trace!("Validating that parcel exists in invoice");
        self.validate_parcel(&parsed_id, parcel_id).await?;

        let mut parcels = self.parcels.lock().await;
        let parcel_id_owned = parcel_id.to_owned();
        trace!("Checking for parcel {}@{} in cache", parsed_id, parcel_id);
        let file = match parcels.get(&parcel_id_owned) {
            Some(f) => {
                // This forces a requirement of a multithreaded runtime, but avoids weird borrowing
                // issues required by the static bound on `spawn_blocking`. If this is going to be a
                // problem, we can try something else
                File::from_std(tokio::task::block_in_place(move || f.reopen())?)
            }
            None => {
                async {
                    debug!("Cache miss for getting parcel. Attempting to fetch from server");
                    let stream = self.remote.get_parcel(&parsed_id, parcel_id).await?;
                    trace!("Attempting to insert parcel data into cache");
                    let tempfile = tokio::task::spawn_blocking(NamedTempFile::new)
                        .await
                        .map_err(|e| ProviderError::Other(e.to_string()))??;
                    let handle = tempfile.as_file().try_clone()?;
                    let mut file = File::from_std(handle);
                    tokio::io::copy(
                        &mut StreamReader::new(stream.map(|res| {
                            res.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
                        })),
                        &mut file,
                    )
                    .await?;

                    // Insert the file in the cache
                    parcels.put(parcel_id_owned, tempfile);
                    trace!("Parcel caching successful");

                    // Seek back to the beginning of the file before returning
                    trace!("Resetting file cursor to start");
                    file.seek(std::io::SeekFrom::Start(0)).await?;
                    Ok::<_, ProviderError>(file)
                }
                .instrument(tracing::trace_span!("get_parcel_cache_miss", invoice_id = %parsed_id, parcel_id))
                .await?
            }
        };

        Ok::<Box<dyn Stream<Item = Result<bytes::Bytes>> + Unpin + Send + Sync>, _>(Box::new(
            FramedRead::new(file, BytesCodec::default())
                .map(|res| res.map(|b| b.freeze()).map_err(ProviderError::from)),
        ))
    }

    #[instrument(level = "trace", skip(self, bindle_id), fields(invoice_id))]
    async fn parcel_exists<I>(&self, bindle_id: I, parcel_id: &str) -> Result<bool>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
    {
        let parsed_id = bindle_id.try_into().map_err(|e| e.into())?;
        tracing::span::Span::current().record("invoice_id", &tracing::field::display(&parsed_id));
        self.validate_parcel(&parsed_id, parcel_id).await?;
        let parcels = self.parcels.lock().await;
        // For some reason we can't just used the borrowed string here because I think it is
        // expecting an &String vs an &str
        if parcels.contains(&parcel_id.to_owned()) {
            trace!("Parcel exists in cache, returning");
            Ok(true)
        } else {
            debug!("Parcel does not exist in cache, checking remote");
            self.remote.parcel_exists(&parsed_id, parcel_id).instrument(tracing::trace_span!("parcel_exists_cache_miss", invoice_id = %parsed_id, parcel_id)).await
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        provider::Provider, signature::KeyRing, testing, SecretKeyEntry, SignatureRole,
        VerificationStrategy,
    };
    use std::{convert::TryFrom, sync::Arc};

    use tokio::sync::Mutex;
    use tokio_stream::StreamExt;

    /// A test provider that lets us make sure the cache is being called
    #[derive(Default, Clone)]
    struct TestProvider {
        get_yanked_count: Arc<Mutex<u8>>,
        get_parcel_count: Arc<Mutex<u8>>,
        parcel_exists_count: Arc<Mutex<u8>>,

        create_invoice_called: Arc<Mutex<bool>>,
        yank_invoice_called: Arc<Mutex<bool>>,
        create_parcel_called: Arc<Mutex<bool>>,
    }

    #[async_trait::async_trait]
    impl Provider for TestProvider {
        async fn create_invoice<I>(&self, _inv: I) -> Result<(crate::Invoice, Vec<crate::Label>)>
        where
            I: Signed + Verified + Send + Sync,
        {
            let mut called = self.create_invoice_called.lock().await;
            *called = true;
            Ok((
                crate::Invoice::new(crate::BindleSpec {
                    id: crate::Id::try_from("foo/bar/1.0.0").unwrap(),
                    description: None,
                    authors: None,
                }),
                Vec::new(),
            ))
        }

        async fn get_yanked_invoice<I>(&self, _id: I) -> Result<Invoice>
        where
            I: TryInto<Id> + Send,
            I::Error: Into<ProviderError>,
        {
            let scaffold = testing::Scaffold::load("valid_v1").await;
            let mut count = self.get_yanked_count.lock().await;
            *count += 1;
            Ok(scaffold.invoice)
        }

        async fn yank_invoice<I>(&self, _id: I) -> Result<()>
        where
            I: TryInto<Id> + Send,
            I::Error: Into<ProviderError>,
        {
            let mut called = self.yank_invoice_called.lock().await;
            *called = true;
            Ok(())
        }

        async fn create_parcel<I, R, B>(
            &self,
            _bindle_id: I,
            _parcel_id: &str,
            _data: R,
        ) -> Result<()>
        where
            I: TryInto<Id> + Send,
            I::Error: Into<ProviderError>,
            R: Stream<Item = std::io::Result<B>> + Unpin + Send + Sync + 'static,
            B: bytes::Buf + Send,
        {
            let mut called = self.create_parcel_called.lock().await;
            *called = true;
            Ok(())
        }

        async fn get_parcel<I>(
            &self,
            _bindle_id: I,
            parcel_id: &str,
        ) -> Result<Box<dyn Stream<Item = Result<bytes::Bytes>> + Unpin + Send + Sync>>
        where
            I: TryInto<Id> + Send,
            I::Error: Into<ProviderError>,
        {
            let scaffold = testing::Scaffold::load("valid_v1").await;
            let mut count = self.get_parcel_count.lock().await;
            *count += 1;
            let info = scaffold
                .parcel_files
                .into_iter()
                .map(|(_, info)| info)
                .find(|info| info.sha == parcel_id)
                .expect("Unable to find parcel");
            Ok(Box::new(
                FramedRead::new(std::io::Cursor::new(info.data), BytesCodec::default())
                    .map(|res| res.map(|b| b.freeze()).map_err(ProviderError::from)),
            ))
        }

        async fn parcel_exists<I>(&self, _bindle_id: I, _parcel_id: &str) -> Result<bool>
        where
            I: TryInto<Id> + Send,
            I::Error: Into<ProviderError>,
        {
            let mut count = self.parcel_exists_count.lock().await;
            *count += 1;
            Ok(true)
        }
    }

    #[tokio::test]
    async fn test_get_invoice() {
        // Get the invoice twice and make sure we only call the remote provider once
        let provider = TestProvider::default();
        let cache = LruCache::new(10, provider.clone());

        cache
            .get_invoice("enterprise.com/warpcore/1.0.0")
            .await
            .expect("Should be able to get invoice");
        cache
            .get_invoice("enterprise.com/warpcore/1.0.0")
            .await
            .expect("Should be able to get invoice a second time");

        let num_called = provider.get_yanked_count.lock().await;

        assert_eq!(
            1, *num_called,
            "Remote store should have only been called once"
        )
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_get_parcel() {
        // Get the invoice twice and make sure we only call the remote provider once
        let provider = TestProvider::default();
        let cache = LruCache::new(10, provider.clone());

        let scaffold = testing::Scaffold::load("valid_v1").await;
        let sha = scaffold.parcel_files.get("parcel").unwrap().sha.as_str();
        let _ = cache
            .get_parcel(&scaffold.invoice.bindle.id, sha)
            .await
            .expect("Should be able to get parcel");
        let _ = cache
            .get_parcel(&scaffold.invoice.bindle.id, sha)
            .await
            .expect("Should be able to get parcel a second time");

        let num_called = provider.get_parcel_count.lock().await;

        assert_eq!(
            1, *num_called,
            "Remote store should have only been called once"
        )
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_parcel_exists() {
        let provider = TestProvider::default();
        let cache = LruCache::new(10, provider.clone());

        let scaffold = testing::Scaffold::load("valid_v1").await;
        let sha = scaffold.parcel_files.get("parcel").unwrap().sha.as_str();

        // First make sure we call through to the remote
        cache
            .parcel_exists(&scaffold.invoice.bindle.id, sha)
            .await
            .expect("Should be able to call parcel exists");

        // Now get the parcel so it is in the cache
        let _ = cache
            .get_parcel(&scaffold.invoice.bindle.id, sha)
            .await
            .expect("Should be able to get parcel");

        // Now make sure we hit the cache
        cache
            .parcel_exists(&scaffold.invoice.bindle.id, sha)
            .await
            .expect("Should be able to call parcel exists");

        let num_called = provider.parcel_exists_count.lock().await;

        assert_eq!(
            1, *num_called,
            "Remote store should have only been called once"
        )
    }

    #[tokio::test]
    async fn test_passthrough() {
        // Make sure all the create operations pass through
        let provider = TestProvider::default();
        let cache = LruCache::new(10, provider.clone());
        let sk = SecretKeyEntry::new("TEST", vec![SignatureRole::Proxy]);

        let scaffold = testing::Scaffold::load("valid_v1").await;
        let verified = VerificationStrategy::MultipleAttestation(vec![])
            .verify(scaffold.invoice.clone(), &KeyRing::default())
            .unwrap();
        let signed = crate::invoice::sign(verified, vec![(SignatureRole::Creator, &sk)]).unwrap();
        cache
            .create_invoice(signed)
            .await
            .expect("Should be able to create invoice");
        cache
            .yank_invoice("enterprise.com/warpcore/1.0.0")
            .await
            .expect("Should be able to yank invoice");
        let parcel_info = scaffold.parcel_files.get("parcel").unwrap();
        cache
            .create_parcel(
                &scaffold.invoice.bindle.id,
                &parcel_info.sha,
                FramedRead::new(
                    std::io::Cursor::new("hello".as_bytes().to_vec()),
                    BytesCodec::default(),
                ),
            )
            .await
            .expect("should be able to create parcel");

        assert!(
            *provider.create_invoice_called.lock().await,
            "Remote provider should have been called for create invoice"
        );
        assert!(
            *provider.yank_invoice_called.lock().await,
            "Remote provider should have been called for yank invoice"
        );
        assert!(
            *provider.create_parcel_called.lock().await,
            "Remote provider should have been called for create parcel"
        );
    }
}
