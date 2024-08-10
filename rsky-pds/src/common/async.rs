use futures::channel::oneshot;
use futures::stream::Stream;
use futures::task::{Context, Poll};
use std::collections::VecDeque;
use std::error::Error;
use std::future::Future;
use std::pin::Pin;
use thiserror::Error;

#[derive(Error, Debug)]
#[error("ReachedMaxBufferSize: `{0}`")]
pub struct AsyncBufferFullError(pub usize);

pub struct AsyncBuffer<T> {
    buffer: VecDeque<T>,
    promise: Option<oneshot::Receiver<()>>,
    resolve: Option<oneshot::Sender<()>>,
    closed: bool,
    to_throw: Option<Box<dyn Error + Send + Sync>>,
    max_size: Option<usize>,
}

impl<T> AsyncBuffer<T> {
    pub fn new(max_size: Option<usize>) -> Self {
        let (resolve, promise) = oneshot::channel();
        AsyncBuffer {
            buffer: VecDeque::new(),
            promise: Some(promise),
            resolve: Some(resolve),
            closed: false,
            to_throw: None,
            max_size,
        }
    }

    pub fn reset_promise(&mut self) {
        let (resolve, promise) = oneshot::channel();
        self.promise = Some(promise);
        self.resolve = Some(resolve);
    }

    pub fn push(&mut self, item: T) {
        self.buffer.push_back(item);
        if let Some(resolve) = self.resolve.take() {
            let _ = resolve.send(());
        }
    }

    pub fn push_many(&mut self, items: Vec<T>) {
        for item in items {
            self.buffer.push_back(item);
        }
        if let Some(resolve) = self.resolve.take() {
            let _ = resolve.send(());
        }
    }

    pub fn throw(&mut self, err: Box<dyn Error + Send + Sync>) {
        self.to_throw = Some(err);
        self.closed = true;
        if let Some(resolve) = self.resolve.take() {
            let _ = resolve.send(());
        }
    }

    pub fn close(&mut self) {
        self.closed = true;
        if let Some(resolve) = self.resolve.take() {
            let _ = resolve.send(());
        }
    }
}

impl<T: Unpin> Stream for AsyncBuffer<T> {
    type Item = Result<T, Box<dyn Error + Send + Sync>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.closed && self.buffer.is_empty() {
            if let Some(err) = self.to_throw.take() {
                return Poll::Ready(Some(Err(err)));
            } else {
                return Poll::Ready(None);
            }
        }

        if let Some(promise) = self.promise.as_mut() {
            match Pin::new(promise).poll(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(_) => {}
            }
        }

        if let Some(err) = self.to_throw.take() {
            return Poll::Ready(Some(Err(err)));
        }

        if let Some(max_size) = self.max_size {
            if self.buffer.len() > max_size {
                return Poll::Ready(Some(Err(Box::new(AsyncBufferFullError(max_size)))));
            }
        }

        if let Some(first) = self.buffer.pop_front() {
            Poll::Ready(Some(Ok(first)))
        } else {
            self.reset_promise();
            Poll::Pending
        }
    }
}
