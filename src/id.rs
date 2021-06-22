//! Contains the implementations for bindle ID representations, which are composed of a name (with
//! possible path delimitation) and a semver compatible version
use std::convert::TryFrom;
use std::fmt;
use std::str::FromStr;

use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("Invalid bindle ID {0}. A bindle ID should be NAME/VERSION")]
    InvalidId(String),
    // TODO: Add an error message so we can pass through the parse error from semver
    #[error("ID does not contain a valid semantic version (e.g. 1.2.3): {0}")]
    InvalidSemver(String),
}

type Result<T> = std::result::Result<T, ParseError>;

const PATH_SEPARATOR: char = '/';

/// A parsed representation of an ID string for a bindle. This is currently defined as an arbitrary
/// path with a version string at the end.
///
/// Examples of valid ID strings include:
///
/// - `foo/0.1.0`
/// - `example.com/foo/1.2.3`
/// - `example.com/a/longer/path/foo/1.10.0-rc.1`
///
/// An `Id` can be parsed from any string using the `.parse()` method:
/// ```
/// use bindle::Id;
///
/// let id: Id = "example.com/foo/1.2.3".parse().expect("should parse");
/// println!("{}", id);
/// ```
///
/// An `Id` can also be parsed from any string using the `TryFrom` or `TryInto` trait:
/// ```
/// use bindle::Id;
/// use std::convert::TryInto;
///
/// let id: Id = String::from("example.com/a/longer/path/foo/1.10.0-rc.1")
///     .try_into().expect("should parse");
/// println!("{}", id);
/// ```
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, Hash, PartialEq, Eq)]
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
    /// name. The hash is guaranteed to be in the character set `[a-zA-Z0-9]`.
    pub fn sha(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(&self.name);
        // Add in the slash between the name and the version
        hasher.update("/");
        hasher.update(self.version_string());
        let result = hasher.finalize();
        format!("{:x}", result)
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        format_args!("{}{}{}", self.name, PATH_SEPARATOR, self.version).fmt(f)
    }
}

impl FromStr for Id {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self> {
        // Every ID should contain at least one separator or it is invalid
        let last_separator_index = match s.rfind(PATH_SEPARATOR) {
            Some(i) => i,
            None => return Err(ParseError::InvalidId(s.to_owned())),
        };

        let (name_part, version_part) = s.split_at(last_separator_index);

        // The split still returns the separator, so trim it off
        let version_part = version_part.trim_start_matches(PATH_SEPARATOR);

        if name_part.is_empty() || version_part.is_empty() {
            let msg = format!("name: '{}', version: '{}'", name_part, version_part);
            return Err(ParseError::InvalidId(msg));
        }

        let version = version_part
            .parse()
            .map_err(|_| ParseError::InvalidSemver(version_part.to_owned()))?;

        Ok(Id {
            name: name_part.to_owned(),
            version,
        })
    }
}

impl From<&Id> for Id {
    fn from(id: &Id) -> Self {
        id.to_owned()
    }
}

// Unfortunately I can't do a generic implementation using AsRef<str> due to this issue in Rust with
// the blanket implementation: https://github.com/rust-lang/rust/issues/50133. So we get _all_ the
// implementations
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

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_id_parsing() {
        // Valid paths
        Id::from_str("foo/1.0.0").expect("Should parse simple ID");
        Id::from_str("example.com/foo/1.0.0").expect("Should parse namespaced ID");
        Id::from_str("example.com/a/long/path/foo/1.0.0").expect("Should parse long ID");
        Id::from_str("example.com/foo/1.0.0-rc.1").expect("Should parse RC version ID");

        // Invalid paths
        assert!(
            Id::from_str("foo/").is_err(),
            "Missing version should fail parsing"
        );
        assert!(
            Id::from_str("1.0.0").is_err(),
            "Missing name should fail parsing"
        );
    }
}
