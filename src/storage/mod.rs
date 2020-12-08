pub mod file;

#[cfg(test)]
pub(crate) mod test_common;

use std::convert::TryInto;

use thiserror::Error;
use tokio::io::AsyncRead;

use crate::id::ParseError;
use crate::Id;

pub type Result<T> = core::result::Result<T, StorageError>;

#[async_trait::async_trait]
pub trait Storage {
    /// This takes an invoice and creates it in storage.
    /// It must verify that each referenced box is present in storage. Any box that
    /// is not present must be returned in the list of IDs.
    async fn create_invoice(&self, inv: &super::Invoice) -> Result<Vec<super::Label>>;

    /// Load an invoice and return it
    ///
    /// This will return an invoice if the bindle exists and is not yanked. The default
    /// implementation of this method is sufficient for most use cases, but can be overridden if
    /// needed
    async fn get_invoice<I>(&self, id: I) -> Result<super::Invoice>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<StorageError>,
    {
        match self.get_yanked_invoice(id).await {
            Ok(inv) if !inv.yanked.unwrap_or(false) => Ok(inv),
            Err(e) => Err(e),
            _ => Err(StorageError::Yanked),
        }
    }

    /// Load an invoice, even if it is yanked.
    async fn get_yanked_invoice<I>(&self, id: I) -> Result<super::Invoice>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<StorageError>;

    /// Remove an invoice by ID
    async fn yank_invoice<I>(&self, id: I) -> Result<()>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<StorageError>;

    /// Creates a parcel with the associated label data. The parcel can be anything that implements
    /// `AsyncRead`
    async fn create_parcel<R: AsyncRead + Unpin + Send + Sync>(
        &self,
        label: &super::Label,
        data: &mut R,
    ) -> Result<()>;

    /// Get a specific parcel using its SHA
    async fn get_parcel(&self, parcel_id: &str) -> Result<Box<dyn AsyncRead + Unpin + Send>>;

    /// Get the label for a parcel
    ///
    /// This reads the label from storage and then parses it into a Label object.
    async fn get_label(&self, parcel_id: &str) -> Result<crate::Label>;
}

/// StorageError describes the possible error states when storing and retrieving bindles.
#[derive(Error, Debug)]
pub enum StorageError {
    #[error("bindle is yanked")]
    Yanked,
    #[error("bindle cannot be created as yanked")]
    CreateYanked,
    #[error("resource not found")]
    NotFound,
    #[error("resource could not be loaded: {0:?}")]
    Io(#[from] std::io::Error),
    #[error("resource already exists")]
    Exists,
    #[error("invalid ID given")]
    InvalidId,
    #[error("digest does not match")]
    DigestMismatch,
    /// An error that occurs when the storage implementation uses a cache and filling that cache
    /// from another source encounters an error
    #[cfg(feature = "caching")]
    #[error("cache fill error: {0:?}")]
    CacheError(#[from] crate::client::ClientError),

    #[error("resource is malformed: {0:?}")]
    Malformed(#[from] toml::de::Error),
    #[error("resource cannot be stored: {0:?}")]
    Unserializable(#[from] toml::ser::Error),

    /// A catch-all for uncategorized errors. Contains an error message describing the underlying
    /// issue
    #[error("{0}")]
    Other(String),
}

impl From<ParseError> for StorageError {
    fn from(e: ParseError) -> StorageError {
        match e {
            ParseError::InvalidId | ParseError::InvalidSemver => StorageError::InvalidId,
        }
    }
}

impl From<std::convert::Infallible> for StorageError {
    fn from(_: std::convert::Infallible) -> StorageError {
        // This can never happen (by definition of infallible), so it doesn't matter what we return
        StorageError::Other("Shouldn't happen".to_string())
    }
}
