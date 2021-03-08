//! Types and traits for use in authorization. This module is only available if the `server` feature
//! is enabled

pub mod always;

/// A trait that can be implemented on any type (such as a custom `User` or `Token` type) so that it
/// can be authorized by an [`Authorizer`](Authorizer)
pub trait Authorizable {
    /// Returns the identity or username of the authenticated user
    fn principal(&self) -> String;

    /// Returns the groups the authenticated user is a member of, generally embedded on something
    /// like a JWT or fetched from an upstream server
    fn groups(&self) -> Vec<String>;
}

/// A trait for any system that can authorize any [`Authorizable`](Authorizable) type
// TODO: Will this need to be async?
pub trait Authorizer {
    /// Checks whether or not the given item is authorized to access provided path and method,
    /// returning a failure reason in the case where the item is not authorized
    // TODO: We might want to have a custom error enum down the line
    fn authorize<A: Authorizable>(
        &self,
        item: A,
        path: &str,
        method: warp::http::Method,
    ) -> anyhow::Result<()>;
}
