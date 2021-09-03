//! An authorizer that authorizes anonymous access for GET requests and denies all other
//! unauthenticated requests

use warp::http::Method;

#[derive(Clone)]
pub struct AnonymousGet;

impl super::Authorizer for AnonymousGet {
    fn authorize<A: super::Authorizable>(
        &self,
        item: A,
        _path: &str,
        method: &Method,
    ) -> anyhow::Result<()> {
        // Any get request should succeed, no matter the path
        if matches!(method, &Method::GET) {
            return Ok(());
        }

        // An empty principal would mean this is anonymous
        if item.principal().is_empty() {
            anyhow::bail!("Anonymous authorization is not allowed for non-GET endpoints")
        } else {
            Ok(())
        }
    }
}
