//! A cache that doesn't ever expire entries, generally for use by a client storing bindles on disk
use std::convert::TryInto;

use log::{info, warn};
use tokio::io::AsyncRead;

use super::{into_cache_result, Cache};
use crate::client::Client;
use crate::storage::{Result, Storage, StorageError};
use crate::Id;

/// A cache that doesn't ever expire entries. It fills the cache by requesting bindles from a bindle
/// server using the configured client and stores them in the given storage implementation
#[derive(Clone)]
pub struct DumbCache<S: Storage + Clone> {
    client: Client,
    inner: S,
}

impl<S: Storage + Clone> DumbCache<S> {
    pub fn new(client: Client, store: S) -> DumbCache<S> {
        DumbCache {
            client,
            inner: store,
        }
    }
}

impl<S: Storage + Send + Sync + Clone> Cache for DumbCache<S> {}

#[async_trait::async_trait]
impl<S: Storage + Send + Sync + Clone> Storage for DumbCache<S> {
    async fn create_invoice(&self, _: &crate::Invoice) -> Result<Vec<crate::Label>> {
        Err(StorageError::Other(
            "This cache implementation does not allow for creation of invoices".to_string(),
        ))
    }

    // Load an invoice, even if it is yanked.
    async fn get_yanked_invoice<I>(&self, id: I) -> Result<crate::Invoice>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<StorageError>,
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
        I::Error: Into<StorageError>,
    {
        // This is just an update of the local cache
        self.inner.yank_invoice(id).await
    }

    async fn create_parcel<R: AsyncRead + Unpin + Send + Sync>(
        &self,
        _: &crate::Label,
        _: &mut R,
    ) -> Result<()> {
        Err(StorageError::Other(
            "This cache implementation does not allow for creation of parcels".to_string(),
        ))
    }

    async fn get_parcel(
        &self,
        parcel_id: &str,
    ) -> Result<Box<dyn AsyncRead + Unpin + Send + Sync>> {
        let possible_entry = into_cache_result(self.inner.get_parcel(parcel_id).await)?;
        match possible_entry {
            Some(parcel) => Ok(parcel),
            None => {
                info!(
                    "Cache miss for parcel {}, attempting to fetch from server",
                    parcel_id
                );
                // TODO: I don't like faking the rest of the data outside of the SHA. We should
                // figure out a nice way to return that metadata in the client (perhaps as a
                // separate tuple than contains metadata and the stream)
                let label = crate::Label {
                    sha256: parcel_id.to_owned(),
                    name: "".to_string(),
                    size: 0,
                    media_type: "*/*".to_string(),
                    annotations: None,
                };
                let stream = self.client.get_parcel_stream(parcel_id).await?;
                // Attempt to insert the parcel into the store, if it fails, warn the user and
                // return the parcel anyway. Either way, we need to refetch the stream, since it has
                // been read after we try to insert
                let stream = match self
                    .inner
                    .create_parcel(&label, &mut crate::async_util::BodyReadBuffer(stream))
                    .await
                {
                    Ok(_) => return self.inner.get_parcel(parcel_id).await,
                    Err(e) => {
                        warn!("Fetched parcel from server, but encountered error when trying to save to local store: {:?}", e);
                        self.client.get_parcel_stream(parcel_id).await?
                    }
                };
                Ok(Box::new(crate::async_util::BodyReadBuffer(stream)))
            }
        }
    }

    async fn get_label(&self, _: &str) -> Result<crate::Label> {
        Err(StorageError::Other(
            "This cache implementation does not allow for fetching labels".to_string(),
        ))
    }
}
