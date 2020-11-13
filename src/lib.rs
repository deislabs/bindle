#![macro_use]
extern crate serde;

use semver::{Compat, Version, VersionReq};
use serde::{Deserialize, Serialize};

use std::collections::BTreeMap;

mod server;

pub mod search;
pub mod storage;
pub use server::server;

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
    fn name(&self) -> String {
        format!("{}/{}", self.bindle.name, self.bindle.version)
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
        version_compare(self.bindle.version.as_str(), requirement)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct BindleSpec {
    pub name: String,
    pub description: Option<String>,
    pub version: String,
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
    pub size: Option<i64>,
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
pub fn version_compare(version: &str, requirement: &str) -> bool {
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
            return match Version::parse(version) {
                Ok(ver) => req.matches(&ver),
                Err(e) => {
                    eprintln!("Match failed with an error: {}", e);
                    false
                }
            }
        }
        Err(e) => {
            eprintln!("SemVer range could not parse: {}", e);
        }
    }
    false
}

// TODO: Version should be a SemVer

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
            size: Some(101),
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
                name: "foo".to_owned(),
                description: Some("bar".to_owned()),
                version: "1.2.3".to_owned(),
                authors: Some(vec!["m butcher".to_owned()]),
            },
            parcels,
            group: None,
        };

        let res = toml::to_string(&inv).unwrap();
        let inv2 = toml::from_str::<Invoice>(res.as_str()).unwrap();

        let b = inv2.bindle;
        assert_eq!(b.name, "foo".to_owned());
        assert_eq!(b.version.as_str(), "1.2.3");
        assert_eq!(b.description.unwrap().as_str(), "bar");
        assert_eq!(b.authors.unwrap()[0], "m butcher".to_owned());

        let parcels = inv2.parcels.unwrap();

        assert_eq!(parcels.len(), 1);

        let par = &parcels[0];
        let lab = &par.label;
        assert_eq!(lab.name, "foo.toml".to_owned());
        assert_eq!(lab.media_type, "text/toml".to_owned());
        assert_eq!(lab.sha256, "abcdef1234567890987654321".to_owned());
        assert_eq!(lab.size.unwrap(), 101)
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
    pub fn version_compare(version: &str, requirement: &str) -> bool {
        println!(
            "Got version compare. Version: {}, Requirement: {}",
            version, requirement
        );
        if requirement.is_empty() {
            return true;
        }

        if let Ok(req) = VersionReq::parse(requirement) {
            return match Version::parse(version) {
                Ok(ver) => req.matches(&ver),
                Err(e) => {
                    eprintln!("Match failed with an error: {}", e);
                    false
                }
            };
        }

        false
    }

    #[test]
    fn test_version_comparisons() {
        // Do not need an exhaustive list of matches -- just a sampling to make sure
        // the outer logic is correct.
        let reqs = vec!["= 1.2.3", "1.2.3", "1.2.3", "^1.1", "~1.2", ""];

        reqs.iter().for_each(|r| {
            if !version_compare("1.2.3", r) {
                panic!("Should have passed: {}", r)
            }
        });

        // Again, we do not need to test the SemVer crate -- just make sure some
        // outliers and obvious cases are covered.
        let reqs = vec!["2", "%^&%^&%"];
        reqs.iter()
            .for_each(|r| assert!(!version_compare("1.2.3", r)));

        // Finally, test the outliers having to do with version strings
        let vers = vec!["", "%^&%^&%"];
        vers.iter().for_each(|v| assert!(!version_compare(v, "^1")));
    }
}
