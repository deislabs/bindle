//! Types and traits for use in authentication. This module is only available if the `server`
//! feature is enabled

pub mod noop;

use crate::authz::Authorizable;

/// A trait that can be implemented by any system able to authenticate a request
pub trait Authenticator {
    /// The authorizable item type that is returned from the `authenticate` method
    type Item: Authorizable;

    /// Authenticate the request given the arbitrary `auth_data`, returning an arbitrary error in
    /// case of a failure. This data could be a JWT or a base64 encoded username + password
    /// combination depending on the implementation
    fn authenticate(&self, auth_data: &str) -> anyhow::Result<Self::Item>;
}
