//! A cache that doesn't ever expire entries, generally for use by a client storing bindles on disk
use std::convert::TryInto;

use tokio_stream::{Stream, StreamExt};
use tracing::{debug, instrument, trace, warn};
use tracing_futures::Instrument;

use super::{into_cache_result, Cache};
use crate::provider::{Provider, ProviderError, Result};
use crate::Id;

/// A cache that doesn't ever expire entries. It fills the cache by requesting bindles from a bindle
/// server using the configured client and stores them in the given storage implementation
#[derive(Clone)]
pub struct DumbCache<Local: Provider + Clone, Remote: Provider + Clone> {
    remote: Remote,
    local: Local,
}

impl<Local: Provider + Clone, Remote: Provider + Clone> DumbCache<Local, Remote> {
    pub fn new(remote: Remote, local: Local) -> DumbCache<Local, Remote> {
        DumbCache { remote, local }
    }
}

impl<Local, Remote> Cache for DumbCache<Local, Remote>
where
    Local: Provider + Send + Sync + Clone,
    Remote: Provider + Send + Sync + Clone,
{
}

#[async_trait::async_trait]
impl<Local, Remote> Provider for DumbCache<Local, Remote>
where
    Local: Provider + Send + Sync + Clone,
    Remote: Provider + Send + Sync + Clone,
{
    async fn create_invoice(&self, _: &crate::Invoice) -> Result<Vec<crate::Label>> {
        Err(ProviderError::Other(
            "This cache implementation does not allow for creation of invoices".to_string(),
        ))
    }

    // Load an invoice, even if it is yanked.
    #[instrument(level = "trace", skip(self, id))]
    async fn get_yanked_invoice<I>(&self, id: I) -> Result<crate::Invoice>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
    {
        let parsed_id: Id = id.try_into().map_err(|e| e.into())?;
        let possible_entry = into_cache_result(self.local.get_yanked_invoice(&parsed_id).await)?;
        match possible_entry {
            Some(inv) => Ok(inv),
            None => {
                async {
                    debug!(
                        "Cache miss for invoice, attempting to fetch from server",
                    );
                    let inv = self.remote.get_yanked_invoice(&parsed_id).await?;
                    // Attempt to insert the invoice into the store, if it fails, warn the user and return the invoice anyway
                    trace!("Attempting to store invoice in cache");
                    if let Err(e) = self.local.create_invoice(&inv).await {
                        warn!(error = %e, "Fetched invoice from server, but encountered error when trying to save to local store");
                    }
                    Ok(inv)
                }.instrument(tracing::trace_span!("get_invoice_cache_miss", invoice_id = %parsed_id)).await
            }
        }
    }

    #[instrument(level = "trace", skip(self, id))]
    async fn yank_invoice<I>(&self, id: I) -> Result<()>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
    {
        // This is just an update of the local cache
        self.local.yank_invoice(id).await
    }

    async fn create_parcel<I, R, B>(&self, _: I, _: &str, _: R) -> Result<()>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
        R: Stream<Item = std::io::Result<B>> + Unpin + Send + Sync,
        B: bytes::Buf + Send,
    {
        Err(ProviderError::Other(
            "This cache implementation does not allow for creation of parcels".to_string(),
        ))
    }

    #[instrument(level = "trace", skip(self, bindle_id))]
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
        let possible_entry = into_cache_result(self.local.get_parcel(&parsed_id, parcel_id).await)?;
        match possible_entry {
            Some(parcel) => Ok(parcel),
            None => {
                async {
                    debug!(
                        "Cache miss for parcel, attempting to fetch from server"
                    );
                    let label = crate::Label {
                        sha256: parcel_id.to_owned(),
                        name: "".to_string(),
                        ..crate::Label::default()
                    };
                    let stream = self
                        .remote
                        .get_parcel(&parsed_id, parcel_id)
                        .await?
                        // This isn't my favorite. Right now we are mapping to an io error which will be mapped back to a storage error
                        .map(|res| {
                            res.map_err(|e| {
                                std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
                            })
                        });
                    // Attempt to insert the parcel into the store, if it fails, warn the user and
                    // return the parcel anyway. Either way, we need to refetch the stream, since it has
                    // been read after we try to insert
                    trace!("Attempting to store parcel in cache");
                    let stream = match self
                        .local
                        .create_parcel(&parsed_id, &label.sha256, stream)
                        .await
                    {
                        Ok(_) => return self.local.get_parcel(&parsed_id, parcel_id).await,
                        Err(e) => {
                            warn!("Fetched parcel from server, but encountered error when trying to save to local store: {:?}", e);
                            self.remote.get_parcel(&parsed_id, parcel_id).await?
                        }
                    };
                    Ok(Box::new(stream.map(|res| res.map_err(ProviderError::from))))
                }.instrument(tracing::trace_span!("get_parcel_cache_miss", invoice_id = %parsed_id, parcel_id)).await
            }
        }
    }

    // In a cache implementation, this just checks for if the local provider has it
    #[instrument(level = "trace", skip(self, bindle_id))]
    async fn parcel_exists<I>(&self, bindle_id: I, parcel_id: &str) -> Result<bool>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
    {
        self.local.parcel_exists(bindle_id, parcel_id).await
    }
}
