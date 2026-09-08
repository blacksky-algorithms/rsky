//! cancellation token nice-extensions trait

use tokio::time::{Duration, error::Elapsed, sleep, timeout};
use tokio_util::sync::CancellationToken;

/// nice helpers on `tokio_util::sync::CancellationToken`s
pub trait CancelExt {
    /// alias for CancellationToken::run_until_cancelled
    ///
    /// returns None if the token cancels before the future completes
    fn run<F: Future>(&self, fut: F) -> impl Future<Output = Option<F::Output>>;
    /// sleep that ends early if if cancelled
    ///
    /// returns `true` when completed after the full sleep completed
    fn sleep(&self, d: Duration) -> impl Future<Output = bool>;
    /// runs the future wrapped in a timeout wrapped in a token canceller
    ///
    /// returns None if the token cancels before the timeout or future completes
    /// returns Some(Err(elapsed)) if the timeout completes before the future
    /// otherwise returns Some(Ok(F::Output)) if the future gets to finish
    fn timeout<F: Future>(
        &self,
        d: Duration,
        fut: F,
    ) -> impl Future<Output = Option<Result<F::Output, Elapsed>>>;
}

impl CancelExt for CancellationToken {
    async fn run<F: Future>(&self, f: F) -> Option<F::Output> {
        self.run_until_cancelled(f).await
    }
    async fn sleep(&self, d: Duration) -> bool {
        self.run_until_cancelled(sleep(d)).await.is_some()
    }
    async fn timeout<F: Future>(&self, d: Duration, fut: F) -> Option<Result<F::Output, Elapsed>> {
        self.run_until_cancelled(timeout(d, fut)).await
    }
}
