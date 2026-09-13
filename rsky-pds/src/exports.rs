//! Bounded, streamed reads of large objects: repository exports and blobs
//! go to the client as they are produced, and only so many run at once.

use crate::apis::ApiError;
use futures::stream::{Stream, StreamExt};
use rocket::http::{ContentType, Header};
use rocket::response::stream::ReaderStream;
use rocket::response::{self, Responder, Response};
use rocket::Request;
use rsky_common::env::env_int;
use std::io::Cursor;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncSeek, ReadBuf};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// How long a request waits for a slot before it is refused.
const WAIT_FOR_SLOT: Duration = Duration::from_secs(30);

pub struct Exports {
    repo: Arc<Semaphore>,
    blob: Arc<Semaphore>,
    wait: Duration,
}

impl Exports {
    pub fn new(repo_slots: usize, blob_slots: usize) -> Self {
        Exports {
            repo: Arc::new(Semaphore::new(repo_slots.max(1))),
            blob: Arc::new(Semaphore::new(blob_slots.max(1))),
            wait: WAIT_FOR_SLOT,
        }
    }

    pub fn from_env() -> Self {
        Exports::new(
            env_int("PDS_MAX_CONCURRENT_EXPORTS").unwrap_or(4),
            env_int("PDS_MAX_CONCURRENT_BLOB_READS").unwrap_or(32),
        )
    }

    pub fn with_wait(mut self, wait: Duration) -> Self {
        self.wait = wait;
        self
    }

    pub fn repo_slots_free(&self) -> usize {
        self.repo.available_permits()
    }

    pub fn blob_slots_free(&self) -> usize {
        self.blob.available_permits()
    }

    pub async fn repo_slot(&self) -> Result<ExportGuard, ApiError> {
        Self::slot(&self.repo, self.wait, "repository exports").await
    }

    pub async fn blob_slot(&self) -> Result<ExportGuard, ApiError> {
        Self::slot(&self.blob, self.wait, "blob reads").await
    }

    async fn slot(
        semaphore: &Arc<Semaphore>,
        wait: Duration,
        what: &str,
    ) -> Result<ExportGuard, ApiError> {
        match tokio::time::timeout(wait, semaphore.clone().acquire_owned()).await {
            Ok(Ok(permit)) => Ok(ExportGuard {
                _permit: permit,
                started: Instant::now(),
            }),
            _ => Err(ApiError::Overloaded(format!("too many concurrent {what}"))),
        }
    }
}

/// Holds a slot for as long as the response body is being sent.
pub struct ExportGuard {
    _permit: OwnedSemaphorePermit,
    started: Instant,
}

impl Drop for ExportGuard {
    fn drop(&mut self) {
        crate::metrics::METRICS
            .export_duration
            .observe(self.started.elapsed().as_secs_f64());
    }
}

/// A stream that keeps a value alive until it is dropped.
pub struct Guarded<S, G> {
    inner: Pin<Box<S>>,
    _guard: G,
}

impl<S, G> Guarded<S, G> {
    pub fn new(inner: S, guard: G) -> Self {
        Guarded {
            inner: Box::pin(inner),
            _guard: guard,
        }
    }
}

impl<S: Stream, G: Unpin> Stream for Guarded<S, G> {
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.get_mut().inner.as_mut().poll_next(cx)
    }
}

/// A repository export sent as it is produced. A failure after the first
/// bytes have gone out ends the body early, which the client sees as a
/// CAR that does not verify.
pub struct CarStream {
    stream: Pin<Box<dyn Stream<Item = Vec<u8>> + Send>>,
}

impl CarStream {
    pub fn new<S>(stream: S, guard: ExportGuard) -> Self
    where
        S: Stream<Item = anyhow::Result<Vec<u8>>> + Send + 'static,
    {
        let bytes = stream
            .map(|chunk| {
                chunk
                    .inspect_err(|err| tracing::error!(?err, "repository export failed"))
                    .ok()
            })
            .take_while(|chunk| futures::future::ready(chunk.is_some()))
            .filter_map(futures::future::ready);
        CarStream {
            stream: Box::pin(Guarded::new(bytes, guard)),
        }
    }
}

impl<'r> Responder<'r, 'r> for CarStream {
    fn respond_to(self, _req: &'r Request<'_>) -> response::Result<'r> {
        Response::build()
            .header(ContentType::new("application", "vnd.ipld.car"))
            .streamed_body(ReaderStream::from(self.stream.map(Cursor::new)))
            .ok()
    }
}

/// A reader whose length is known up front, so the body can carry a
/// content length without being seekable.
pub struct Unseekable<R>(pub R);

impl<R: AsyncRead + Unpin> AsyncRead for Unseekable<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl<R: Unpin> AsyncSeek for Unseekable<R> {
    fn start_seek(self: Pin<&mut Self>, _position: std::io::SeekFrom) -> std::io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "body is not seekable",
        ))
    }

    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<u64>> {
        Poll::Ready(Ok(0))
    }
}

/// A blob sent as it is read from object storage, with the length and
/// type its registration recorded.
pub struct BlobBody {
    reader: Guarded<Unseekable<Pin<Box<dyn AsyncRead + Send>>>, ExportGuard>,
    size: usize,
    mime_type: String,
}

impl BlobBody {
    pub fn new(
        reader: impl AsyncRead + Send + 'static,
        size: usize,
        mime_type: Option<String>,
        guard: ExportGuard,
    ) -> Self {
        let reader: Pin<Box<dyn AsyncRead + Send>> = Box::pin(reader);
        BlobBody {
            reader: Guarded::new(Unseekable(reader), guard),
            size,
            mime_type: mime_type.unwrap_or_else(|| "application/octet-stream".to_string()),
        }
    }
}

impl<S: AsyncRead, G: Unpin> AsyncRead for Guarded<S, G> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        self.get_mut().inner.as_mut().poll_read(cx, buf)
    }
}

impl<S: AsyncSeek, G: Unpin> AsyncSeek for Guarded<S, G> {
    fn start_seek(self: Pin<&mut Self>, position: std::io::SeekFrom) -> std::io::Result<()> {
        self.get_mut().inner.as_mut().start_seek(position)
    }

    fn poll_complete(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<u64>> {
        self.get_mut().inner.as_mut().poll_complete(cx)
    }
}

impl<'r> Responder<'r, 'r> for BlobBody {
    fn respond_to(self, _req: &'r Request<'_>) -> response::Result<'r> {
        Response::build()
            .header(Header::new("content-type", self.mime_type))
            .header(Header::new(
                "content-security-policy",
                "default-src 'none'; sandbox",
            ))
            .sized_body(self.size, self.reader)
            .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn slots_are_bounded_and_released_with_the_body() {
        let exports = Exports::new(1, 1).with_wait(Duration::from_millis(50));
        assert_eq!(exports.repo_slots_free(), 1);
        let guard = exports.repo_slot().await.unwrap();
        assert_eq!(exports.repo_slots_free(), 0);
        let refused = exports.repo_slot().await.map(drop).unwrap_err();
        assert!(matches!(refused, ApiError::Overloaded(message) if message.contains("exports")));
        let stream = CarStream::new(
            futures::stream::iter(vec![
                Ok(vec![1u8]),
                Err(anyhow::anyhow!("boom")),
                Ok(vec![2u8]),
            ]),
            guard,
        );
        let chunks: Vec<Vec<u8>> = stream.stream.collect().await;
        assert_eq!(chunks, vec![vec![1u8]]);
        assert_eq!(exports.repo_slots_free(), 1);

        let blob_guard = exports.blob_slot().await.unwrap();
        assert_eq!(exports.blob_slots_free(), 0);
        let refused = exports.blob_slot().await.map(drop).unwrap_err();
        assert!(matches!(refused, ApiError::Overloaded(_)));
        let mut body = BlobBody::new(Cursor::new(b"abc".to_vec()), 3, None, blob_guard);
        assert_eq!(body.mime_type, "application/octet-stream");
        let mut out = Vec::new();
        body.reader.read_to_end(&mut out).await.unwrap();
        assert_eq!(out, b"abc");
        assert!(Pin::new(&mut body.reader)
            .start_seek(std::io::SeekFrom::Start(0))
            .is_err());
        let mut cx = Context::from_waker(futures::task::noop_waker_ref());
        assert!(matches!(
            Pin::new(&mut body.reader).poll_complete(&mut cx),
            Poll::Ready(Ok(0))
        ));
        drop(body);
        assert_eq!(exports.blob_slots_free(), 1);
        assert!(Exports::from_env().repo_slots_free() >= 1);
    }
}
