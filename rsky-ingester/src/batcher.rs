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
    rx: mpsc::Receiver<T>,
    flush_tx: mpsc::Sender<Vec<T>>,
}

impl<T: Send + 'static> Batcher<T> {
    pub fn new(max_size: usize, timeout_ms: u64) -> (mpsc::Sender<T>, mpsc::Receiver<Vec<T>>) {
        // Use bounded channel with capacity = 2x batch size
        // This limits in-memory events and propagates backpressure to the sender
        let (tx, rx) = mpsc::channel(max_size * 2);
        // Use bounded channel for batches too (capacity = 4 batches)
        // This ensures backpressure propagates when write task is paused
        let (flush_tx, flush_rx) = mpsc::channel(4);

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
                                self.flush().await;
                            }
                        }
                        None => {
                            // Channel closed, flush remaining and exit
                            if !self.batch.is_empty() {
                                self.flush().await;
                            }
                            break;
                        }
                    }
                }
                // Timeout reached
                _ = sleep(time_until_flush) => {
                    if !self.batch.is_empty() {
                        self.flush().await;
                    }
                }
            }
        }
    }

    async fn flush(&mut self) {
        let batch = std::mem::replace(&mut self.batch, Vec::with_capacity(self.max_size));
        tracing::debug!("Batcher flushing {} events", batch.len());
        // Bounded send - will block when write task is paused, propagating backpressure
        if let Err(e) = self.flush_tx.send(batch).await {
            tracing::error!("Failed to send batch to write task: {:?}", e);
        }
        self.last_flush = Instant::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_batcher_size() {
        let (tx, mut rx) = Batcher::new(3, 1000);

        tx.send(1).await.unwrap();
        tx.send(2).await.unwrap();
        tx.send(3).await.unwrap();

        let batch = rx.recv().await.unwrap();
        assert_eq!(batch, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn test_batcher_timeout() {
        let (tx, mut rx) = Batcher::new(10, 100);

        tx.send(1).await.unwrap();
        tx.send(2).await.unwrap();

        let batch = rx.recv().await.unwrap();
        assert_eq!(batch, vec![1, 2]);
    }
}
