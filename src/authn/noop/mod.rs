use super::Authenticator;
use crate::authz::noop::Anonymous;

/// An authenticator that simply returns an anonymous user
#[derive(Clone, Debug)]
pub struct NoopAuthenticator;

#[async_trait::async_trait]
impl Authenticator for NoopAuthenticator {
    type Item = Anonymous;

    async fn authenticate(&self, _auth_data: &str) -> anyhow::Result<Self::Item> {
        Ok(Anonymous)
    }
}
