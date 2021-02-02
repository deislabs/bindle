//! Definition and implementation of the `Label` type
//!
//! See the [Label Spec](https://github.com/deislabs/bindle/blob/master/docs/label-spec.md) for more
//! detailed information

use serde::{Deserialize, Serialize};

use crate::invoice::{AnnotationMap, FeatureMap};

/// Metadata of a stored parcel
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
