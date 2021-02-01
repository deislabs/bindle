//! Definition of the `Group` type

use serde::{Deserialize, Serialize};

/// A group is a top-level organization object that may contain zero or more parcels. Every parcel
/// belongs to at least one group, but may belong to others.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Group {
    pub name: String,
    pub required: Option<bool>,
    pub satisfied_by: Option<String>,
}
