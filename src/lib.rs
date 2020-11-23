#![macro_use]
extern crate serde;

#[cfg(feature = "client")]
pub mod client;
pub mod id;
pub mod search;
#[cfg(feature = "server")]
pub mod server;
pub mod storage;

pub use id::Id;
pub use search::Matches;

use semver::{Compat, Version, VersionReq};
use serde::{Deserialize, Serialize};

use std::collections::BTreeMap;

use search::SearchOptions;

pub const BINDLE_VERSION_1: &str = "v1.0.0";

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Invoice {
    pub bindle_version: String,
    pub yanked: Option<bool>,
    pub bindle: BindleSpec,
    pub annotations: Option<BTreeMap<String, String>>,
    #[serde(alias = "parcel")]
    pub parcels: Option<Vec<Parcel>>,
    // TODO: Should this be renamed "groups" or should "parcels" be renamed to "parcel"
    pub group: Option<Vec<Group>>,
}

impl Invoice {
    /// produce a slash-delimited "invoice name"
    ///
    /// For example, an invoice with the bindle name "hello" and the bindle version
    /// "v1.2.3" will produce "hello/v1.2.3"
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
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct BindleSpec {
    #[serde(flatten)]
    pub id: Id,
    pub description: Option<String>,
    pub authors: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Parcel {
    pub label: Label,
    pub conditions: Option<Condition>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Label {
    pub sha256: String,
    pub media_type: String,
    pub name: String,
    pub size: u64,
    pub annotations: Option<BTreeMap<String, String>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Condition {
    pub member_of: Option<Vec<String>>,
    pub requires: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Group {
    pub name: String,
    pub required: Option<bool>,
    pub satisfied_by: Option<String>,
}

/// A custom wrapper for responding to invoice creation responses. Because invoices can be created
/// before parcels are uploaded, we need to inform the user if there are missing parcels in the
/// bindle spec
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InvoiceCreateResponse {
    pub invoice: Invoice,
    pub missing: Option<Vec<Label>>,
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

// This isn't a `From` implementation because it isn't symmetrical. Converting from a
// `SearchOptions` to a `QueryOptions` would always end up with some fields set to none.
impl Into<SearchOptions> for QueryOptions {
    fn into(self) -> SearchOptions {
        let defaults = SearchOptions::default();
        SearchOptions {
            limit: self.limit.unwrap_or(defaults.limit),
            offset: self.offset.unwrap_or(defaults.offset),
            strict: self.strict.unwrap_or(defaults.strict),
            yanked: self.yanked.unwrap_or(defaults.yanked),
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
        };
        let parcel = Parcel {
            label,
            conditions: None,
        };
        let parcels = Some(vec![parcel]);
        let inv = Invoice {
            bindle_version: BINDLE_VERSION_1.to_owned(),
            yanked: None,
            annotations: None,
            bindle: BindleSpec {
                id: "foo/1.2.3".parse().unwrap(),
                description: Some("bar".to_owned()),
                authors: Some(vec!["m butcher".to_owned()]),
            },
            parcels,
            group: None,
        };

        let res = toml::to_string(&inv).unwrap();
        let inv2 = toml::from_str::<Invoice>(res.as_str()).unwrap();

        let b = inv2.bindle;
        assert_eq!(b.id.name(), "foo".to_owned());
        assert_eq!(b.id.version_string(), "1.2.3");
        assert_eq!(b.description.unwrap().as_str(), "bar");
        assert_eq!(b.authors.unwrap()[0], "m butcher".to_owned());

        let parcels = inv2.parcels.unwrap();

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
        let raw2 = toml::to_string_pretty(&invoice).expect("clean serialization of TOML");
        println!("===========\n{}\n===========", raw2);
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
}
