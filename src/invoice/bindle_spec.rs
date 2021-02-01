//! The specification for a bindle

use serde::{Deserialize, Serialize};

use crate::id::Id;

/// The specification for a bindle, that uniquely identifies the Bindle and provides additional
/// optional metadata
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct BindleSpec {
    #[serde(flatten)]
    pub id: Id,
    pub description: Option<String>,
    pub authors: Option<Vec<String>>,
}
