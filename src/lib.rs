//! A crate for interacting with Bindles
//!
//! Bindle is an aggregate object storage system used for storing aggregate applications. For more
//! information and examples, see the
//! [README](https://github.com/deislabs/bindle/blob/master/README.md) in the Bindle repo.
//!
//! This crate is the reference implementation of the [Bindle
//! Spec](https://github.com/deislabs/bindle/blob/master/docs/bindle-spec.md) and it contains both a
//! client and a server implementation, along with various other utilities

pub mod async_util;
#[cfg(feature = "caching")]
pub mod cache;
#[cfg(feature = "client")]
pub mod client;
mod id;
pub mod provider;
#[cfg(feature = "client")]
pub mod proxy;
pub mod search;
#[cfg(feature = "server")]
pub mod server;
#[cfg(feature = "client")]
pub mod standalone;
#[cfg(feature = "test-tools")]
pub mod testing;

pub mod filters;

#[doc(inline)]
pub use id::Id;
#[doc(inline)]
pub use search::Matches;

use semver::{Compat, Version, VersionReq};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use std::collections::BTreeMap;
use std::hash::Hash;

use search::SearchOptions;

/// The version string for the v1 Bindle Spec
pub const BINDLE_VERSION_1: &str = "1.0.0";

/// Alias for feature map in an Invoice's parcel
pub type FeatureMap = BTreeMap<String, BTreeMap<String, String>>;

/// Alias for annotations map
pub type AnnotationMap = BTreeMap<String, String>;

/// The main structure for a Bindle invoice.
///
/// The invoice describes a specific version of a bindle. For example, the bindle
/// `foo/bar/1.0.0` would be represented as an Invoice with the `BindleSpec` name
/// set to `foo/bar` and version set to `1.0.0`.
///
/// Most fields on this struct are singular to best represent the specification. There,
/// fields like `group` and `parcel` are singular due to the conventions of TOML.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Invoice {
    pub bindle_version: String,
    pub yanked: Option<bool>,
    pub bindle: BindleSpec,
    pub annotations: Option<BTreeMap<String, String>>,
    pub parcel: Option<Vec<Parcel>>,
    pub group: Option<Vec<Group>>,
    pub signature: Option<Vec<Signature>>,
}

impl Invoice {
    /// produce a slash-delimited "invoice name"
    ///
    /// For example, an invoice with the bindle name "hello" and the bindle version
    /// "1.2.3" will produce "hello/1.2.3"
    pub fn name(&self) -> String {
        format!("{}/{}", self.bindle.id.name(), self.bindle.id.version())
    }

    /// Creates a standard name for an invoice
    ///
    /// This is designed to create a repeatable opaque name for the invoice
    /// We don't typically want to have a bindle ID using its name and version number. This
    /// would impose both naming constraints on the bindle and security issues on the
    /// storage layout. So this function hashes the name/version data (which together
    /// MUST be unique in the system) and uses the resulting hash as the canonical
    /// name. The hash is guaranteed to be in the character set [a-zA-Z0-9].
    pub fn canonical_name(&self) -> String {
        self.bindle.id.sha()
    }

    /// Compare a SemVer "requirement" string to the version on this bindle
    ///
    /// An empty range matches anything.
    ///
    /// A range that fails to parse matches nothing.
    ///
    /// An empty version matches nothing (unless the requirement is empty)
    ///
    /// A version that fails to parse matches nothing (unless the requirement is empty).
    ///
    /// In all other cases, if the version satisfies the requirement, this returns true.
    /// And if it fails to satisfy the requirement, this returns false.
    fn version_in_range(&self, requirement: &str) -> bool {
        version_compare(self.bindle.id.version(), requirement)
    }

    /// Sign the parcels on the current package.
    ///
    /// Note that this signature will be invalidated if any parcels are
    /// added after this signature.
    fn sign(&mut self, key: String) -> Result<(), SignatureError> {
        todo!()
    }

    fn verify(&self, keyring: String) -> Result<(), SignatureError> {
        todo!()
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct BindleSpec {
    #[serde(flatten)]
    pub id: Id,
    pub description: Option<String>,
    pub authors: Option<Vec<String>>,
}

/// A description of a stored parcel file
///
/// A parcel file can be an arbitrary "blob" of data. This could be binary or text files. This
/// object contains the metadata and associated conditions for using a parcel. For more information,
/// see the [Bindle Spec](https://github.com/deislabs/bindle/blob/master/docs/bindle-spec.md)
#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Parcel {
    pub label: Label,
    pub conditions: Option<Condition>,
}

impl Parcel {
    pub fn member_of(&self, group: &str) -> bool {
        match &self.conditions {
            Some(conditions) => match &conditions.member_of {
                Some(groups) => groups.iter().any(|g| *g == group),
                None => false,
            },
            None => false,
        }
    }
    /// returns true if this parcel is a member of the "global" group (default).
    ///
    /// The spec says: "An implicit global group exists. It has no name, and includes
    /// _only_ the parcels that are not assigned to any other group."
    /// Therefore, if this returns true, it is a member of the "global" group.
    pub fn is_global_group(&self) -> bool {
        match &self.conditions {
            Some(conditions) => match &conditions.member_of {
                Some(groups) => groups.is_empty(),
                None => true,
            },
            None => true,
        }
    }
}

/// Metadata of a stored parcel
///
/// See the [Label Spec](https://github.com/deislabs/bindle/blob/master/docs/label-spec.md) for more
/// detailed information
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Label {
    pub sha256: String,
    pub media_type: String,
    pub name: String,
    pub size: u64,
    pub annotations: Option<AnnotationMap>,
    pub feature: Option<FeatureMap>,
}

impl Label {
    pub fn new(name: String, sha256: String) -> Self {
        Label {
            name,
            sha256,
            ..Label::default()
        }
    }
}

impl Default for Label {
    fn default() -> Self {
        Self {
            sha256: "".to_owned(),
            media_type: "application/octet-stream".to_owned(),
            name: "".to_owned(),
            size: 0,
            annotations: None,
            feature: None,
        }
    }
}

/// Conditions associate parcels to [`Group`](crate::Group)s
#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Condition {
    pub member_of: Option<Vec<String>>,
    pub requires: Option<Vec<String>>,
}

/// A group is a top-level organization object that may contain zero or more parcels. Every parcel
/// belongs to at least one group, but may belong to others.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Group {
    pub name: String,
    pub required: Option<bool>,
    pub satisfied_by: Option<String>,
}

/// A signature describes a cryptographic signature of the parcel list.
///
/// In the current implementation, a signature signs the list of parcels that belong on
/// an invoice. The signature, in the current implementation, is an Ed25519 signature
/// and is signed by the private counterpart of the given public key.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Signature {
    // The cleartext name of the user who signed
    pub by: String,
    // The signature block, encoded as hex chars
    pub signature: String,
    // The public key, encoded as hex chars
    pub key: String,
}

#[derive(Error, Debug)]
pub enum SignatureError {
    #[error("not all signatures can be verified")]
    Unverified,
    #[error("failed to sign the invoice with the given key")]
    SigningFailed,
}

/// A custom type for responding to invoice creation requests. Because invoices can be created
/// before parcels are uploaded, this allows the API to inform the user if there are missing parcels
/// in the bindle spec
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InvoiceCreateResponse {
    pub invoice: Invoice,
    pub missing: Option<Vec<Label>>,
}

/// A response to a missing parcels request. TOML doesn't support top level arrays, so they
/// must be embedded in a table
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MissingParcelsResponse {
    pub missing: Vec<Label>,
}

/// A string error message returned from the server
#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorResponse {
    error: String,
}

/// Available options for the query API
#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct QueryOptions {
    #[serde(alias = "q")]
    pub query: Option<String>,
    #[serde(alias = "v")]
    pub version: Option<String>,
    #[serde(alias = "o")]
    pub offset: Option<u64>,
    #[serde(alias = "l")]
    pub limit: Option<u8>,
    pub strict: Option<bool>,
    pub yanked: Option<bool>,
}

impl From<QueryOptions> for SearchOptions {
    fn from(qo: QueryOptions) -> Self {
        let defaults = SearchOptions::default();
        SearchOptions {
            limit: qo.limit.unwrap_or(defaults.limit),
            offset: qo.offset.unwrap_or(defaults.offset),
            strict: qo.strict.unwrap_or(defaults.strict),
            yanked: qo.yanked.unwrap_or(defaults.yanked),
        }
    }
}

/// Check whether the given version is within the legal range.
///
/// An empty range matches anything.
///
/// A range that fails to parse matches nothing.
///
/// An empty version matches nothing (unless the requirement is empty)
///
/// A version that fails to parse matches nothing (unless the requirement is empty).
///
/// In all other cases, if the version satisfies the requirement, this returns true.
/// And if it fails to satisfy the requirement, this returns false.
fn version_compare(version: &Version, requirement: &str) -> bool {
    if requirement.is_empty() {
        return true;
    }

    // Setting Compat::Npm follows the rules here:
    // https://www.npmjs.com/package/semver
    //
    // Most importantly, the requirement "1.2.3" is treated as "= 1.2.3".
    // Without the compat mode, "1.2.3" is treated as "^1.2.3".
    match VersionReq::parse_compat(requirement, Compat::Npm) {
        Ok(req) => {
            return req.matches(version);
        }
        Err(e) => {
            log::error!("SemVer range could not parse: {}", e);
        }
    }
    false
}

#[cfg(test)]
mod test {
    use super::*;
    use std::fs::read_to_string;
    use std::path::Path;

    #[test]
    fn test_invoice_should_serialize() {
        let label = Label {
            sha256: "abcdef1234567890987654321".to_owned(),
            media_type: "text/toml".to_owned(),
            name: "foo.toml".to_owned(),
            size: 101,
            annotations: None,
            feature: None,
        };
        let parcel = Parcel {
            label,
            conditions: None,
        };
        let parcels = Some(vec![parcel]);
        let inv = Invoice {
            bindle_version: BINDLE_VERSION_1.to_owned(),
            bindle: BindleSpec {
                id: "foo/1.2.3".parse().unwrap(),
                description: Some("bar".to_owned()),
                authors: Some(vec!["m butcher".to_owned()]),
            },
            parcel: parcels,
            yanked: None,
            annotations: None,
            group: None,
            signature: None,
        };

        let res = toml::to_string(&inv).unwrap();
        let inv2 = toml::from_str::<Invoice>(res.as_str()).unwrap();

        let b = inv2.bindle;
        assert_eq!(b.id.name(), "foo".to_owned());
        assert_eq!(b.id.version_string(), "1.2.3");
        assert_eq!(b.description.unwrap().as_str(), "bar");
        assert_eq!(b.authors.unwrap()[0], "m butcher".to_owned());

        let parcels = inv2.parcel.unwrap();

        assert_eq!(parcels.len(), 1);

        let par = &parcels[0];
        let lab = &par.label;
        assert_eq!(lab.name, "foo.toml".to_owned());
        assert_eq!(lab.media_type, "text/toml".to_owned());
        assert_eq!(lab.sha256, "abcdef1234567890987654321".to_owned());
        assert_eq!(lab.size, 101);
    }

    #[test]
    fn test_examples_in_spec_parse() {
        let test_files = vec![
            "test/data/simple-invoice.toml",
            "test/data/full-invoice.toml",
            "test/data/alt-format-invoice.toml",
        ];
        test_files.iter().for_each(|file| test_parsing_a_file(file));
    }

    fn test_parsing_a_file(filename: &str) {
        let invoice_path = Path::new(filename);
        let raw = read_to_string(invoice_path).expect("read file contents");

        let invoice = toml::from_str::<Invoice>(raw.as_str()).expect("clean parse of invoice");

        // Now we serialize it and compare it to the original version
        let _raw2 = toml::to_string_pretty(&invoice).expect("clean serialization of TOML");
        // FIXME: Do we care about this detail?
        //assert_eq!(raw, raw2);
    }

    #[test]
    fn test_version_comparisons() {
        // Do not need an exhaustive list of matches -- just a sampling to make sure
        // the outer logic is correct.
        let reqs = vec!["= 1.2.3", "1.2.3", "1.2.3", "^1.1", "~1.2", ""];
        let version = Version::parse("1.2.3").unwrap();

        reqs.iter().for_each(|r| {
            if !version_compare(&version, r) {
                panic!("Should have passed: {}", r)
            }
        });

        // Again, we do not need to test the SemVer crate -- just make sure some
        // outliers and obvious cases are covered.
        let reqs = vec!["2", "%^&%^&%"];
        reqs.iter()
            .for_each(|r| assert!(!version_compare(&version, r)));
    }

    #[test]
    fn parcel_no_groups() {
        let invoice = r#"
        bindleVersion = "1.0.0"

        [bindle]
        name = "aricebo"
        version = "1.2.3"

        [[group]]
        name = "images"

        [[parcel]]
        [parcel.label]
        sha256 = "aaabbbcccdddeeefff"
        name = "telescope.gif"
        mediaType = "image/gif"
        size = 123_456
        [parcel.conditions]
        memberOf = ["telescopes"]

        [[parcel]]
        [parcel.label]
        sha256 = "111aaabbbcccdddeee"
        name = "telescope.txt"
        mediaType = "text/plain"
        size = 123_456
        "#;

        let invoice: crate::Invoice = toml::from_str(invoice).expect("a nice clean parse");
        let parcels = invoice.parcel.expect("expected some parcels");

        let img = &parcels[0];
        let txt = &parcels[1];

        assert!(img.member_of("telescopes"));
        assert!(!img.is_global_group());

        assert!(txt.is_global_group());
        assert!(!txt.member_of("telescopes"));
    }

    #[test]
    fn signing_and_verifying() {
        let invoice = r#"
        bindleVersion = "1.0.0"

        [bindle]
        name = "aricebo"
        version = "1.2.3"

        [[parcel]]
        [parcel.label]
        sha256 = "aaabbbcccdddeeefff"
        name = "telescope.gif"
        mediaType = "image/gif"
        size = 123_456
        
        [[parcel]]
        [parcel.label]
        sha256 = "111aaabbbcccdddeee"
        name = "telescope.txt"
        mediaType = "text/plain"
        size = 123_456
        "#;

        let invoice: crate::Invoice = toml::from_str(invoice).expect("a nice clean parse");
        let parcels = invoice.parcel.expect("expected some parcels");

        assert!(invoice.signature.is_none());
    }
}
