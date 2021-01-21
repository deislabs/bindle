//! A collection of various utilities for asyncifying things, publicly exposed for convenience of
//! those consuming Bindle as a Rust SDK

use std::io::Write;
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll};

use sha2::{Digest, Sha256};

/// A wrapper to implement `AsyncWrite` on Sha256
pub struct AsyncSha256 {
    inner: Mutex<Sha256>,
}

impl Default for AsyncSha256 {
    fn default() -> Self {
        AsyncSha256::new()
    }
}

impl AsyncSha256 {
    /// Equivalent to the `Sha256::new()` function
    pub fn new() -> Self {
        AsyncSha256 {
            inner: Mutex::new(Sha256::new()),
        }
    }

    /// Consumes self and returns the bare Sha256. This should only be called once you are done
    /// writing. This will only return an error if for some reason the underlying mutex was poisoned
    pub fn into_inner(self) -> std::sync::LockResult<Sha256> {
        self.inner.into_inner()
    }
}

impl tokio::io::AsyncWrite for AsyncSha256 {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::result::Result<usize, std::io::Error>> {
        // Because the hasher is all in memory, we only need to make sure only one caller at a time
        // can write using the mutex
        let mut inner = match self.inner.try_lock() {
            Ok(l) => l,
            Err(_) => return Poll::Pending,
        };

        Poll::Ready(inner.write(buf))
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<std::result::Result<(), std::io::Error>> {
        let mut inner = match self.inner.try_lock() {
            Ok(l) => l,
            Err(_) => return Poll::Pending,
        };

        Poll::Ready(inner.flush())
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::result::Result<(), std::io::Error>> {
        // There are no actual shutdown tasks to perform, so just flush things as defined in the
        // trait documentation
        self.poll_flush(cx)
    }
}
