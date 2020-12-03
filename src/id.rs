/// Contains the implementations for bindle ID representations, which are composed of a name (with
/// possible path delimitation) and a semver compatible version
use std::convert::TryFrom;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("Invalid ID")]
    InvalidId,
    // TODO: Add an error message so we can pass through the parse error from semver
    #[error("ID does not contain a valid semver")]
    InvalidSemver,
}

type Result<T> = std::result::Result<T, ParseError>;

/// A parsed representation of an ID string for a bindle. This is currently defined as an arbitrary
/// path with a version string at the end. Examples of valid ID strings include:
/// `foo/0.1.0`
/// `example.com/foo/1.2.3`
/// `example.com/a/longer/path/foo/1.10.0-rc.1`
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Id {
    name: String,
    version: semver::Version,
}

impl Id {
    /// Returns the name part of the ID
    pub fn name(&self) -> &str {
        &self.name
    }

    // Returns the [`Version`](semver::Version) part of this ID
    pub fn version(&self) -> &semver::Version {
        &self.version
    }

    /// Returns the version part of the ID. This is returned as a `String` as it is a conversion
    /// from the underlying semver
    pub fn version_string(&self) -> String {
        self.version.to_string()
    }

    /// Returns the SHA256 sum of this Id for use as a common identifier
    ///
    /// We don't typically want to store a bindle with its name and version number. This
    /// would impose both naming constraints on the bindle and security issues on the
    /// storage layout. So this function hashes the name/version data (which together
    /// MUST be unique in the system) and uses the resulting hash as the canonical
    /// name. The hash is guaranteed to be in the character set [a-zA-Z0-9].
    pub fn sha(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(&self.name);
        // Add in the slash between the name and the version
        hasher.update("/");
        hasher.update(self.version_string());
        let result = hasher.finalize();
        format!("{:x}", result)
    }

    fn parse_from_path<P: AsRef<Path>>(id_path: P) -> Result<Self> {
        let ref_path = id_path.as_ref();
        let parent = match ref_path.parent() {
            Some(p) => p,
            None => return Err(ParseError::InvalidId),
        };

        let name = match parent.to_str() {
            Some(s) if !s.is_empty() => s.to_owned(),
            _ => return Err(ParseError::InvalidId),
        };

        let version_part = match ref_path.file_name() {
            Some(s) => s,
            None => return Err(ParseError::InvalidId),
        };

        let version = match version_part.to_str() {
            Some(s) if !s.is_empty() => s.to_owned(),
            _ => return Err(ParseError::InvalidId),
        };

        let version = version.parse().map_err(|_| ParseError::InvalidSemver)?;

        Ok(Id { name, version })
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // NOTE: If we find that every Storage implementation is using the
        // `canonical_invoice_name_strings` method of hashing, we can do that here instead
        Path::new(&self.name)
            .join(self.version_string())
            .display()
            .fmt(f)
    }
}

impl FromStr for Id {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self> {
        let id_path = Path::new(s);
        Self::parse_from_path(id_path)
    }
}

impl From<&Id> for Id {
    fn from(id: &Id) -> Self {
        id.clone()
    }
}

// Unfortunately I can't do a generic implementation using AsRef<str>/AsRef<Path> due to this issue
// in Rust with the blanket implementation: https://github.com/rust-lang/rust/issues/50133. So we
// get _all_ the implementations
impl TryFrom<String> for Id {
    type Error = ParseError;

    fn try_from(value: String) -> Result<Self> {
        value.parse()
    }
}

impl TryFrom<&String> for Id {
    type Error = ParseError;

    fn try_from(value: &String) -> Result<Self> {
        value.parse()
    }
}

impl TryFrom<&str> for Id {
    type Error = ParseError;

    fn try_from(value: &str) -> Result<Self> {
        value.parse()
    }
}

impl TryFrom<&Path> for Id {
    type Error = ParseError;

    fn try_from(value: &Path) -> Result<Self> {
        Self::parse_from_path(value)
    }
}

impl TryFrom<PathBuf> for Id {
    type Error = ParseError;

    fn try_from(value: PathBuf) -> Result<Self> {
        Self::parse_from_path(value)
    }
}

impl TryFrom<&PathBuf> for Id {
    type Error = ParseError;

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
