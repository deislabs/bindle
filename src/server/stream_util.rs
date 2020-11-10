use std::io::Read;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::buf::{Buf, BufExt};
use tokio::io::AsyncRead;
use tokio::stream::Stream;

/// A wrapper around a Warp stream of bytes that implements AsyncRead. This might no longer be
/// necessary once we hit tokio 0.3 and upgrade tokio-util. Tokio util has a StreamReader wrapper we
/// can use, but there might still be some conversion stuff to deal with
pub struct BodyReadBuffer<B, T>(pub T)
where
    B: Buf,
    T: Stream<Item = Result<B, warp::Error>> + Unpin;

impl<B, T> AsyncRead for BodyReadBuffer<B, T>
where
    B: Buf,
    T: Stream<Item = Result<B, warp::Error>> + Unpin,
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
            Err(e) => return Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, e))),
        };

        Poll::Ready(buffer.reader().read(buf))
    }
}
