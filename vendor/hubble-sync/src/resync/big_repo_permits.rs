//! permit-acquisition-noting semaphore wrapper for big repo tracking
//!
//! the cost to resync a repo in atproto varies by like seven orders of
//! magnitude. there are a lot of very small repos, so it's valuable to process
//! resyncs for them with high concurrency. but a single resync concurrency
//! limit exposes the system to resource spikes if enough vary-large repos are
//! scheduled concurrently. the result is that you need to either be overly
//! conservative (slower backfill, higher normal resync latency), or risk an
//! unlucky resync ordering crashing the system.
//!
//! hubble-sync has one coarse "is-big" flag on repos to split resyncs into two
//! size categories: small (high concurrency) and big (low concurrency). this
//! helps it acheive high resync throughput while keeping resource spikes
//! relatively well contained.
//!
//! this works, but *newly discovered* repos pose a challenge: we don't know if
//! they are small or big before starting. and because everything is fun: this
//! matters *the most* during initial backfill, when all resources are the most
//! contended, and all repos are of unknown size.
//!
//! hubble-sync (with default resync data) takes an optimistic approach to
//! syncing new repos: no big-repo concurrency slot is taken to start, and we
//! try to proceed as if it's a small repo. we track the size as data is
//! streamed in, and if it reaches the "is-big" threshold, we pause and try to
//! acquire a big-repo concurrency slot before continuing. if we can't get a
//! slot (permit) within a few seconds (because too many other big repos are
//! already in progress), we abandon the attempt and schedule a retry later.
//!
//! but! if we retry (for any reason) a repo's first resync attempt, and we
//! found out on the first try that the repo is big, we should not take the
//! optimisitic path! generating the data for resync is expensive for PDSes too!
//!
//! so we *track* when a big-repo slot is taken, and save that fact on RepoInfo.
//! if the resync fails, then on retry we can pre-acquire the big-repo slot and
//! not risk abandoning *that* attempt mid-stream due to waiting for a slot.
//! (which, during initial backfill, all-new-repos, is when that's most likely)
//!
//! note: this tracking is specific to the *first* (re)sync attempt of a newly
//! discovered repo. normally, pre-acquiring a slot is determined by
//! `Resyncable::is_big()`, which receives the size+count and change metadata
//! from the last *successful* resync.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, TryAcquireError};

#[derive(Debug, Clone)]
pub struct BigRepoPermits {
    semaphore: Arc<Semaphore>,
    used: Arc<AtomicBool>,
}

/// thin acquire-noting wrapper for owned semaphore premits
///
/// the acquisition is noted so that failed optimistic resyncs (unknown repo
/// size, no permit pre-acquire) of big repos can take the pre-acquire path on
/// retry.
impl BigRepoPermits {
    pub(crate) fn new(semaphore: Arc<Semaphore>) -> Self {
        let used = Arc::new(AtomicBool::new(false));
        Self { semaphore, used }
    }
    /// acquire a big-repo permit for resync
    ///
    /// acquiring is recorded, first-resync attempts that obtained one mid
    /// flight will get a pre-acquired one on retry.
    pub async fn acquire_owned(&self) -> OwnedSemaphorePermit {
        self.used.store(true, Ordering::Relaxed);
        self.semaphore
            .clone()
            .acquire_owned()
            .await
            .expect("big repo semaphore not closed")
    }
    /// try to acquire without blocking
    ///
    /// otherwise like [`Self::acquire_owned`]
    pub fn try_acquire_owned(&self) -> Option<OwnedSemaphorePermit> {
        self.used.store(true, Ordering::Relaxed);
        self.semaphore
            .clone()
            .try_acquire_owned()
            .map_err(|e| match e {
                TryAcquireError::Closed => panic!("semaphore closed"),
                TryAcquireError::NoPermits => (),
            })
            .ok() // no permits -> none
    }

    pub(crate) fn was_used(&self) -> bool {
        self.used.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn starts_unused() {
        let permits = BigRepoPermits::new(Arc::new(Semaphore::new(1)));
        assert!(!permits.was_used());
    }

    #[tokio::test]
    async fn acquire_marks_used_and_holds_the_permit() {
        let sem = Arc::new(Semaphore::new(1));
        let permits = BigRepoPermits::new(sem.clone());

        let _permit = permits.acquire_owned().await;
        assert!(permits.was_used());
        assert_eq!(
            sem.available_permits(),
            0,
            "the permit is held until dropped"
        );
    }

    #[tokio::test]
    async fn used_flag_is_set_before_the_await_so_a_timed_out_acquire_still_counts() {
        // the whole point: a first-seen attempt that overflows, fails to get a
        // slot in time, and bails must still be recorded as big so the retry
        // pre-acquires.
        let sem = Arc::new(Semaphore::new(1));
        let permits = BigRepoPermits::new(sem.clone());
        let _held = sem.clone().acquire_owned().await.expect("semaphore open");

        // no permits free -> the wrapper's acquire blocks and times out
        let timed_out = tokio::time::timeout(Duration::from_millis(20), permits.acquire_owned())
            .await
            .is_err();

        assert!(timed_out, "acquire cannot succeed with no free permits");
        assert!(
            permits.was_used(),
            "the used flag is set before the acquire await"
        );
    }

    #[tokio::test]
    async fn clones_share_the_used_flag() {
        let permits = BigRepoPermits::new(Arc::new(Semaphore::new(1)));
        let clone = permits.clone();

        let _permit = clone.acquire_owned().await;
        assert!(
            permits.was_used(),
            "acquiring through a clone marks the shared flag"
        );
    }

    #[test]
    fn try_acquire_returns_a_permit_when_free_and_marks_used() {
        let sem = Arc::new(Semaphore::new(1));
        let permits = BigRepoPermits::new(sem.clone());

        let permit = permits.try_acquire_owned();
        assert!(permit.is_some(), "a free slot is acquired");
        assert!(permits.was_used());
        assert_eq!(
            sem.available_permits(),
            0,
            "the permit is held until dropped"
        );

        drop(permit);
        assert_eq!(sem.available_permits(), 1, "released on drop");
    }

    #[test]
    fn try_acquire_returns_none_when_saturated_but_still_marks_used() {
        // the fast-fail counterpart to the reactive-timeout case: a known-big
        // repo that finds no slot free must bail *and* stay flagged as big, so
        // its retry keeps taking the pre-acquire path rather than optimistically
        // re-downloading.
        let sem = Arc::new(Semaphore::new(1));
        let permits = BigRepoPermits::new(sem.clone());
        let _held = sem.clone().try_acquire_owned().expect("semaphore open");

        assert!(
            permits.try_acquire_owned().is_none(),
            "no slot free -> no permit"
        );
        assert!(
            permits.was_used(),
            "the used flag is set even when no permit was available"
        );
    }

    #[test]
    fn try_acquire_does_not_queue_behind_a_waiter() {
        // the whole reason pre-acquire uses try_acquire: it must not sit in the
        // semaphore's wait queue competing with reactive waiters for a freeing
        // permit. it only ever takes an already-free slot.
        let sem = Arc::new(Semaphore::new(1));
        let permits = BigRepoPermits::new(sem.clone());

        let held = permits.try_acquire_owned().expect("first slot free");
        assert!(permits.try_acquire_owned().is_none(), "saturated");

        // freeing a slot makes the next try succeed immediately — no lingering
        // reservation from the earlier miss.
        drop(held);
        assert!(
            permits.try_acquire_owned().is_some(),
            "a freed slot is takeable again"
        );
    }
}
