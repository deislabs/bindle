//! An embedded database backed `Provider` implementation.
//!
//! Currently, the underlying storage engine uses the [sled embedded
//! database](https://github.com/spacejam/sled). This database provides caching support as well as
//! atomic operations. Underneath the hood, we encode the data using the
//! [CBOR](https://github.com/pyfisch/cbor) format for efficient serialization/deserialization from
//! the database.
//!
//! This provider is currently experimental, with the goal of replacing the `FileProvider` as the
//! default provider in the future.
//!
//! This will only be available if the `provider` feature is enabled

use std::convert::TryInto;
use std::path::Path;
use std::sync::Arc;

use sha2::{Digest, Sha256};
use sled::Error as SledError;
use tokio::io::AsyncReadExt;
use tokio::sync::Semaphore;
use tokio_stream::{Stream, StreamExt};
use tokio_util::codec::{BytesCodec, FramedRead};
use tokio_util::io::StreamReader;
use tracing::{debug, error, info, instrument, trace, warn};
use tracing_futures::Instrument;

use crate::provider::{Provider, ProviderError, Result};
use crate::search::Search;
use crate::verification::Verified;
use crate::{Id, Signed};

const INVOICE_DB_NAME: &str = "invoices";
const PARCEL_DB_NAME: &str = "parcels";
// TODO: This number should be equal to the number of threads configured for blocking. We could
// expose this value in the constructor, but that feels too much like a low-level detail to expose
// in the API. But I also can't find a way to fetch this configured value
const BLOCKING_THREAD_COUNT: usize = 512;

/// An embedded database backend for storing and retrieving bindles and parcels.
///
/// Given a storage directory, EmbeddedProvider brings its own storage layout for keeping track of
/// Bindles.
///
/// An EmbeddedProvider needs a search engine implementation. When invoices are created or yanked,
/// the index will be updated.
pub struct EmbeddedProvider<T> {
    invoices: sled::Tree,
    parcels: sled::Tree,
    index: T,
    semaphore: Arc<Semaphore>,
}

impl<T: Clone> Clone for EmbeddedProvider<T> {
    fn clone(&self) -> Self {
        EmbeddedProvider {
            invoices: self.invoices.clone(),
            parcels: self.parcels.clone(),
            index: self.index.clone(),
            semaphore: self.semaphore.clone(),
        }
    }
}

impl<T: Search + Send + Sync> EmbeddedProvider<T> {
    pub async fn new<P: AsRef<Path>>(storage_path: P, index: T) -> anyhow::Result<Self> {
        debug!(storage_path = %storage_path.as_ref().display(), "Creating new embedded provider");
        let sp = storage_path.as_ref().to_owned();
        let db = tokio::task::spawn_blocking(|| sled::open(sp)).await??;
        let owned = db.clone();
        let invoices =
            tokio::task::spawn_blocking(move || owned.open_tree(INVOICE_DB_NAME)).await??;
        let parcels = tokio::task::spawn_blocking(move || db.open_tree(PARCEL_DB_NAME)).await??;
        let emb = EmbeddedProvider {
            invoices,
            parcels,
            index,
            semaphore: Arc::new(Semaphore::new(BLOCKING_THREAD_COUNT)),
        };
        debug!("warming index");
        if let Err(e) = emb.warm_index().await {
            warn!(error = %e, "Error warming index");
        }
        Ok(emb)
    }

    /// This warms the index by loading all of the invoices currently in the DB
    ///
    /// Warming the index is something that the storage backend should do, though I am
    /// not sure whether EVERY storage backend should do it. It is the responsibility of
    /// storage because storage is the sole authority about what documents are actually
    /// in the repository. So it needs to communicate (on startup) what documents it knows
    /// about. The storage engine merely needs to store any non-duplicates. So we can
    /// safely insert, but ignore errors that come back because of duplicate entries.
    #[instrument(level = "trace", skip(self))]
    async fn warm_index(&self) -> anyhow::Result<()> {
        // Read all invoices
        info!("Beginning index warm");
        let mut total_indexed: u64 = 0;
        // NOTE(thomastaylor312): Trying to do this async and spawn blocking is impossible unless we
        // add a clone constraint to T. So technically this could cause a blocking issue depending
        // on the cache size and if there are other IO operations (though it does have the advantage
        // of filling the cache). However, I think this is fine as we only call this on startup
        for res in self.invoices.iter() {
            let (key, raw) = res.map_err(map_sled_error)?;
            let sha = String::from_utf8_lossy(key.as_ref());
            let invoice: crate::Invoice = serde_cbor::from_slice(raw.as_ref())?;

            let digest = invoice.canonical_name();
            if sha != digest {
                anyhow::bail!(
                    "SHA {} did not match computed digest {}. Delete this record.",
                    sha,
                    digest
                );
            }

            if let Err(e) = self.index.index(&invoice).await {
                error!(invoice_id = %invoice.bindle.id, error = %e, "Error indexing invoice");
            }
            total_indexed += 1;
        }
        debug!(total_indexed, "Warmed index");
        Ok(())
    }
}

#[async_trait::async_trait]
impl<T: crate::search::Search + Send + Sync> Provider for EmbeddedProvider<T> {
    #[instrument(level = "trace", skip(self, invoice), fields(invoice_id = tracing::field::Empty))]
    async fn create_invoice<I>(&self, invoice: I) -> Result<(crate::Invoice, Vec<crate::Label>)>
    where
        I: Signed + Verified + Send + Sync,
    {
        let inv = invoice.signed();
        tracing::span::Span::current()
            .record("invoice_id", &tracing::field::display(&inv.bindle.id));
        // It is illegal to create a yanked invoice.
        if inv.yanked.unwrap_or(false) {
            debug!(id = %inv.bindle.id, "Invoice being created is set to yanked");
            return Err(ProviderError::CreateYanked);
        }

        let invoice_id = inv.canonical_name();

        let invoices = self.invoices.clone();

        let serialized = serde_cbor::to_vec(&inv)?;

        debug!("Inserting invoice into database");
        let res = spawn_lock(self.semaphore.clone(), move || {
            invoices.compare_and_swap(&invoice_id, None as Option<&[u8]>, Some(serialized))
        })
        .await?;

        match res {
            Ok(Ok(())) => (),
            Err(e) => return Err(map_sled_error(e)),
            // We'll only get a compare and swap error if it already exists
            Ok(Err(_)) => return Err(ProviderError::Exists),
        }

        // Attempt to update the index. Right now, we log an error if the index update
        // fails.
        if let Err(e) = self.index.index(&inv).await {
            error!(error = %e, "Error indexing new invoice");
        }

        // if there are no parcels, bail early
        if inv.parcel.is_none() {
            return Ok((inv, Vec::with_capacity(0)));
        }

        trace!("Checking for missing parcels listed in newly created invoice");
        let s = self.semaphore.clone();
        let parcels = self.parcels.clone();
        // Loop through the boxes and see what exists
        let missing = inv
            .parcel
            // Need to clone so we can move into the spawn_lock
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(|k| (s.clone(), parcels.clone(), k.label))
            .map(|(s, parcels, label)| async move {
                // Check if the parcel exists in the database
                let sha = label.sha256.to_owned();
                let found = spawn_lock(s, move || parcels.contains_key(&sha).unwrap_or(false))
                    .await
                    .unwrap_or(false);
                if found {
                    None
                } else {
                    Some(label)
                }
            });

        let labels = futures::future::join_all(missing)
            .instrument(tracing::trace_span!("lookup_missing"))
            .await
            .into_iter()
            .flatten()
            .collect();
        Ok((inv, labels))
    }

    #[instrument(level = "trace", skip(self, id), fields(id))]
    async fn get_yanked_invoice<I>(&self, id: I) -> Result<crate::Invoice>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
    {
        let parsed_id: Id = id.try_into().map_err(|e| e.into())?;
        tracing::Span::current().record("id", &tracing::field::display(&parsed_id));

        // NOTE: sled has its own caching, so we don't need to worry about manually implementing
        // here
        debug!("Getting invoice from database");

        let invoice_id = parsed_id.sha();
        let invoices = self.invoices.clone();
        let data = match spawn_lock(self.semaphore.clone(), move || invoices.get(&invoice_id))
            .await?
            .map_err(map_sled_error)?
        {
            Some(d) => d,
            None => return Err(ProviderError::NotFound),
        };

        // Parse
        trace!("Parsing invoice from raw data");
        let invoice: crate::Invoice = serde_cbor::from_slice(data.as_ref())?;

        // Return object
        Ok(invoice)
    }

    #[instrument(level = "trace", skip(self, id), fields(id))]
    async fn yank_invoice<I>(&self, id: I) -> Result<()>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
    {
        let parsed_id = id.try_into().map_err(|e| e.into())?;
        tracing::Span::current().record("id", &tracing::field::display(&parsed_id));
        trace!("Fetching invoice from storage");
        let mut inv = self.get_yanked_invoice(&parsed_id).await?;
        inv.yanked = Some(true);

        debug!("Yanking invoice");

        // NOTE: Using the update_and_fetch method would result in a double deserialization step so
        // we can re-index. There _is_ a small possibility that someone could fetch the current
        // value from the DB right before we mutate, but the consequences of this are likely small
        // or non-existent, so we aren't worrying about wrapping in a transaction

        // Attempt to update the index. Right now, we log an error if the index update
        // fails.
        trace!("Indexing yanked invoice");
        if let Err(e) = self.index.index(&inv).await {
            error!(error = %e, "Error indexing yanked invoice");
        }

        // Encode the invoice into a TOML object
        trace!("Encoding invoice");
        let serialized = serde_cbor::to_vec(&inv)?;
        let invoice_id = inv.canonical_name();
        let invoices = self.invoices.clone();
        debug!("Writing yanked invoice to database");
        spawn_lock(self.semaphore.clone(), move || {
            invoices.insert(&invoice_id, serialized)
        })
        .await?
        .map_err(map_sled_error)?;

        Ok(())
    }

    #[instrument(level = "trace", skip(self, bindle_id, data), fields(id))]
    async fn create_parcel<I, R, B>(&self, bindle_id: I, parcel_id: &str, data: R) -> Result<()>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
        R: Stream<Item = std::io::Result<B>> + Unpin + Send + Sync + 'static,
        B: bytes::Buf + Send,
    {
        debug!("Validating bindle -> parcel relationship");
        let parsed_id = bindle_id.try_into().map_err(|e| e.into())?;
        tracing::Span::current().record("id", &tracing::field::display(&parsed_id));
        let label = self.validate_parcel(parsed_id, parcel_id).await?;

        debug!("Reading data from stream");

        // Read the data into memory (it is going to start there anyway in the database before
        // getting flushed to disk)
        let mut parcel_data: Vec<u8> = Vec::with_capacity(label.size as usize);
        StreamReader::new(
            data.map(|res| res.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))),
        )
        .read_to_end(&mut parcel_data)
        .await?;

        debug!("Validating size");
        if parcel_data.len() as u64 != label.size {
            info!(
                expected = label.size,
                read_bytes = parcel_data.len(),
                "Attempted to insert parcel with incorrect size"
            );
            return Err(ProviderError::SizeMismatch);
        }

        debug!("Validating sha");
        let calculated = format!("{:x}", Sha256::digest(&parcel_data));
        if label.sha256 != calculated {
            info!(expected_sha = %label.sha256, %calculated, "Mismatched SHA when creating parcel");
            return Err(ProviderError::DigestMismatch);
        }

        debug!("Inserting parcel into database");
        let parcels = self.parcels.clone();
        let pid = parcel_id.to_owned();
        let res = spawn_lock(self.semaphore.clone(), move || {
            parcels.compare_and_swap(&pid, None as Option<&[u8]>, Some(parcel_data))
        })
        .await?;

        match res {
            Ok(Ok(())) => Ok(()),
            Err(e) => Err(map_sled_error(e)),
            // This error is only possible if the parcel already exists
            Ok(Err(_)) => Err(ProviderError::Exists),
        }
    }

    #[instrument(level = "trace", skip(self, bindle_id), fields(id))]
    async fn get_parcel<I>(
        &self,
        bindle_id: I,
        parcel_id: &str,
    ) -> Result<Box<dyn Stream<Item = Result<bytes::Bytes>> + Unpin + Send + Sync>>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
    {
        debug!("Validating bindle -> parcel relationship");
        let parsed_id = bindle_id.try_into().map_err(|e| e.into())?;
        tracing::Span::current().record("id", &tracing::field::display(&parsed_id));
        self.validate_parcel(parsed_id, parcel_id).await?;

        debug!("Getting parcel from storage");
        let parcels = self.parcels.clone();
        let pid = parcel_id.to_owned();
        let data = match spawn_lock(self.semaphore.clone(), move || parcels.get(&pid))
            .await?
            .map_err(map_sled_error)?
        {
            // Wrap the data in a cursor so it implements AsyncRead and can be streamed
            Some(d) => std::io::Cursor::new(d),
            None => return Err(ProviderError::NotFound),
        };

        Ok::<Box<dyn Stream<Item = Result<bytes::Bytes>> + Unpin + Send + Sync>, _>(Box::new(
            FramedRead::new(data, BytesCodec::new())
                .map(|res| res.map_err(map_io_error).map(|b| b.freeze())),
        ))
    }

    #[instrument(level = "trace", skip(self, bindle_id), fields(id))]
    async fn parcel_exists<I>(&self, bindle_id: I, parcel_id: &str) -> Result<bool>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
    {
        debug!("Validating bindle -> parcel relationship");
        let parsed_id = bindle_id.try_into().map_err(|e| e.into())?;
        tracing::Span::current().record("id", &tracing::field::display(&parsed_id));
        self.validate_parcel(parsed_id, parcel_id).await?;

        debug!("Checking if parcel exists in storage");
        let pid = parcel_id.to_owned();
        let parcels = self.parcels.clone();
        spawn_lock(self.semaphore.clone(), move || parcels.contains_key(&pid))
            .await?
            .map_err(map_sled_error)
    }
}

fn map_io_error(e: std::io::Error) -> ProviderError {
    if matches!(e.kind(), std::io::ErrorKind::NotFound) {
        return ProviderError::NotFound;
    }
    ProviderError::from(e)
}

fn map_sled_error(e: SledError) -> ProviderError {
    match &e {
        // This is a panicable error because if the collection is somehow gone, we can't keep
        // continuing
        SledError::CollectionNotFound(e) => panic!(
            "The collection {} was not found, something is wrong with the database",
            String::from_utf8_lossy(e)
        ),
        SledError::Io(i) => {
            error!(error = ?e, "IO error occurred while accessingata store");
            // Add some more decoration as to _where_ the IO error came from
            ProviderError::Io(std::io::Error::new(
                i.kind(),
                format!("Error accessing local data store: {}", i),
            ))
        }
        SledError::Unsupported(_) | SledError::ReportableBug(_) => {
            error!(error = ?e, "Error while attempting to access embedded data store");
            ProviderError::Other(String::from(
                "Internal system error while performing data storage lookup",
            ))
        }
        SledError::Corruption { at, bt } => {
            // This is a panicable error as it means the data store is corrupted and we
            // no longer have all our data
            panic!(
                "Detected database corruption at: {:?}, with backtrace of: {:?}",
                at, bt
            )
        }
    }
}

/// A helper function that wraps `spawn_blocking` with a semaphore permit acquisition
async fn spawn_lock<F, R>(semaphore: Arc<Semaphore>, f: F) -> Result<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    // According to the docs, the only error that can occur here is if the semaphore is closed.
    // In that case, we should panic as it should never close while the application is running
    let _permit = semaphore
        .acquire()
        .await
        .expect("Unable to synchronize threads...aborting");
    trace!(
        remaining_permits = semaphore.available_permits(),
        "Successfully acquired spawn_blocking permit"
    );
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| ProviderError::Other("Internal error: unable to lock task".into()))
}
