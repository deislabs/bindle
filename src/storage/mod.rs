//! The `Storage` trait definition and various implementations

pub mod file;

#[cfg(test)]
pub(crate) mod test_common;

use std::convert::TryInto;

use thiserror::Error;
use tokio::stream::Stream;

use crate::id::ParseError;
use crate::Id;

/// A custom shorthand result type that always has an error type of [`StorageError`](StorageError)
pub type Result<T> = core::result::Result<T, StorageError>;

/// The basic functionality required for bindle to use something as a storage engine.
///
/// Please note that due to this being an `async_trait`, the types might look complicated. Look at
/// the code directly to see the simpler function signatures for implementation
#[async_trait::async_trait]
pub trait Storage {
    /// This takes an invoice and creates it in storage.
    ///
    /// It must verify that each referenced parcel is present in storage. Any parcel that is not
    /// present must be returned in the list of IDs.
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

    /// Load an invoice, even if it is yanked. This is called by the default implementation of
    /// `get_invoice`
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
    /// `Stream`
    async fn create_parcel<R, B>(&self, label: &super::Label, data: &mut R) -> Result<()>
    where
        R: Stream<Item = std::io::Result<B>> + Unpin + Send + Sync,
        B: bytes::Buf;

    /// Get a specific parcel using its SHA
    async fn get_parcel(
        &self,
        parcel_id: &str,
    ) -> Result<Box<dyn Stream<Item = Result<bytes::Bytes>> + Unpin + Send + Sync>>;

    /// Get the label for a parcel
    ///
    /// This reads the label from storage and then parses it into a Label object.
    async fn get_label(&self, parcel_id: &str) -> Result<crate::Label>;
}

/// StorageError describes the possible error states when storing and retrieving bindles.
#[derive(Error, Debug)]
pub enum StorageError {
    /// The invoice being accessed has been yanked
    #[error("bindle is yanked")]
    Yanked,
    /// The error returned when the invoice is valid, but is already set to yanked
    #[error("bindle cannot be created as yanked")]
    CreateYanked,
    /// When the resource is not found in the store
    #[error("resource not found: if an item does not appear in our records, it does not exist!")]
    NotFound,
    /// Any errors that occur due to IO issues. Contains the underlying IO `Error`
    #[error("resource could not be loaded: {0:?}")]
    Io(#[from] std::io::Error),
    /// The resource being created already exists in the system
    #[error("resource already exists")]
    Exists,
    /// The error returned when the given `Id` was invalid and unable to be parsed
    #[error("invalid ID given")]
    InvalidId,
    /// An uploaded parcel does not match the SHA-256 sum provided with its label
    #[error("digest does not match")]
    DigestMismatch,
    /// An error that occurs when the storage implementation uses a cache and filling that cache
    /// from another source encounters an error. Only available with the `caching` feature enabled
    #[cfg(feature = "caching")]
    #[error("cache fill error: {0:?}")]
    CacheError(#[from] crate::client::ClientError),

    /// The data cannot be properly deserialized from TOML
    #[error("resource is malformed: {0:?}")]
    Malformed(#[from] toml::de::Error),
    /// The data cannot be properly serialized from TOML
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
