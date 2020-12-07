//! A collection of various utilities for asyncifying things, publicly exposed for convenience of
//! those consuming Bindle as a Rust SDK

use std::io::{Read, Write};
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll};

use bytes::buf::{Buf, BufExt};
use sha2::{Digest, Sha256};
use tokio::io::AsyncRead;
use tokio::stream::Stream;

/// A wrapper around a Warp stream of bytes that implements AsyncRead. This might no longer be
/// necessary once we hit tokio 0.3 and upgrade tokio-util. Tokio util has a StreamReader wrapper we
/// can use, but there might still be some conversion stuff to deal with
pub struct BodyReadBuffer<B, T, E>(pub T)
where
    B: Buf,
    T: Stream<Item = Result<B, E>> + Unpin,
    E: std::error::Error;

impl<'a, B, T, E> AsyncRead for BodyReadBuffer<B, T, E>
where
    B: Buf,
    T: Stream<Item = Result<B, E>> + Unpin,
    E: std::error::Error + Send + Sync + 'a,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        let res = match Pin::new(&mut self.0).poll_next(cx) {
            Poll::Pending => return Poll::Pending,
            // End of stream maps to EOF in this situation
            Poll::Ready(None) => return Poll::Ready(Ok(0)),
            // If we get here, we can unwrap safely
            Poll::Ready(Some(res)) => res,
        };

        let buffer = match res {
            Ok(b) => b,
            // There isn't much of a way to introspect a warp error easily so we can't really
            // provide much context here with the right kind
            Err(e) => {
                return Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("{:?}", e), // dirty hack to get around lifetimes
                )));
            }
        };

        Poll::Ready(buffer.reader().read(buf))
    }
}

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
