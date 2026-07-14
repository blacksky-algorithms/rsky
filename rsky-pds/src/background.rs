// based on https://github.com/bluesky-social/atproto/blob/main/packages/pds/src/background.ts

use anyhow::Result;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{Notify, Semaphore};

const DEFAULT_CONCURRENCY: usize = 5;

#[derive(Debug)]
struct QueueState {
    pending: AtomicUsize,
    destroyed: AtomicBool,
    notify: Notify,
    semaphore: Semaphore,
}

/// A simple queue for in-process, out-of-band/backgrounded work.
/// Task failures are logged, never surfaced.
#[derive(Debug, Clone)]
pub struct BackgroundQueue {
    state: Arc<QueueState>,
}

impl Default for BackgroundQueue {
    fn default() -> Self {
        Self::new(DEFAULT_CONCURRENCY)
    }
}

impl BackgroundQueue {
    pub fn new(concurrency: usize) -> Self {
        BackgroundQueue {
            state: Arc::new(QueueState {
                pending: AtomicUsize::new(0),
                destroyed: AtomicBool::new(false),
                notify: Notify::new(),
                semaphore: Semaphore::new(concurrency),
            }),
        }
    }

    pub fn destroyed(&self) -> bool {
        self.state.destroyed.load(Ordering::SeqCst)
    }

    pub fn add<F>(&self, task: F)
    where
        F: Future<Output = Result<()>> + Send + 'static,
    {
        if self.destroyed() {
            return;
        }
        let state = self.state.clone();
        state.pending.fetch_add(1, Ordering::SeqCst);
        tokio::spawn(async move {
            match state.semaphore.acquire().await {
                Ok(_permit) => {
                    if let Err(err) = task.await {
                        tracing::error!(?err, "background queue task failed");
                    }
                }
                Err(err) => tracing::error!(?err, "background queue semaphore closed"),
            }
            state.pending.fetch_sub(1, Ordering::SeqCst);
            state.notify.notify_waiters();
        });
    }

    /// Waits for every queued task to finish.
    pub async fn process_all(&self) {
        loop {
            let notified = self.state.notify.notified();
            if self.state.pending.load(Ordering::SeqCst) == 0 {
                return;
            }
            notified.await;
        }
    }

    /// Stops accepting new tasks and completes all pending/in-progress tasks.
    pub async fn destroy(&self) {
        if self.state.destroyed.swap(true, Ordering::SeqCst) {
            tracing::warn!("BackgroundQueue::destroy() called multiple times");
        }
        self.process_all().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    #[tokio::test]
    async fn runs_tasks_and_drains() {
        let queue = BackgroundQueue::default();
        let counter = Arc::new(AtomicUsize::new(0));
        for _ in 0..10 {
            let counter = counter.clone();
            queue.add(async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(())
            });
        }
        queue.process_all().await;
        assert_eq!(counter.load(Ordering::SeqCst), 10);
        // draining an idle queue returns immediately
        queue.process_all().await;
    }

    #[tokio::test]
    async fn limits_concurrency() {
        let queue = BackgroundQueue::new(2);
        let running = Arc::new(AtomicUsize::new(0));
        let max_running = Arc::new(AtomicUsize::new(0));
        for _ in 0..8 {
            let running = running.clone();
            let max_running = max_running.clone();
            queue.add(async move {
                let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                max_running.fetch_max(now, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(10)).await;
                running.fetch_sub(1, Ordering::SeqCst);
                Ok(())
            });
        }
        queue.process_all().await;
        assert!(max_running.load(Ordering::SeqCst) <= 2);
    }

    #[tokio::test]
    async fn logs_and_swallows_task_errors() {
        let queue = BackgroundQueue::default();
        queue.add(async { anyhow::bail!("task failed") });
        queue.process_all().await;
        assert!(!queue.destroyed());
    }

    #[tokio::test]
    async fn destroy_stops_accepting_tasks() {
        let queue = BackgroundQueue::default();
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_before = counter.clone();
        queue.add(async move {
            counter_before.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });
        queue.destroy().await;
        assert!(queue.destroyed());
        let counter_after = counter.clone();
        queue.add(async move {
            counter_after.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });
        // second destroy warns but still drains
        queue.destroy().await;
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }
}
