//! Caching implementations for client and server-side usage. This module is under heavy development
//! and iteration

use crate::storage::{Storage, StorageError};

pub mod dumb;
pub use dumb::DumbCache;

// Once implemented, we can export this
mod lru;
// pub use lru::LruCache;

/// A marker trait that indicates this is a caching implementation (as opposed to just storage)
pub trait Cache: Storage {}

/// A custom result type representing a possible cache miss. As all underlying caches implement
/// `Storage`, this contains a storage error that is guaranteed not to be a cache miss (e.g.
/// NotFound). The Option indicates whether a value was returned. This value is obtained by
/// coverting a normal storage result using `into_cache_result`
pub(crate) type CacheResult<T> = Result<Option<T>, crate::storage::StorageError>;

/// Converts a storage result into a `CacheResult`
pub(crate) fn into_cache_result<T>(res: crate::storage::Result<T>) -> CacheResult<T> {
    match res {
        Ok(val) => Ok(Some(val)),
        Err(e) if matches!(e, StorageError::NotFound) => Ok(None),
        Err(e) => Err(e),
    }
}
