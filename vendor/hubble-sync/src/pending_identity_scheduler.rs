//! Repo identity resolution retry queue + discovery entry
//!
//! repos discovered on the firehose usually don't go through this queue: they
//! only land here if their initial actor-wake identity resolution fails.
//!
//! all backfill-discovered repos *do* go through here though:
//!
//! 1. `RepoDiscovery::request_sync` pushes a batch of DIDs into the queue. the
//!    queue applies backpressure (delaying its async return) when full.
//! 2. The new DIDs are upserted into the queue. The entire queue is always kept
//!    in memory, but also writes to storage, so that it can be recovered on
//!    restart.
//! 3. The driver pops due DIDs and dispatches ScheduledIdentityResolve messages
//!    to the repo registry, which attempts to wake (and resolve) the identity.
//! 4. If the DID was not previously known to the system, the repo will be
//!    initialized as `Desynchronized`, and get a `FirstSeen` resync scheduled.
//! 5. The resynch scheduler picks that up and kick off the actual resync!
//!
//! Initial repo identity resolution failures (re)enter *this* queue (repos
//! cannot be initialized without a resolved identity), to be retried some more
//! times later.

use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use metrics::{counter, gauge};
use tokio::sync::{Notify, Semaphore};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, trace};

use crate::metrics::{
    BACKFILL_ENQUEUED_TOTAL, BACKFILL_REQUESTED_TOTAL, PENDING_IDENTITY_ALREADY_RESOLVED_TOTAL,
    PENDING_IDENTITY_SCHEDULER_DISPATCHED_TOTAL, PENDING_IDENTITY_SCHEDULER_SCHEDULED,
};
use crate::repo_actor::{RepoMessage, RepoRegistry};
use crate::storage::repo::{
    AccountStatus, AccountStatusSource, PendingIdentity, PendingIdentityQueueEntry, RepoInfo,
};
use crate::{
    CancelExt, Did, DidMethod, LoadError, PrefixedEngine, RepoSendError, Resolve, StorageBatch,
    StorageEngine, SyncConsumer,
};

const IDLE_POLL: Duration = Duration::from_secs(15);
const REGISTRY_SEND_BACKOFF: Duration = Duration::from_secs(15);

pub struct DiscoveredRepo {
    pub did: Did,
    pub moderation: Option<(AccountStatus, AccountStatusSource)>,
}

pub struct PendingScheduler {
    inner: Mutex<Schedule>,
    /// backpressuring queue limit for pushes from repo discovery
    ///
    /// when a batch of repos from discovery would exceed the limit, the batch
    /// waits until enough items have been popped by `next` to fit.
    ///
    /// note: firehose discovery does *not* check this limit.
    discovery_limit: NonZeroUsize,
    /// backpressure notifier when space is freed
    popped: Notify,
}

#[derive(Default)]
struct Schedule {
    ordered: BTreeSet<(SystemTime, Did)>,
    by_did: HashMap<Did, SystemTime>,
}

impl PendingScheduler {
    pub fn new(discovery_limit: NonZeroUsize) -> Self {
        Self {
            inner: Default::default(),
            discovery_limit,
            popped: Notify::new(),
        }
    }

    fn rm(s: &mut Schedule, did: &Did) {
        if let Some(t) = s.by_did.remove(did) {
            s.ordered.remove(&(t, did.clone()));
        }
    }

    pub fn upsert(&self, did: &Did, due: SystemTime) {
        let mut s = self.inner.lock().expect("pending scheduler lock");
        Self::rm(&mut s, did);
        s.ordered.insert((due, did.clone()));
        s.by_did.insert(did.clone(), due);
        gauge!(PENDING_IDENTITY_SCHEDULER_SCHEDULED).set(s.by_did.len() as f64);
    }

    /// add multiple DIDs at once
    ///
    /// like calling `upsert` in a loop, but does it all under a single lock
    /// acquisition
    pub fn upsert_batch(&self, dids: Vec<Did>, due: SystemTime) {
        let mut s = self.inner.lock().expect("pending scheduler lock");
        for did in dids {
            Self::rm(&mut s, &did);
            s.ordered.insert((due, did.clone()));
            s.by_did.insert(did, due);
        }
        gauge!(PENDING_IDENTITY_SCHEDULER_SCHEDULED).set(s.by_did.len() as f64);
    }

    pub fn remove(&self, did: &Did) {
        let mut s = self.inner.lock().expect("pending scheduler lock");
        Self::rm(&mut s, did);
        gauge!(PENDING_IDENTITY_SCHEDULER_SCHEDULED).set(s.by_did.len() as f64);
    }

    pub fn next(&self, now: SystemTime) -> Option<PendingIdentityQueueEntry> {
        let mut s = self.inner.lock().expect("pending scheduler lock");
        // peek + bail if first is not yet due
        if now < s.ordered.first()?.0 {
            return None;
        }
        // first due, so take it out for real
        let (due, did) = s.ordered.pop_first().expect("just found under lock");
        s.by_did.remove(&did);
        gauge!(PENDING_IDENTITY_SCHEDULER_SCHEDULED).set(s.by_did.len() as f64);

        // batches from backfill must wait for space; popping a due item might
        // free up what they need, so let them know.
        //
        // can't use `notify_waiters()` because then they would *all* think that
        // it's safe to push under the limit, and might overshoot by a lot.
        //
        // `notify_first()` would work, but since waiters migth need to wait for
        // many `next()` calls (up to their batch size) and re-register their
        // waits if that size isn't available, the "first" waiter doesn't get to
        // hold that first-position spot. if later batches are smaller, they
        // might starve a large batch as they get cycled in as the "first"
        // waiter.
        //
        // using `notify_last()` doesn't entirely prevent starvation, but it
        // at least gives things a chance. the waiters become a kind of pseudo-
        // LIFO queue, where at least a large last-added batch won't keep
        // getting cycled-past. i guess it can still be starved by *new* batches
        // arriving, soooo maybe this mini-essay is a rationalization of
        // something not-very-effetive.
        //
        // but anyway.
        //
        // it *tries*, a bit, to keep polling the same re-registering waiter
        // until it gets through.
        //
        // usually the batch sizes shouldn't be to widly varying, so hpoefully
        // this all doesn't matter much.
        self.popped.notify_last();
        Some(PendingIdentityQueueEntry { did, due })
    }

    pub fn until_next(&self, now: SystemTime) -> Option<Duration> {
        let s = self.inner.lock().expect("pending scheduler lock");
        let (t, _) = s.ordered.first()?;
        Some(t.duration_since(now).unwrap_or(Duration::ZERO))
    }

    /// wait until there is at least `space_needed` less DIDs in the queue than
    /// `discovery_limit` that are already due.
    ///
    /// there will have been that much space before this function returns, but
    /// new DIDs being added might race with follow-up inserts. this just means
    /// that the total number of queued DIDs can sometimes overshoot the limit.
    ///
    /// if a batch larger than `discovery_limit` is submitted, then this
    /// function just waits until the currently-due set is empty. if you follow
    /// up with an oversized insert, it will just be accepted as overshoot. it's
    /// best not to try to insert mega-sized batches.
    async fn wait_for_space(&self, space_needed: usize) {
        let limit = self.discovery_limit.into();
        loop {
            let now = SystemTime::now();

            // set up early to avoid falling into a gap between checking the
            // if space is free and a space opening up. since notifies use
            // `.notify_last()`, we need to explicitly enable our receiver so
            // that we become eligible before being polled. See
            // [`tokio::sync::Notify::notified`].
            let mut notified = std::pin::pin!(self.popped.notified());
            notified.as_mut().enable();

            let current_num_due = self.len_due(now);
            if current_num_due + space_needed <= limit {
                // ready: has space
                break;
            }
            if space_needed > limit && current_num_due == 0 {
                // special case: more-than-limit space needed accepted when
                // current_num_due reaches zero.
                break;
            }
            trace!(
                current_num_due,
                space_needed,
                limit = self.discovery_limit,
                "repo discovery/backfill backpressure"
            );
            notified.await;
        }
    }

    pub fn len_due(&self, now: SystemTime) -> usize {
        self.inner
            .lock()
            .expect("pending scheduler lock")
            .ordered
            .iter()
            .take_while(|(t, _)| *t <= now)
            .count()
    }

    pub fn len(&self) -> usize {
        self.inner
            .lock()
            .expect("pending scheduler lock")
            .by_did
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner
            .lock()
            .expect("pending scheduler lock")
            .by_did
            .is_empty()
    }
}

impl fmt::Debug for PendingScheduler {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("PendingScheduler")
            .field("size", &self.inner.try_lock().map(|g| g.by_did.len()))
            .finish_non_exhaustive()
    }
}

/// Out-of-band (off-firehose) new repo intake
///
/// used by eg., backfill crawlers to notify hubble-sync of repos that exist,
/// that it might not have seen over the firehose.
#[derive(Clone)]
pub struct RepoDiscovery<S: StorageEngine> {
    storage: S,
    scheduler: Arc<PendingScheduler>,
}

impl<S: StorageEngine> RepoDiscovery<S> {
    pub fn new(storage: S, scheduler: Arc<PendingScheduler>) -> Self {
        Self { storage, scheduler }
    }

    /// submit a batch of repos (as DIDs) to be synced by hubble-sync
    ///
    /// pending repos are kept in an in-memory queue *and* persisted to storage
    /// so that they can be recovered on restart.
    ///
    /// this function applies backpressure (by waiting) when the pending
    /// identity queue is too full to accept the batch. if the number of DIDs is
    /// greater than the limit (don't do this), they will *all* be inserted once
    /// queue is empty of any due items, overshooting the limit.
    ///
    /// use the `with_batch` callback to add atomic storage mutations to the
    /// queue's own write batch. for example: saving a recoverable `listRepos`
    /// cursor corresponding to the page following the submitted page of DIDs.
    pub async fn request_sync(
        &self,
        repos: Vec<DiscoveredRepo>,
        with_batch: impl FnOnce(&mut S::Batch),
    ) -> Result<(), LoadError<S::Error>> {
        let now = SystemTime::now();
        counter!(BACKFILL_REQUESTED_TOTAL).increment(repos.len() as u64);

        // skip repos we already know about
        let storage = self.storage.clone();
        let fresh: Vec<DiscoveredRepo> =
            tokio::task::spawn_blocking(move || -> Result<_, LoadError<S::Error>> {
                let mut fresh = Vec::with_capacity(repos.len());
                for r in repos {
                    if RepoInfo::exists(&storage, &r.did)? {
                        continue;
                    }
                    fresh.push(r);
                }
                Ok(fresh)
            })
            .await
            .expect("no task panic")?;

        let n_fresh = fresh.len();
        self.scheduler.wait_for_space(n_fresh).await;
        // if we get here, there at least *was* enough free space...

        let mut batch = self.storage.batch();
        for repo in &fresh {
            PendingIdentity::enqueue_new(
                repo.did.clone(),
                repo.moderation.clone(),
                now,
                &mut batch,
            );
        }
        with_batch(&mut batch);

        tokio::task::spawn_blocking(move || batch.commit())
            .await
            .expect("no task panic")
            .map_err(LoadError::Storage)?;

        let fresh_dids = fresh.into_iter().map(|r| r.did).collect();

        // ...we didn't get a reservation for the free space, so it's not
        // actually guaranteed to still be availble. but, overshoots are just a
        // resource excursion, not a correctness problem, so, whatever.
        self.scheduler.upsert_batch(fresh_dids, now);

        counter!(BACKFILL_ENQUEUED_TOTAL).increment(n_fresh as u64);
        Ok(())
    }
}

/// Initial in-memory hydration from the db
///
/// blocking!!
///
/// TODO: probably goes on PendingScheduler impl
pub fn bootstrap<S: StorageEngine>(
    scheduler: &PendingScheduler,
    storage: &S,
) -> Result<usize, LoadError<S::Error>> {
    let mut count = 0;
    for entry in PendingIdentityQueueEntry::scan(storage) {
        let entry = entry?;
        scheduler.upsert(&entry.did, entry.due);
        count += 1;
    }
    Ok(count)
}

/// drive the scheduler until cancelled
///
/// TODO: probably goes on PendingScheduler impl
pub async fn drive<S: StorageEngine, A: SyncConsumer<Engine = S>, R: Resolve>(
    scheduler: Arc<PendingScheduler>,
    plc_resolve_limit: Arc<Semaphore>,
    storage: PrefixedEngine<S>,
    repo_registry: Arc<RepoRegistry<S, A, R>>,
    cancel: CancellationToken,
) -> Result<(), LoadError<S::Error>> {
    let bs_scheduler = scheduler.clone();
    let bs_storage = storage.clone();
    tokio::task::spawn_blocking(move || bootstrap(&bs_scheduler, &bs_storage))
        .await
        .expect("no storage panic")
        .inspect(|n| info!(n, "initial pending-identity queue loaded from db"))?;

    while !cancel.is_cancelled() {
        let now = SystemTime::now();
        let wait = scheduler
            .until_next(now)
            .unwrap_or(IDLE_POLL)
            .min(IDLE_POLL);
        if !cancel.sleep(wait).await {
            return Ok(()); // cancelled
        }
        let now = SystemTime::now();
        while let Some(entry) = scheduler.next(now) {
            let did = entry.did.clone();

            // reconcile first: a resolved repo can just clean up
            let s = storage.clone();
            let already_resolved =
                tokio::task::spawn_blocking(move || -> Result<_, LoadError<S::Error>> {
                    if RepoInfo::exists(&s, &entry.did)? {
                        let mut b = s.batch();
                        PendingIdentity::reconcile_delete(entry, &mut b);
                        b.commit().map_err(LoadError::Storage)?;
                        Ok(true)
                    } else {
                        Ok(false)
                    }
                })
                .await
                .expect("no panic reconciling pending")?;

            if already_resolved {
                counter!(PENDING_IDENTITY_ALREADY_RESOLVED_TOTAL).increment(1);
                continue;
            }
            counter!(PENDING_IDENTITY_SCHEDULER_DISPATCHED_TOTAL).increment(1);

            let resolve_permit = if matches!(did.method(), DidMethod::Plc) {
                // concurrency bound on PLC throttles our requests to it
                //
                // we probably should have an actual rate-limit as well, since
                // this couples backfill to plc RTT
                //
                // either way, we want to leave headroom for firehose-driven
                // resolution, which isn't subject to the limit.
                //
                // and: for now, we just wait here. we could be blocking did:web
                // resolutions from proceeding, but _for now_ that's a low-
                // enough impact with acceptably small effect.
                let Some(permit) = cancel
                    .run(plc_resolve_limit.clone().acquire_owned())
                    .await
                    .transpose()
                    .expect("semaphore not closed")
                else {
                    return Ok(()); // bail: cancelled
                };
                Some(permit)
            } else {
                // no bound on did-web concurrency for now (+not shared with plc limit)
                None
            };

            let message = RepoMessage::ScheduledIdentityResolve { resolve_permit };
            if let Err(e) = repo_registry.try_send(&did, message) {
                if matches!(e, RepoSendError::Draining(_)) {
                    // seems we're shutting down
                    info!(%did, "dropping pending identity resolve because repo registry is draining");
                    return Ok(());
                }
                error!(?e, %did, "pending resolution retry dispatch failed");

                // if sending failed, we should sloowwwww doooooooowwwwwnnn
                if !cancel.sleep(REGISTRY_SEND_BACKOFF).await {
                    // cancelled (likely if we're failing sends?)
                    return Ok(());
                }

                // put it back in the scheduler so it gets retried and stays
                // consistent with the on-disk queue (except the timestamp)
                let due = SystemTime::now();
                scheduler.upsert(&did, due);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::UNIX_EPOCH;

    use super::*;
    use crate::StorageBatch;
    use crate::storage::engine::mem::MemEngine;

    fn plc_did(label: &str) -> Did {
        let mut s = label.to_string();
        while s.len() < 24 {
            s.push('a');
        }
        s.truncate(24);
        Did::new(format!("did:plc:{s}")).expect("valid plc did")
    }

    fn t(ms: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_millis(ms)
    }

    #[test]
    fn empty_scheduler_has_no_work() {
        let s = PendingScheduler::new(1000.try_into().unwrap());
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
        assert!(s.next(t(0)).is_none());
        assert!(s.until_next(t(0)).is_none());
    }

    #[test]
    fn upsert_then_next_returns_due_did() {
        let s = PendingScheduler::new(1000.try_into().unwrap());
        s.upsert(&plc_did("a"), t(100));
        assert_eq!(s.len(), 1);

        let got = s.next(t(200)).expect("a is due");
        assert_eq!(got.did, plc_did("a"));
        assert!(s.is_empty(), "next consumes the entry");
    }

    #[test]
    fn next_skips_entries_that_arent_due_yet() {
        // BUG: PendingScheduler::next currently pops the earliest entry
        // regardless of `now`. This test will fail until next checks
        // due-time against `now`, mirroring ResyncScheduler's behavior.
        let s = PendingScheduler::new(1000.try_into().unwrap());
        s.upsert(&plc_did("future"), t(1_000));
        assert!(
            s.next(t(500)).is_none(),
            "next should return None when nothing is due"
        );
        assert_eq!(s.len(), 1, "entry must remain scheduled when not yet due");
    }

    #[test]
    fn not_due_entry_stays_dispatchable() {
        // regression: `next` used to `pop_first()` and only bail on `now`
        // *after* removing the entry from `ordered` (leaving it in `by_did`).
        // so `len()` still read 1 while the entry became undispatchable —
        // which is exactly why `next_skips_entries_that_arent_due_yet`'s
        // len()-check passed despite the bug. verify the entry is actually
        // still returned by a later, due `next`.
        let s = PendingScheduler::new(1000.try_into().unwrap());
        s.upsert(&plc_did("future"), t(1_000));

        assert!(s.next(t(500)).is_none(), "not due yet");
        assert_eq!(s.len(), 1);

        // with the bug this returned None (entry was gone from `ordered`)
        let got = s
            .next(t(1_500))
            .expect("must still be dispatchable once due");
        assert_eq!(got.did, plc_did("future"));
        assert!(s.is_empty());
    }

    #[tokio::test]
    async fn wait_for_space_blocks_until_a_slot_frees() {
        // limit 2, filled with two due entries: a waiter needing 1 slot must
        // block until `next` pops one and notifies.
        let s = Arc::new(PendingScheduler::new(2.try_into().unwrap()));
        s.upsert(&plc_did("a"), t(0)); // t(0) == epoch ⇒ always due
        s.upsert(&plc_did("b"), t(0));

        let waiter = {
            let s = s.clone();
            tokio::spawn(async move { s.wait_for_space(1).await })
        };
        tokio::task::yield_now().await; // let it park on the notify
        assert!(
            !waiter.is_finished(),
            "must backpressure while at the limit"
        );

        assert!(s.next(t(1)).is_some(), "free a slot (notifies the waiter)");

        tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .expect("waiter should unblock once a slot frees")
            .expect("waiter task panicked");
    }

    #[test]
    fn next_returns_earliest_due() {
        let s = PendingScheduler::new(1000.try_into().unwrap());
        s.upsert(&plc_did("late"), t(900));
        s.upsert(&plc_did("early"), t(100));
        s.upsert(&plc_did("mid"), t(500));

        let first = s.next(t(2_000)).expect("earliest due");
        assert_eq!(first.did, plc_did("early"));
        let second = s.next(t(2_000)).expect("next earliest");
        assert_eq!(second.did, plc_did("mid"));
        let third = s.next(t(2_000)).expect("last");
        assert_eq!(third.did, plc_did("late"));
        assert!(s.is_empty());
    }

    #[test]
    fn upsert_existing_did_updates_due_time() {
        let s = PendingScheduler::new(1000.try_into().unwrap());
        s.upsert(&plc_did("a"), t(100));
        s.upsert(&plc_did("a"), t(5_000));
        // still only one entry — upsert replaced, didn't duplicate
        assert_eq!(s.len(), 1);
        let dur = s.until_next(t(0)).expect("entry present");
        assert_eq!(dur, Duration::from_millis(5_000));
    }

    #[test]
    fn upsert_existing_did_moves_in_ordering() {
        let s = PendingScheduler::new(1000.try_into().unwrap());
        s.upsert(&plc_did("a"), t(100));
        s.upsert(&plc_did("b"), t(500));
        // bump a far into the future
        s.upsert(&plc_did("a"), t(10_000));

        let first = s.next(t(20_000)).expect("earliest");
        assert_eq!(
            first.did,
            plc_did("b"),
            "b should now be earliest since a was pushed out"
        );
    }

    #[test]
    fn remove_takes_out_a_did() {
        let s = PendingScheduler::new(1000.try_into().unwrap());
        s.upsert(&plc_did("a"), t(100));
        s.upsert(&plc_did("b"), t(200));
        s.remove(&plc_did("a"));

        assert_eq!(s.len(), 1);
        let got = s.next(t(1_000)).expect("b remains");
        assert_eq!(got.did, plc_did("b"));
    }

    #[test]
    fn remove_unknown_did_is_noop() {
        let s = PendingScheduler::new(1000.try_into().unwrap());
        s.upsert(&plc_did("a"), t(100));
        s.remove(&plc_did("ghost"));
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn wait_returns_none_when_empty() {
        let s = PendingScheduler::new(1000.try_into().unwrap());
        assert!(s.until_next(t(0)).is_none());
    }

    #[test]
    fn wait_returns_zero_when_anything_due() {
        let s = PendingScheduler::new(1000.try_into().unwrap());
        s.upsert(&plc_did("a"), t(100));
        s.upsert(&plc_did("b"), t(10_000));
        let dur = s.until_next(t(500)).expect("has entries");
        assert_eq!(dur, Duration::ZERO);
    }

    #[test]
    fn wait_returns_earliest_when_none_due() {
        let s = PendingScheduler::new(1000.try_into().unwrap());
        s.upsert(&plc_did("a"), t(10_000));
        s.upsert(&plc_did("b"), t(5_000));
        let dur = s.until_next(t(0)).expect("has entries");
        assert_eq!(dur, Duration::from_millis(5_000));
    }

    fn write_pq_entry(eng: &MemEngine, did: Did, due: SystemTime) {
        let mut b = eng.batch();
        PendingIdentityQueueEntry { did, due }.insert(&mut b);
        b.commit().unwrap();
    }

    #[test]
    fn bootstrap_empty_db_returns_zero() {
        let eng = MemEngine::new();
        let s = PendingScheduler::new(1000.try_into().unwrap());
        let n = bootstrap(&s, &eng).expect("ok");
        assert_eq!(n, 0);
        assert!(s.is_empty());
    }

    #[test]
    fn bootstrap_hydrates_all_entries() {
        let eng = MemEngine::new();
        write_pq_entry(&eng, plc_did("a"), t(500));
        write_pq_entry(&eng, plc_did("b"), t(1_500));
        write_pq_entry(&eng, plc_did("c"), t(2_500));

        let s = PendingScheduler::new(1000.try_into().unwrap());
        let n = bootstrap(&s, &eng).expect("ok");
        assert_eq!(n, 3);
        assert_eq!(s.len(), 3);

        let first = s.next(t(10_000)).expect("a");
        assert_eq!(first.did, plc_did("a"));
        let second = s.next(t(10_000)).expect("b");
        assert_eq!(second.did, plc_did("b"));
        let third = s.next(t(10_000)).expect("c");
        assert_eq!(third.did, plc_did("c"));
    }
}
