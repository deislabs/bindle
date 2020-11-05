pub mod file;

#[cfg(test)]
pub(crate) mod test_common;

use std::convert::{TryFrom, TryInto};
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use thiserror::Error;
use tokio::io::AsyncRead;

pub type Result<T> = core::result::Result<T, StorageError>;

#[async_trait::async_trait]
pub trait Storage {
    /// This takes an invoice and creates it in storage.
    /// It must verify that each referenced box is present in storage. Any box that
    /// is not present must be returned in the list of IDs.
    async fn create_invoice(&self, inv: &super::Invoice) -> Result<Vec<super::Label>>;
    // Load an invoice and return it
    //
    // This will return an invoice if the bindle exists and is not yanked
    async fn get_invoice<I>(&self, id: I) -> Result<super::Invoice>
    where
        I: TryInto<Id, Error = StorageError> + Send;
    // Load an invoice, even if it is yanked.
    async fn get_yanked_invoice<I>(&self, id: I) -> Result<super::Invoice>
    where
        I: TryInto<Id, Error = StorageError> + Send;
    // Remove an invoice by ID
    async fn yank_invoice<I>(&self, id: I) -> Result<()>
    where
        I: TryInto<Id, Error = StorageError> + Send;
    async fn create_parcel<R: AsyncRead + Unpin + Send + Sync>(
        &self,
        label: &super::Label,
        data: &mut R,
    ) -> Result<()>;

    async fn get_parcel(&self, label: &crate::Label) -> Result<Box<dyn AsyncRead + Unpin>>;
    // Get the label for a parcel
    //
    // This reads the label from storage and then parses it into a Label object.
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
    #[error("resource could not be loaded")]
    IO(#[from] std::io::Error),
    #[error("resource already exists")]
    Exists,
    #[error("Invalid ID given")]
    InvalidId,
    #[error("digest does not match")]
    DigestMismatch,

    // TODO: Investigate how to make this more helpful
    #[error("resource is malformed")]
    Malformed(#[from] toml::de::Error),
    #[error("resource cannot be stored")]
    Unserializable(#[from] toml::ser::Error),
}

/// A parsed representation of an ID string for a bindle. This is currently defined as an arbitrary
/// path with a version string at the end. Examples of valid ID strings include:
/// `foo/0.1.0`
/// `example.com/foo/1.2.3`
/// `example.com/a/longer/path/foo/1.10.0-rc.1`
#[derive(Clone, Debug)]
pub struct Id {
    name: String,
    version: String,
}

impl Id {
    /// Returns the name part of the ID
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the version part of the ID
    pub fn version(&self) -> &str {
        &self.version
    }

    fn parse_from_path<P: AsRef<Path>>(id_path: P) -> Result<Self> {
        let ref_path = id_path.as_ref();
        let parent = match ref_path.parent() {
            Some(p) => p,
            None => return Err(StorageError::InvalidId),
        };

        let name = match parent.to_str() {
            Some(s) if !s.is_empty() => s.to_owned(),
            _ => return Err(StorageError::InvalidId),
        };

        let version_part = match ref_path.file_name() {
            Some(s) => s,
            None => return Err(StorageError::InvalidId),
        };

        let version = match version_part.to_str() {
            Some(s) if !s.is_empty() => s.to_owned(),
            _ => return Err(StorageError::InvalidId),
        };

        Ok(Id { name, version })
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // NOTE: If we find that every Storage implementation is using the
        // `canonical_invoice_name_strings` method of hashing, we can do that here instead
        Path::new(&self.name).join(&self.version).display().fmt(f)
    }
}

impl FromStr for Id {
    type Err = StorageError;

    fn from_str(s: &str) -> Result<Self> {
        let id_path = Path::new(s);
        Self::parse_from_path(id_path)
    }
}

// Unfortunately I can't do a generic implementation using AsRef<str>/AsRef<Path> due to this issue
// in Rust with the blanket implementation: https://github.com/rust-lang/rust/issues/50133. So we
// get _all_ the implementations
impl TryFrom<String> for Id {
    type Error = StorageError;

    fn try_from(value: String) -> Result<Self> {
        value.parse()
    }
}

impl TryFrom<&String> for Id {
    type Error = StorageError;

    fn try_from(value: &String) -> Result<Self> {
        value.parse()
    }
}

impl TryFrom<&str> for Id {
    type Error = StorageError;

    fn try_from(value: &str) -> Result<Self> {
        value.parse()
    }
}

impl TryFrom<&Path> for Id {
    type Error = StorageError;

    fn try_from(value: &Path) -> Result<Self> {
        Self::parse_from_path(value)
    }
}

impl TryFrom<PathBuf> for Id {
    type Error = StorageError;

    fn try_from(value: PathBuf) -> Result<Self> {
        Self::parse_from_path(value)
    }
}

impl TryFrom<&PathBuf> for Id {
    type Error = StorageError;

    fn try_from(value: &PathBuf) -> Result<Self> {
        Self::parse_from_path(value)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_id_parsing() {
        // Valid paths
        Id::parse_from_path("foo/1.0.0").expect("Should parse simple ID");
        Id::parse_from_path("example.com/foo/1.0.0").expect("Should parse namespaced ID");
        Id::parse_from_path("example.com/a/long/path/foo/1.0.0").expect("Should parse long ID");
        // Obviously this doesn't matter right now, but if we want to start parsing versions in the
        // future, it will
        Id::parse_from_path("example.com/foo/1.0.0-rc.1").expect("Should parse RC version ID");

        // Invalid paths
        assert!(
            Id::parse_from_path("foo/").is_err(),
            "Missing version should fail parsing"
        );
        assert!(
            Id::parse_from_path("1.0.0").is_err(),
            "Missing name should fail parsing"
        );
    }
}
