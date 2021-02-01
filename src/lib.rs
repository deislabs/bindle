//! A crate for interacting with Bindles
//!
//! Bindle is an aggregate object storage system used for storing aggregate applications. For more
//! information and examples, see the
//! [README](https://github.com/deislabs/bindle/blob/master/README.md) in the Bindle repo.
//!
//! This crate is the reference implementation of the [Bindle
//! Spec](https://github.com/deislabs/bindle/blob/master/docs/bindle-spec.md) and it contains both a
//! client and a server implementation, along with various other utilities

mod id;
mod invoice;

pub mod async_util;
#[cfg(feature = "caching")]
pub mod cache;
#[cfg(feature = "client")]
pub mod client;
pub mod provider;
#[cfg(feature = "client")]
pub mod proxy;
pub mod search;
#[cfg(feature = "server")]
pub mod server;
#[cfg(feature = "client")]
pub mod standalone;
#[cfg(feature = "test-tools")]
pub mod testing;

pub mod filters;

#[doc(inline)]
pub use id::Id;
#[doc(inline)]
pub use invoice::*;
#[doc(inline)]
pub use search::Matches;

/// The version string for the v1 Bindle Spec
pub const BINDLE_VERSION_1: &str = "1.0.0";
