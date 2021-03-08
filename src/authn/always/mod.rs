use super::Authenticator;
use crate::authz::always::Anonymous;

/// An authenticator that simply returns an anonymous user
#[derive(Clone, Debug)]
pub struct AlwaysAuthenticate;

#[async_trait::async_trait]
impl Authenticator for AlwaysAuthenticate {
    type Item = Anonymous;

    async fn authenticate(&self, _auth_data: &str) -> anyhow::Result<Self::Item> {
        Ok(Anonymous)
    }
}
