//! The `Provider` trait definition and various implementations.
//!
//! A Bindle Provider can be anything that gives (e.g. "provides") access to Bindles. This could be
//! anything from a cache, to a proxy, or a storage backend (such as a database). Thus, Bindles can
//! be fetched from an arbitrarily complex chain of providers depending on the needs of your
//! team/code/organization.
//!
//! In general, we don't recommend too many nested chains in code
//! (`MyLocalCache<MyRemoteCache<SharedCache<CompanyCache<FileProvider>>>>` is not a happy thing in
//! code) and suggest using chains of proxies between various providers. In practice, it could look
//! like this (not real types): `LocalCache<Proxy> -> (Another server) Proxy -> (The server where
//! bindles are stored) DatabaseProvider` This pattern can be particularly useful in larger teams as
//! you could easily add another source of Bindles to your approved list of external sources at any
//! point along the chain
//!
//! ## Terminal Providers
//!
//! One concept that is not actually code, but is important to understand, is the idea of "Terminal
//! Providers." These are providers that mark the end of a chain and are generally the actual
//! storage/database backend where the Bindles exist. In general, Providers that are _not_ terminal
//! will generally contain another Provider implementation or an HTTP client to talk to another
//! server upstream

pub mod file;

use std::convert::TryInto;

use thiserror::Error;
use tokio_stream::Stream;

use crate::id::ParseError;
use crate::Id;

/// A custom shorthand result type that always has an error type of [`ProviderError`](ProviderError)
pub type Result<T> = core::result::Result<T, ProviderError>;

/// The basic functionality required for a Bindle provider.
///
/// Please note that due to this being an `async_trait`, the types might look complicated. Look at
/// the code directly to see the simpler function signatures for implementation.
///
/// IMPORTANT: If you are implementing a terminal provider, you must internally handle atomic
/// operations/locking to avoid race conditions in the back end. If you are writing one for a
/// database, this is likely handled for you by the database. However, in cases such as the built in
/// [`FileProvider`](crate::provider::file::FileProvider), you must ensure that two create
/// operations do not conflict and that a read operation of something being created also does not
/// conflict.
#[async_trait::async_trait]
pub trait Provider {
    /// This takes an invoice and creates it in storage.
    ///
    /// It must verify that each referenced parcel is present in storage. Any parcel that is not
    /// present must be returned in the list of labels.
    async fn create_invoice(&self, inv: &super::Invoice) -> Result<Vec<super::Label>>;

    /// Load an invoice and return it
    ///
    /// This will return an invoice if the bindle exists and is not yanked. The default
    /// implementation of this method is sufficient for most use cases, but can be overridden if
    /// needed
    async fn get_invoice<I>(&self, id: I) -> Result<super::Invoice>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
    {
        match self.get_yanked_invoice(id).await {
            Ok(inv) if !inv.yanked.unwrap_or(false) => Ok(inv),
            Err(e) => Err(e),
            _ => Err(ProviderError::Yanked),
        }
    }

    /// Load an invoice, even if it is yanked. This is called by the default implementation of
    /// `get_invoice`
    async fn get_yanked_invoice<I>(&self, id: I) -> Result<super::Invoice>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>;

    /// Remove an invoice by ID
    async fn yank_invoice<I>(&self, id: I) -> Result<()>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>;

    // Checks if the given parcel ID exists within an invoice. The default implementation will fetch
    // the parcel and check if the given parcel ID exists. Returns the parcel label if valid. Most
    // providers should implement some sort of caching for `get_yanked_invoice` to avoid fetching
    // the invoice every single time a parcel is requested. Provider implementations may also
    // implement this function to include other validation logic if desired.
    async fn validate_parcel<I>(&self, bindle_id: I, parcel_id: &str) -> Result<crate::Label>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
    {
        let inv = self.get_yanked_invoice(bindle_id).await?;
        match inv
            .parcel
            .unwrap_or_default()
            .into_iter()
            .find(|p| p.label.sha256 == parcel_id)
        {
            Some(p) => Ok(p.label),
            None => Err(ProviderError::NotFound),
        }
    }

    /// Creates a parcel with the associated sha. The parcel can be anything that implements
    /// `Stream`
    ///
    /// For some terminal providers, the bindle ID may not be necessary, but it is always required
    /// for an implementation. Implementors MUST validate that the length of the sent parcel is the
    /// same as specified in the invoice
    async fn create_parcel<I, R, B>(&self, bindle_id: I, parcel_id: &str, data: R) -> Result<()>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
        R: Stream<Item = std::io::Result<B>> + Unpin + Send + Sync + 'static,
        B: bytes::Buf + Send;

    /// Get a specific parcel using its SHA.
    ///
    /// For some terminal providers, the bindle ID may not be necessary, but it is always required
    /// for an implementation
    async fn get_parcel<I>(
        &self,
        bindle_id: I,
        parcel_id: &str,
    ) -> Result<Box<dyn Stream<Item = Result<bytes::Bytes>> + Unpin + Send + Sync>>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>;

    /// Checks if the given parcel exists in storage.
    ///
    /// This should not load the full parcel but only indicate if the parcel exists. For some
    /// terminal providers, the bindle ID may not be necessary, but it is always required
    async fn parcel_exists<I>(&self, bindle_id: I, parcel_id: &str) -> Result<bool>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>;
}

/// ProviderError describes the possible error states when storing and retrieving bindles.
#[derive(Error, Debug)]
pub enum ProviderError {
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
    #[error("parcel size does not match invoice")]
    SizeMismatch,
    #[error(
        "a write operation is currently in progress for this resource and it cannot be accessed"
    )]
    WriteInProgress,
    /// An error that occurs when the provider implementation uses a proxy and that proxy request
    /// encounters an error. Only available with the `client` feature enabled
    #[cfg(feature = "client")]
    #[error("proxy error: {0:?}")]
    ProxyError(#[from] crate::client::ClientError),

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

impl From<ParseError> for ProviderError {
    fn from(e: ParseError) -> ProviderError {
        match e {
            ParseError::InvalidId | ParseError::InvalidSemver => ProviderError::InvalidId,
        }
    }
}

impl From<std::convert::Infallible> for ProviderError {
    fn from(_: std::convert::Infallible) -> ProviderError {
        // This can never happen (by definition of infallible), so it doesn't matter what we return
        ProviderError::Other("Shouldn't happen".to_string())
    }
}
