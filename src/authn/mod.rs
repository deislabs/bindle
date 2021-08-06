//! Types and traits for use in authentication. This module is only available if the `server`
//! feature is enabled

pub mod always;
pub mod github;

use crate::authz::Authorizable;

/// A trait that can be implemented by any system able to authenticate a request
#[async_trait::async_trait]
pub trait Authenticator {
    /// The authorizable item type that is returned from the `authenticate` method
    type Item: Authorizable + Send + 'static;

    /// Authenticate the request given the arbitrary `auth_data`, returning an arbitrary error in
    /// case of a failure. This data will likely be the value of the Authorization header. Anonymous
    /// auth will be indicated by an empty auth_data string
    async fn authenticate(&self, auth_data: &str) -> anyhow::Result<Self::Item>;

    /// The client_id to use for this authentication. Defaults to an empty string if not implemented
    fn client_id(&self) -> &str {
        ""
    }
}
