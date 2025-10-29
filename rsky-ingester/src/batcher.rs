use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::time::sleep;

/// Batcher collects events and flushes them in batches
/// either when the batch is full or after a timeout
pub struct Batcher<T> {
    batch: Vec<T>,
    max_size: usize,
    timeout: Duration,
    last_flush: Instant,
    rx: mpsc::UnboundedReceiver<T>,
    flush_tx: mpsc::UnboundedSender<Vec<T>>,
}

impl<T: Send + 'static> Batcher<T> {
    pub fn new(
        max_size: usize,
        timeout_ms: u64,
    ) -> (mpsc::UnboundedSender<T>, mpsc::UnboundedReceiver<Vec<T>>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let (flush_tx, flush_rx) = mpsc::unbounded_channel();

        let mut batcher = Self {
            batch: Vec::with_capacity(max_size),
            max_size,
            timeout: Duration::from_millis(timeout_ms),
            last_flush: Instant::now(),
            rx,
            flush_tx,
        };

        tokio::spawn(async move {
            batcher.run().await;
        });

        (tx, flush_rx)
    }

    async fn run(&mut self) {
        loop {
            // Calculate time until next flush
            let elapsed = self.last_flush.elapsed();
            let time_until_flush = if elapsed >= self.timeout {
                Duration::from_millis(0)
            } else {
                self.timeout - elapsed
            };

            tokio::select! {
                // Receive new events
                maybe_event = self.rx.recv() => {
                    match maybe_event {
                        Some(event) => {
                            self.batch.push(event);
                            if self.batch.len() >= self.max_size {
                                self.flush();
                            }
                        }
                        None => {
                            // Channel closed, flush remaining and exit
                            if !self.batch.is_empty() {
                                self.flush();
                            }
                            break;
                        }
                    }
                }
                // Timeout reached
                _ = sleep(time_until_flush) => {
                    if !self.batch.is_empty() {
                        self.flush();
                    }
                }
            }
        }
    }

    fn flush(&mut self) {
        let batch = std::mem::replace(&mut self.batch, Vec::with_capacity(self.max_size));
        let _ = self.flush_tx.send(batch);
        self.last_flush = Instant::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_batcher_size() {
        let (tx, mut rx) = Batcher::new(3, 1000);

        tx.send(1).unwrap();
        tx.send(2).unwrap();
        tx.send(3).unwrap();

        let batch = rx.recv().await.unwrap();
        assert_eq!(batch, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn test_batcher_timeout() {
        let (tx, mut rx) = Batcher::new(10, 100);

        tx.send(1).unwrap();
        tx.send(2).unwrap();

        let batch = rx.recv().await.unwrap();
        assert_eq!(batch, vec![1, 2]);
    }
}
