//! A cache that doesn't ever expire entries, generally for use by a client storing bindles on disk
use std::convert::TryInto;

use log::{info, warn};
use tokio::stream::{Stream, StreamExt};

use super::{into_cache_result, Cache};
use crate::client::Client;
use crate::provider::{Provider, ProviderError, Result};
use crate::Id;

/// A cache that doesn't ever expire entries. It fills the cache by requesting bindles from a bindle
/// server using the configured client and stores them in the given storage implementation
#[derive(Clone)]
pub struct DumbCache<P: Provider + Clone> {
    client: Client,
    inner: P,
}

impl<P: Provider + Clone> DumbCache<P> {
    pub fn new(client: Client, provider: P) -> DumbCache<P> {
        DumbCache {
            client,
            inner: provider,
        }
    }
}

impl<P: Provider + Send + Sync + Clone> Cache for DumbCache<P> {}

#[async_trait::async_trait]
impl<P: Provider + Send + Sync + Clone> Provider for DumbCache<P> {
    async fn create_invoice(&self, _: &crate::Invoice) -> Result<Vec<crate::Label>> {
        Err(ProviderError::Other(
            "This cache implementation does not allow for creation of invoices".to_string(),
        ))
    }

    // Load an invoice, even if it is yanked.
    async fn get_yanked_invoice<I>(&self, id: I) -> Result<crate::Invoice>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
    {
        let parsed_id: Id = id.try_into().map_err(|e| e.into())?;
        let possible_entry = into_cache_result(self.inner.get_yanked_invoice(&parsed_id).await)?;
        match possible_entry {
            Some(inv) => Ok(inv),
            None => {
                info!(
                    "Cache miss for invoice {}, attempting to fetch from server",
                    parsed_id
                );
                let inv = self.client.get_invoice(parsed_id).await?;
                // Attempt to insert the invoice into the store, if it fails, warn the user and return the invoice anyway
                if let Err(e) = self.inner.create_invoice(&inv).await {
                    warn!("Fetched invoice from server, but encountered error when trying to save to local store: {:?}", e);
                }
                Ok(inv)
            }
        }
    }

    async fn yank_invoice<I>(&self, id: I) -> Result<()>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
    {
        // This is just an update of the local cache
        self.inner.yank_invoice(id).await
    }

    async fn create_parcel<I, R, B>(&self, _: I, _: &str, _: R) -> Result<()>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
        R: Stream<Item = std::io::Result<B>> + Unpin + Send + Sync,
        B: bytes::Buf,
    {
        Err(ProviderError::Other(
            "This cache implementation does not allow for creation of parcels".to_string(),
        ))
    }

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
        let possible_entry = into_cache_result(self.inner.get_parcel(&parsed_id, parcel_id).await)?;
        match possible_entry {
            Some(parcel) => Ok(parcel),
            None => {
                info!(
                    "Cache miss for parcel {}, attempting to fetch from server",
                    parcel_id
                );
                let label = crate::Label {
                    sha256: parcel_id.to_owned(),
                    name: "".to_string(),
                    ..crate::Label::default()
                };
                let stream = self
                    .client
                    .get_parcel_stream(parsed_id.clone(), parcel_id)
                    .await?
                    // This isn't my favorite. Right now we are mapping a client error to an io error, which will be mapped back to a storage error
                    .map(|res| {
                        res.map_err(|e| {
                            std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
                        })
                    });
                // Attempt to insert the parcel into the store, if it fails, warn the user and
                // return the parcel anyway. Either way, we need to refetch the stream, since it has
                // been read after we try to insert
                let stream = match self
                    .inner
                    .create_parcel(&parsed_id, &label.sha256, stream)
                    .await
                {
                    Ok(_) => return self.inner.get_parcel(parsed_id.clone(), parcel_id).await,
                    Err(e) => {
                        warn!("Fetched parcel from server, but encountered error when trying to save to local store: {:?}", e);
                        self.client
                            .get_parcel_stream(parsed_id.clone(), parcel_id)
                            .await?
                    }
                };
                Ok(Box::new(stream.map(|res| res.map_err(ProviderError::from))))
            }
        }
    }

    // In a cache implementation, this just checks for if the inner store has it
    async fn parcel_exists<I>(&self, bindle_id: I, parcel_id: &str) -> Result<bool>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
    {
        self.inner.parcel_exists(bindle_id, parcel_id).await
    }
}
