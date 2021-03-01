//! Types and traits for use in authorization. This module is only available if the `server` feature
//! is enabled

pub mod noop;

use std::fmt::{self, Display};

/// The verb being used for the request
#[derive(Debug, Clone)]
pub enum Verb {
    /// Read access to a given object
    Get,
    /// Create access for a given object
    Create,
    /// Yank access for a given object. This only applies to invoices as parcels cannot be yanked.
    Yank,
}

impl Display for Verb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Verb::Get => write!(f, "get"),
            Verb::Create => write!(f, "create"),
            Verb::Yank => write!(f, "yank"),
        }
    }
}

/// Specific restrictions on path access granted
///
/// Due to the arbitrarily pathy nature of Bindle IDs, when mapping roles for an invoice, an
/// additional type restriction must be specified.
#[derive(Debug, Clone)]
pub enum TypeRestriction {
    /// Indicates that this path is a full bindle path and isn’t a subpath.
    ///
    /// A create role on `example.com/foo` with a `bindle` type means that only an `example.com/foo`
    /// bindle can be created. It will prevent a user from creating an `example.com/foo/bar` bindle.
    /// This is the default rule if no type is specified
    Bindle,
    /// Indicates that this path is a subpath, granting the user the right to do anything with the
    /// allowed verb under that subpath.
    ///
    /// So a create role on `example.com/foo` with a subpath type means that user can arbitrarily
    /// create any bindle starting with the `example.com/foo` subpath, but CANNOT create a bindle
    /// with that exact path.
    Subpath,
}

impl Display for TypeRestriction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeRestriction::Bindle => write!(f, "bindle"),
            TypeRestriction::Subpath => write!(f, "subpath"),
        }
    }
}

impl Default for TypeRestriction {
    fn default() -> Self {
        TypeRestriction::Bindle
    }
}

/// The type of object being acted on. Each variant has its unique ID embedded.
#[derive(Debug, Clone)]
pub enum Object {
    Invoice(crate::Id),
    Parcel(String),
}

#[derive(Debug)]
/// A custom error that describes why authorization failed on any given object
pub struct AuthorizationFailure {
    principal: String,
    verb: Verb,
    restriction: Option<TypeRestriction>,
    object: Object,
}

impl std::error::Error for AuthorizationFailure {}

impl AuthorizationFailure {
    /// Creates a new AuthorizationFailure using the given parameters. Generally speaking, this
    /// should only be used by `Authorizer` implementations
    pub fn new(
        principal: &str,
        verb: Verb,
        restriction: Option<TypeRestriction>,
        object: Object,
    ) -> Self {
        AuthorizationFailure {
            principal: principal.into(),
            verb,
            restriction,
            object,
        }
    }
}

impl Display for AuthorizationFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // This results in a message that looks like this: "Principal foo does not have
        // create/subpath access for invoice with identity of example.com/foo/bar/1.0.0". This is
        // mainly to avoid allocating new strings and then writing those
        write!(f, "Principal {} does not have ", self.principal)?;
        match self.restriction.as_ref() {
            Some(r) => write!(f, "{}/{}", self.verb, r)?,
            None => write!(f, "{}", self.verb)?,
        }
        write!(f, " access for ")?;

        match &self.object {
            Object::Invoice(id) => write!(f, "invoice with identity of {}", id),
            Object::Parcel(sha) => write!(f, "parcel with identity of {}", sha),
        }
    }
}

/// A custom result type for use by [`Authorizer`](Authorizer) implementation
pub type Result<T> = std::result::Result<T, AuthorizationFailure>;

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
pub trait Authorizer {
    /// Authorizes the given item, returning a failure reason in the case where the item is not
    /// authorized
    fn authorize<A: Authorizable>(item: A, verb: Verb, object: Object) -> Result<()>;
}
