//! A simple noop authorizer that does nothing for use when authorization is not desired or for
//! development environments
use super::{Authorizable, Authorizer, Result};

/// An anonymous user
pub struct Anonymous;

impl Authorizable for Anonymous {
    fn principal(&self) -> String {
        String::from("NOOP")
    }

    fn groups(&self) -> Vec<String> {
        Vec::with_capacity(0)
    }
}

/// An authorizer that always returns success
pub struct NoopAuthorizer;

impl Authorizer for NoopAuthorizer {
    fn authorize<A: Authorizable>(_: A, _: super::Verb, _: super::Object) -> Result<()> {
        Ok(())
    }
}
