use super::Authenticator;
use crate::authz::noop::Anonymous;

/// An authenticator that simply returns an anonymous user
pub struct NoopAuthenticator;

impl Authenticator for NoopAuthenticator {
    type Item = Anonymous;

    fn authenticate(&self, _auth_data: &str) -> anyhow::Result<Self::Item> {
        Ok(Anonymous)
    }
}
