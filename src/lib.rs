#![macro_use]
extern crate serde;

use serde::{Deserialize, Serialize};

mod storage;

pub const BINDLE_VERSION_1: &str = "v1.0.0";

#[derive(Serialize, Deserialize, Debug)]
pub struct Invoice {
    bindle_version: String,
    yanked: Option<bool>,
    bindle: BindleSpec,
    parcels: Option<Vec<Parcel>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BindleSpec {
    name: String,
    description: Option<String>,
    version: String,
    authors: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Parcel {
    label: Label,
    conditions: Option<Condition>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Label {
    sha256: String,
    media_type: String,
    name: String,
    size: Option<i64>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Condition {
    member_of: Option<Vec<String>>,
    requires: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Group {
    name: String,
    required: bool,
    satisfied_by: Option<String>,
}

// TODO: Version should be a SemVer

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_invoice() {
        let label = Label {
            sha256: "abcdef1234567890987654321".to_owned(),
            media_type: "text/toml".to_owned(),
            name: "foo.toml".to_owned(),
            size: Some(101),
        };
        let parcel = Parcel {
            label,
            conditions: None,
        };
        let parcels = Some(vec![parcel]);
        let inv = Invoice {
            bindle_version: BINDLE_VERSION_1.to_owned(),
            yanked: None,
            bindle: BindleSpec {
                name: "foo".to_owned(),
                description: Some("bar".to_owned()),
                version: "v1.2.3".to_owned(),
                authors: Some(vec!["m butcher".to_owned()]),
            },
            parcels,
        };

        let res = toml::to_string(&inv).unwrap();
        let inv2 = toml::from_str::<Invoice>(res.as_str()).unwrap();

        let b = inv2.bindle;
        assert_eq!(b.name, "foo".to_owned());
        assert_eq!(b.version.as_str(), "v1.2.3");
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
}
