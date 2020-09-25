#![macro_use]
extern crate serde;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod storage;

#[derive(Serialize, Deserialize, Debug)]
struct Invoice {
    bindle: BindleSpec,
    boxes: BTreeMap<String, Label>,
}

#[derive(Serialize, Deserialize, Debug)]
struct BindleSpec {
    name: String,
    description: Option<String>,
    version: String,
    sha256: String,
    authors: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Debug)]
struct Label {
    sha256: String,
    media_type: String,
    name: String,
    size: Option<i64>,
}

// TODO: Version should be a SemVer

#[cfg(test)]
mod test {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn test_invoice() {
        let label = Label {
            sha256: "abcdef1234567890987654321".to_owned(),
            media_type: "text/toml".to_owned(),
            name: "foo.toml".to_owned(),
            size: Some(101),
        };
        let mut boxen = BTreeMap::new();
        boxen.insert(label.sha256.to_string(), label);

        let inv = Invoice {
            bindle: BindleSpec {
                name: "foo".to_owned(),
                description: Some("bar".to_owned()),
                version: "v1.2.3".to_owned(),
                authors: Some(vec!["m butcher".to_owned()]),
                sha256: "abcdef1234567890987654321".to_owned(),
            },
            boxes: boxen,
        };

        let res = toml::to_string(&inv).unwrap();
        let inv2 = toml::from_str::<Invoice>(res.as_str()).unwrap();

        let b = inv2.bindle;
        assert_eq!(b.name, "foo".to_owned());
        assert_eq!(b.version.as_str(), "v1.2.3");
        assert_eq!(b.description.unwrap().as_str(), "bar");
        assert_eq!(b.authors.unwrap()[0], "m butcher".to_owned());
        assert_eq!(b.sha256.as_str(), "abcdef1234567890987654321");
        assert_eq!(inv2.boxes.len(), 1);

        let bx = inv2
            .boxes
            .get(&"abcdef1234567890987654321".to_owned())
            .unwrap();
        assert_eq!(bx.name, "foo.toml".to_owned());
        assert_eq!(bx.media_type, "text/toml".to_owned());
        assert_eq!(bx.sha256, "abcdef1234567890987654321".to_owned());
        assert_eq!(bx.size.unwrap(), 101)
    }
}
