use futures::stream::Stream;
use futures::task::{Context, Poll};
use std::collections::VecDeque;
use std::error::Error;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::Waker;
use thiserror::Error;

#[derive(Error, Debug)]
#[error("ReachedMaxBufferSize: `{0}`")]
pub struct AsyncBufferFullError(pub usize);

pub struct AsyncBuffer<T> {
    buffer: Arc<Mutex<VecDeque<T>>>,
    closed: Arc<Mutex<bool>>,
    waker: Arc<Mutex<Option<Waker>>>,
    to_throw: Arc<Mutex<Option<Box<dyn Error + Send + Sync>>>>,
    max_size: Option<usize>,
}

impl<T> AsyncBuffer<T> {
    pub fn new(max_size: Option<usize>) -> Self {
        AsyncBuffer {
            buffer: Arc::new(Mutex::new(VecDeque::new())),
            closed: Arc::new(Mutex::new(false)),
            waker: Arc::new(Mutex::new(None)),
            to_throw: Arc::new(Mutex::new(None)),
            max_size,
        }
    }

    pub fn push(&self, item: T) {
        println!("@LOG: Pushing to buffer.");
        let mut buffer = self.buffer.lock().unwrap();
        buffer.push_back(item);
        if let Some(waker) = self.waker.lock().unwrap().take() {
            waker.wake();
        }
    }

    pub fn push_many(&self, items: Vec<T>) {
        println!("@LOG: Pushing many to buffer.");
        let mut buffer = self.buffer.lock().unwrap();
        for item in items {
            buffer.push_back(item);
        }
        if let Some(waker) = self.waker.lock().unwrap().take() {
            waker.wake();
        }
    }

    pub fn throw(&self, err: Box<dyn Error + Send + Sync>) {
        println!("@LOG: Asyncbuffer throwing error.");
        let mut to_throw = self.to_throw.lock().unwrap();
        *to_throw = Some(err);
        let mut closed = self.closed.lock().unwrap();
        *closed = true;
        if let Some(waker) = self.waker.lock().unwrap().take() {
            waker.wake();
        }
    }

    pub fn close(&self) {
        println!("@LOG: Closing Asyncbuffer.");
        let mut closed = self.closed.lock().unwrap();
        *closed = true;
        if let Some(waker) = self.waker.lock().unwrap().take() {
            waker.wake();
        }
    }
}

impl<T: Unpin> Stream for AsyncBuffer<T> {
    type Item = Result<T, Box<dyn Error + Send + Sync>>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        println!("@LOG: Entered poll_next");

        // Lock the closed state
        let closed = *self.closed.lock().unwrap();
        let mut buffer = self.buffer.lock().unwrap();

        if closed && buffer.is_empty() {
            println!("@LOG: Buffer closed or empty");
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
            println!("@LOG: poll_next err.");
            return Poll::Ready(Some(Err(err)));
        }

        // Check if the buffer size exceeds the max_size
        if let Some(max_size) = self.max_size {
            if buffer.len() > max_size {
                println!("@LOG: poll_next max_size.");
                return Poll::Ready(Some(Err(Box::new(AsyncBufferFullError(max_size)))));
            }
        }

        // Retrieve the next item from the buffer
        if let Some(first) = buffer.pop_front() {
            println!("@LOG: poll_next pop_front.");
            return Poll::Ready(Some(Ok(first)));
        } else {
            println!("@LOG: poll_next poll::pending.");
            return Poll::Pending;
        }
    }
}
