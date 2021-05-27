//! Definition of the `Condition` type

use serde::{Deserialize, Serialize};

/// Conditions associate parcels to [`Group`](crate::Group)s
#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Condition {
    pub member_of: Option<Vec<String>>,
    pub requires: Option<Vec<String>>,
}

impl Condition {
    pub fn in_default_group(&self) -> bool {
        return self.member_of.is_none();
    }
}
