//! A simple noop authorizer that does nothing for use when authorization is not desired or for
//! development environments
use super::{Authorizable, Authorizer};

/// An anonymous user
#[derive(Debug, Clone)]
pub struct Anonymous;

impl Authorizable for Anonymous {
    fn principal(&self) -> &str {
        ""
    }
}

/// An authorizer that always returns success
#[derive(Debug, Clone)]
pub struct AlwaysAuthorize;

impl Authorizer for AlwaysAuthorize {
    fn authorize<A: Authorizable>(
        &self,
        _: A,
        _: &str,
        _: &warp::http::Method,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}
