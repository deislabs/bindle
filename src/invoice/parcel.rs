//! Definition and implementation of the `Parcel` type

use serde::{Deserialize, Serialize};

use crate::invoice::{Condition, Label};

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
