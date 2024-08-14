use crate::common::time::SECOND;
use crate::common::wait;
use futures::stream::Stream;
use futures::task::{Context, Poll};
use std::cmp;
use std::collections::VecDeque;
use std::error::Error;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::Waker;
use thiserror::Error;

#[derive(Error, Debug)]
#[error("ReachedMaxBufferSize: `{0}`")]
pub struct AsyncBufferFullError(pub usize);

pub struct AsyncBuffer<T> {
    pub buffer: Arc<Mutex<VecDeque<T>>>,
    closed: Arc<Mutex<bool>>,
    waker: Arc<Mutex<Option<Waker>>>,
    to_throw: Arc<Mutex<Option<Box<dyn Error + Send + Sync>>>>,
    max_size: Option<usize>,
    tries_with_no_results: Arc<Mutex<u32>>,
}

impl<T> AsyncBuffer<T> {
    pub fn new(max_size: Option<usize>) -> Self {
        AsyncBuffer {
            buffer: Arc::new(Mutex::new(VecDeque::new())),
            closed: Arc::new(Mutex::new(false)),
            waker: Arc::new(Mutex::new(None)),
            to_throw: Arc::new(Mutex::new(None)),
            max_size,
            tries_with_no_results: Arc::new(Mutex::new(0)),
        }
    }

    pub fn push(&self, item: T) {
        let mut buffer = self.buffer.lock().unwrap();
        buffer.push_back(item);
        if let Some(waker) = self.waker.lock().unwrap().take() {
            waker.wake();
        }
    }

    pub fn push_many(&self, items: Vec<T>) {
        let mut buffer = self.buffer.lock().unwrap();
        for item in items {
            buffer.push_back(item);
        }
        if let Some(waker) = self.waker.lock().unwrap().take() {
            waker.wake();
        }
    }

    pub fn throw(&self, err: Box<dyn Error + Send + Sync>) {
        let mut to_throw = self.to_throw.lock().unwrap();
        *to_throw = Some(err);
        let mut closed = self.closed.lock().unwrap();
        *closed = true;
        if let Some(waker) = self.waker.lock().unwrap().take() {
            waker.wake();
        }
    }

    pub fn close(&self) {
        let mut closed = self.closed.lock().unwrap();
        *closed = true;
        if let Some(waker) = self.waker.lock().unwrap().take() {
            waker.wake();
        }
    }

    fn exponential_backoff(&self, mut waker: MutexGuard<Option<Waker>>) -> () {
        let mut tries_with_no_results = self.tries_with_no_results.lock().unwrap();
        *tries_with_no_results += 1;
        let wait_time = cmp::min(
            2u64.checked_pow(*tries_with_no_results).unwrap_or(2),
            SECOND as u64,
        );
        wait(wait_time);
        if let Some(waker) = waker.take() {
            waker.wake();
        }
    }
}

impl<T: Unpin + std::fmt::Debug> Stream for AsyncBuffer<T> {
    type Item = Result<T, Box<dyn Error + Send + Sync>>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Lock the closed state
        let closed = *self.closed.lock().unwrap();
        let mut buffer = self.buffer.lock().unwrap();

        if closed && buffer.is_empty() {
            let mut to_throw = self.to_throw.lock().unwrap();
            return if let Some(err) = to_throw.take() {
                Poll::Ready(Some(Err(err)))
            } else {
                Poll::Ready(None)
            };
        }

        // Lock the waker
        let mut waker = self.waker.lock().unwrap();

        // Reset the waker
        *waker = Some(cx.waker().clone());

        // Check if there is an error to throw
        let mut to_throw = self.to_throw.lock().unwrap();
        if let Some(err) = to_throw.take() {
            return Poll::Ready(Some(Err(err)));
        }

        // Check if the buffer size exceeds the max_size
        if let Some(max_size) = self.max_size {
            if buffer.len() > max_size {
                return Poll::Ready(Some(Err(Box::new(AsyncBufferFullError(max_size)))));
            }
        }

        // Retrieve the next item from the buffer
        return if let Some(first) = buffer.pop_front() {
            let mut tries_with_no_results = self.tries_with_no_results.lock().unwrap();
            *tries_with_no_results = 0;
            Poll::Ready(Some(Ok(first)))
        } else {
            drop(buffer);
            self.exponential_backoff(waker);
            Poll::Pending
        };
    }
}
