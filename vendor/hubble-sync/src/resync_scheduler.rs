//! in-memory scheduler over the resync queue
//!
//! the resync queue itself is a partial secondary index over repos: a repo in
//! `desynchronized` state must have exactly one entry in the queue. the repo
//! owns that index entry, so that it can atomically insert and remove itself
//! when transitioning into and out of `desynchronized` state.
//!
//! the scheduler is an eventually-consistent in-memory read-only view into the
//! queue, to notify repos when their resync is due.
//!
//! also: do we need a hubble-sync-global "clock", to ensure that repos can't
//! write queue entries in the past? or does our eventual-consistency catch
//! those? i think we keep the time-lower-bound in memory to avoid scanning
//! tombstones, so this could be a real risk.
//!
//! big huge new caveat: now that we request repos from the direct upstream
//! (which is a sync-spec SHOULD), the per-host limit scheduling is a much less
//! precise!
//!
//! - pds upstream: we might undercount (migrated repo checks unrelated host)
//!   -> an actor wakes early, it's not optimal but not a huge big deal
//!   -> if the repo is now inactive on the upstream pds, it might fail fast
//!
//! - relay upstream: we might misattribute limit checks (migrated repo and
//!   we are out of sync with the relay)
//!   -> we can end up over- or under-gating dispatching to a host
//!
//! these are both non-fatal because the hosts ultimately apply their own
//! limits, but they mean our scheduler can't be perfect and optimal. it's still
//! worth keeping the host bucketing, because it'll typically be *mostly* right,
//! so *mostly* effective.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use governor::{DefaultDirectRateLimiter, Quota, RateLimiter};
use metrics::{counter, gauge};
use tokio::sync::{Semaphore, TryAcquireError};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, trace};

use crate::metrics::{
    RESYNC_CONCURRENCY_SATURATED_TOTAL, RESYNC_DISPATCH_FAILED_TOTAL,
    RESYNC_DISPATCH_THROTTLED_TOTAL, RESYNC_HOST_BACKPRESSURED_TOTAL,
    RESYNC_REBOOTSTRAP_ADDED_TOTAL, RESYNC_REBOOTSTRAP_PASSES_TOTAL,
    RESYNC_SCHEDULER_DISPATCHED_TOTAL, RESYNC_SCHEDULER_HOSTS_BY_STATE,
    RESYNC_SCHEDULER_ROUND_ROBIN_LEN, RESYNC_SCHEDULER_SCHEDULED,
};
use crate::repo_actor::{RepoMessage, RepoRegistry};
use crate::storage::repo::NextQueuedByHost;
use crate::{
    CancelExt, Host, Hostname, LoadError, PrefixedEngine, RepoSendError, Resolve, StorageEngine,
    StorageError,
};
use crate::{Did, HostRegistry, SyncConsumer};

const IDLE_POLL: Duration = Duration::from_secs(1);
/// wait when every due resync is stuck behind host concurrency
const BACKPRESSURE_POLL: Duration = Duration::from_millis(50);
/// how often we scan for new hosts
const REBOOTSTRAP_INTERVAL: Duration = Duration::from_secs(5);
/// keep resync dispatches at least this long behind "now"
///
/// helps any "now"-queued resync avoid getting accidentally behind a current-ms
/// cursor if its DID is lex-smaller than the cursor's. it *should* be safe to
/// set this to 1ms, probably worth measuring, but also a few ms is harmless for
/// a little margin.
///
/// anything that _does_ get behind the cursor will be eventually picked up at
/// the next cursors-drop (every [`CURSOR_DROP_INTERVAL`]).
const DISPATCH_LAG_MIN: Duration = Duration::from_millis(4);
/// catch stragglers, fix clock jumps, crashed/stuck attempts
const CURSOR_DROP_INTERVAL: Duration = Duration::from_secs(4);
/// all cursors dropped if this is somehow reached between normal drop intervals
const MAX_CURSORS: usize = 1_000_000;
/// queued resyncs per host to prefetch at once
const PREFETCH_SIZE: usize = 64;

#[derive(Debug, thiserror::Error)]
pub enum ResyncDriveError<E: StorageError> {
    #[error("load: {0}")]
    Load(#[from] LoadError<E>),
    #[error("dispatching resync for {did:?} stuck: {source}")]
    DispatchStuck { did: Did, source: RepoSendError },
}

/// in-memory round-robin-by-host resync scheduler
#[derive(Default)]
pub struct ResyncScheduler {
    inner: Mutex<Schedule>,
}

#[derive(Default)]
struct Schedule {
    /// currently-queued host work
    /// - inserting work for the same host replaces
    /// - removed on next()
    hosts: HashMap<Option<Hostname>, QueuedResync>,
    /// - inserting work for a new host goes to the back (so next() -> insert()
    ///   maintains round-robin order)
    round_robin: VecDeque<Option<Hostname>>,
    /// queued resyncs stay in the db until the resync *completes*, so we need
    /// to keep our own track of where we are in dispatching them.
    ///
    /// note: periodically we drop all cursors to force a re-scan (and some re-
    /// emits) of earlier resyncs, in case any get left behind from clock jumps,
    /// failed resyncs, etc.
    cursors: HashMap<Option<Hostname>, (SystemTime, Did)>,
}

/// a resync that's ready to go
#[derive(Debug)]
pub struct QueuedResync {
    pub host: Option<Arc<Host>>,
    pub did: Did,
    due: SystemTime,
}

impl QueuedResync {
    pub fn new(host: Option<Arc<Host>>, did: Did, due: SystemTime) -> Self {
        Self {
            host,
            did,
            due: crate::storage::unix_ms_u64::floor(due),
        }
    }

    pub fn set_due(&mut self, t: SystemTime) {
        self.due = crate::storage::unix_ms_u64::floor(t);
    }
    pub fn due(&self) -> SystemTime {
        self.due
    }
}

impl ResyncScheduler {
    pub fn new() -> Self {
        Self::default()
    }

    /// insert a scheduled resync, setting up the host if needed
    pub fn insert(&self, resync: QueuedResync) {
        let host = resync.host.as_ref().map(|h| h.name().clone());
        let mut schedule = self.inner.lock().expect("scheduler lock");
        let prev = schedule.hosts.insert(host.clone(), resync);
        if prev.is_none() {
            // only if this host is being freshly inserted: add it at the back
            // (at back is important because next() pops it from the front)
            schedule.round_robin.push_back(host);
            gauge!(RESYNC_SCHEDULER_SCHEDULED).set(schedule.hosts.len() as f64);
        }
    }

    /// insert only if no slot exists for this host + it's due after host cursor
    ///
    /// orrrr replace if it's after the host cursor but before the currently due
    ///
    /// this can happen if a future resync is waiting, and a new resync is then
    /// scheduled sooner for the same host.
    pub fn insert_if_absent_or_earlier(&self, resync: QueuedResync) -> bool {
        let host = resync.host.as_ref().map(|h| h.name().clone());
        let mut schedule = self.inner.lock().expect("scheduler lock");

        let occupied = match schedule.hosts.get(&host) {
            // we can bail here: there's already something scheduled, and we
            // are not meant to go before it
            Some(existing) if existing.due() <= resync.due() => return false,
            Some(_) => true,
            None => false,
        };

        // drop resyncs that are behind the host's current cursor (we probably
        // already dispatched)
        if let Some((due, did)) = schedule.cursors.get(&host)
            && !resync.is_queued_after(*due, did)
        {
            return false;
        }

        schedule.hosts.insert(host.clone(), resync);
        if !occupied {
            // new host: add to the round-robin
            schedule.round_robin.push_back(host);
        }
        gauge!(RESYNC_SCHEDULER_SCHEDULED).set(schedule.hosts.len() as f64);
        true
    }

    /// when the persisted queue for a host was fully emptied, we don't need to
    /// keep them in the round-robin
    pub fn unschedule_host(&self, host: &Option<Arc<Host>>) {
        let host = host.as_ref().map(|h| h.name().clone());
        let mut schedule = self.inner.lock().expect("scheduler lock");
        let actually_removed = schedule.hosts.remove(&host).is_some();
        if actually_removed {
            // clear from the round-robin order, the hard way. infrequent event.
            schedule.round_robin.retain(|h| *h != host);
            gauge!(RESYNC_SCHEDULER_SCHEDULED).set(schedule.hosts.len() as f64);
        }
    }

    /// update a host's cursor so the next db read gets the next entry
    ///
    /// all existing cursors are dropped if we somehow see a very huge number of
    /// them since our last drop_cursors() call. which is unlikely. but hey.
    pub fn record_cursor(&self, host: &Option<Arc<Host>>, due: SystemTime, did: Did) {
        let host = host.as_ref().map(|h| h.name().clone());
        let mut schedule = self.inner.lock().expect("scheduler lock");
        if schedule.cursors.len() >= MAX_CURSORS {
            // doing it directly instead of calling self.drop_cursors() since we
            // already hold a lock (bleh)
            schedule.cursors.clear();
        }
        schedule.cursors.insert(host, (due, did));
    }

    /// allow us to catch stragglers, fix clock jumps, crashed/stuck resyncs
    ///
    /// this can result in re-dispatching already-dispatched queued resyncs,
    /// which is fine (they get dropped by actor Intake or should become noops).
    pub fn drop_cursors(&self) {
        self.inner.lock().expect("scheduler lock").cursors.clear();
    }

    /// get the next ready and deliverable resync task, host-round-robinned
    ///
    /// skips not-yet-due entries and hosts at max concurrency
    ///
    /// caller should insert a host's next resync after popping one, to keep the
    /// round-robin order.
    pub fn next(&self, now: SystemTime) -> Option<QueuedResync> {
        let mut schedule = self.inner.lock().expect("scheduler lock");
        let mut nohost_deferred: Option<QueuedResync> = None; // don't queue no-host resyncs unless we have no other work
        let rr_len = schedule.round_robin.len();
        for _ in 0..rr_len {
            // up to one full loop around the robin
            let host = schedule
                .round_robin
                .pop_front()
                .expect("we entered the loop, so there must be at least one to pop");
            let resync = schedule
                .hosts
                .remove(&host)
                .expect("missing `host` for round-robin position (scheduler bug)");

            // make sure we *don't* dispatch resyncs that are now-or-future--
            // hold back a little to avoid leaving now-scheduled resyncs behind
            let lag = now.duration_since(resync.due).unwrap_or(Duration::ZERO);
            if lag < DISPATCH_LAG_MIN {
                // not due enough yet: reinsert, rotate
                schedule.hosts.insert(host.clone(), resync);
                schedule.round_robin.push_back(host);
                continue;
            }
            // skip past hosts who can't accept work; rotate to the back
            if resync
                .host
                .as_ref()
                .is_some_and(|h| h.at_max_concurrency() || h.backoff_remaining(now).is_some())
            {
                schedule.hosts.insert(host.clone(), resync);
                schedule.round_robin.push_back(host);
                continue;
            }
            if host.is_none() {
                // due, but unknown host
                nohost_deferred = Some(resync);
                continue;
            }
            // due, known host, deliverable: dispatch
            // (reinsert any deferred first)
            if let Some(q) = nohost_deferred.take() {
                schedule.hosts.insert(None, q);
                schedule.round_robin.push_back(None);
            }
            gauge!(RESYNC_SCHEDULER_SCHEDULED).set(schedule.hosts.len() as f64);
            return Some(resync);
        }
        // no known-host resyncs dispatched, check if we can fall back
        if let Some(resync) = nohost_deferred {
            gauge!(RESYNC_SCHEDULER_SCHEDULED).set(schedule.hosts.len() as f64);
            return Some(resync);
        }
        None
    }

    /// if nothing is due yet, how long until the next scheduled will be
    ///
    /// returns Some(ZERO) if there is no need to wait.
    /// returns Some(Duration) if nothing is ready -- how long to wait
    /// returns None if there are no active hosts at all.
    ///
    /// callers can bound their max wait times, to catch work scheduled while in
    /// a long wait.
    pub fn wait(&self, now: SystemTime) -> Option<Duration> {
        let schedule = self.inner.lock().expect("scheduler lock");
        let earliest = schedule.hosts.values().map(|r| r.due).min()?;
        let now_lagged = now - DISPATCH_LAG_MIN; // can't dispatch until min lag
        Some(
            earliest
                .duration_since(now_lagged)
                .unwrap_or(Duration::ZERO),
        )
    }

    /// snapshot scheduled hosts by why `next()` would (or wouldn't) dispatch
    /// them, into gauges. buckets are exclusive, categorized in `next()`'s own
    /// skip order, with ring-membership checked first (a host present in
    /// `hosts` but missing from `round_robin` is never visited by `next()`).
    ///
    /// diagnostic only; called from the drive loop's idle branch.
    pub fn record_state_gauges(&self, now: SystemTime) {
        let schedule = self.inner.lock().expect("scheduler lock");
        let in_ring: HashSet<&Option<Hostname>> = schedule.round_robin.iter().collect();

        let (mut not_in_ring, mut not_due, mut at_max, mut backoff, mut deliverable) =
            (0u64, 0u64, 0u64, 0u64, 0u64);
        for (host_key, resync) in &schedule.hosts {
            if !in_ring.contains(host_key) {
                not_in_ring += 1;
                continue;
            }
            let lag = now.duration_since(resync.due()).unwrap_or(Duration::ZERO);
            if lag < DISPATCH_LAG_MIN {
                not_due += 1;
                continue;
            }
            match resync.host.as_ref() {
                Some(h) if h.at_max_concurrency() => at_max += 1,
                Some(h) if h.backoff_remaining(now).is_some() => backoff += 1,
                _ => deliverable += 1,
            }
        }

        gauge!(RESYNC_SCHEDULER_ROUND_ROBIN_LEN).set(schedule.round_robin.len() as f64);
        gauge!(RESYNC_SCHEDULER_HOSTS_BY_STATE, "state" => "deliverable").set(deliverable as f64);
        gauge!(RESYNC_SCHEDULER_HOSTS_BY_STATE, "state" => "not_due").set(not_due as f64);
        gauge!(RESYNC_SCHEDULER_HOSTS_BY_STATE, "state" => "at_max").set(at_max as f64);
        gauge!(RESYNC_SCHEDULER_HOSTS_BY_STATE, "state" => "backoff").set(backoff as f64);
        gauge!(RESYNC_SCHEDULER_HOSTS_BY_STATE, "state" => "not_in_ring").set(not_in_ring as f64);
    }

    pub fn len(&self) -> usize {
        self.inner.lock().expect("scheduler lock").hosts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.lock().expect("scheduler lock").hosts.is_empty()
    }
}

impl fmt::Debug for ResyncScheduler {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ResyncScheduler")
            .field("size", &self.inner.try_lock().map(|g| g.hosts.len()))
            .finish_non_exhaustive()
    }
}

/// fill available host rsync slots from the database
///
/// skips adding in already-occupied scheduler host slots
///
/// TODO: probably goes on ResyncScheduler impl
async fn bootstrap<S: crate::StorageEngine>(
    scheduler: Arc<ResyncScheduler>,
    storage: S,
    registry: Arc<HostRegistry>,
) -> Result<usize, LoadError<S::Error>> {
    tokio::task::spawn_blocking(move || {
        let mut count = 0;
        for next_queued in NextQueuedByHost::new(&storage, &registry) {
            scheduler.insert_if_absent_or_earlier(next_queued?);
            count += 1;
        }
        Ok(count)
    })
    .await
    .expect("no storage panic")
}

async fn rebootstrap_if_due<S: crate::StorageEngine>(
    sched: &Arc<ResyncScheduler>,
    stor: &S,
    reg: &Arc<HostRegistry>,
    last: &mut Instant,
) -> Result<(), LoadError<S::Error>> {
    if last.elapsed() < REBOOTSTRAP_INTERVAL {
        return Ok(());
    }
    let added = bootstrap(sched.clone(), stor.clone(), reg.clone()).await?;
    counter!(RESYNC_REBOOTSTRAP_PASSES_TOTAL).increment(1);
    if added > 0 {
        counter!(RESYNC_REBOOTSTRAP_ADDED_TOTAL).increment(added as u64);
        tracing::trace!(added, "re-bootstrap pass added entries");
    }
    *last = Instant::now();
    Ok(())
}

/// drive the scheduler until cancelled
///
/// TODO: probably goes on ResyncScheduler impl
pub async fn drive<S: StorageEngine, A: SyncConsumer<Engine = S>, R: Resolve>(
    scheduler: Arc<ResyncScheduler>,
    storage: PrefixedEngine<S>,
    host_registry: Arc<HostRegistry>,
    repo_registry: Arc<RepoRegistry<S, A, R>>,
    dispatch_qps: NonZeroU32,
    concurrency_limit: usize,
    cancel: CancellationToken,
) -> Result<(), ResyncDriveError<S::Error>> {
    bootstrap(scheduler.clone(), storage.clone(), host_registry.clone())
        .await
        .inspect(|n| info!(n, "initial resyncs loaded from db"))?;

    let dispatch_limit: DefaultDirectRateLimiter =
        RateLimiter::direct(Quota::per_second(dispatch_qps));
    let concurrency_limit = Arc::new(Semaphore::new(concurrency_limit));

    let mut prefetch_buffer: HashMap<Option<Hostname>, VecDeque<QueuedResync>> = HashMap::new();

    let mut last_rebootstrap = Instant::now();
    let mut last_cursors_drop = Instant::now();

    while !cancel.is_cancelled() {
        if last_cursors_drop.elapsed() >= CURSOR_DROP_INTERVAL {
            trace!("droping cursors and prefetch buffers to catch stragglers");
            scheduler.drop_cursors();
            prefetch_buffer.clear();
            last_cursors_drop = Instant::now();
        }
        rebootstrap_if_due(&scheduler, &storage, &host_registry, &mut last_rebootstrap).await?;

        // dispatch every deliverable resync. next() skips not-yet-due and host-
        // saturated entries, so everything it returns can go out right now — and
        // undeliverable work never spends the throttle or ticks the counter.
        let now = SystemTime::now();
        while !cancel.is_cancelled()
            && let Some(item) = scheduler.next(now)
        {
            rebootstrap_if_due(&scheduler, &storage, &host_registry, &mut last_rebootstrap).await?;

            let permit = match concurrency_limit.clone().try_acquire_owned() {
                Ok(p) => p,
                Err(TryAcquireError::NoPermits) => {
                    // at limit, do the counter then wait on it
                    counter!(RESYNC_CONCURRENCY_SATURATED_TOTAL).increment(1);
                    let Some(p) = cancel
                        .run(concurrency_limit.clone().acquire_owned())
                        .await
                        .transpose()
                        .expect("semaphore not closed")
                    else {
                        return Ok(()); // cancelled
                    };
                    p
                }
                Err(TryAcquireError::Closed) => panic!("semaphore closed"),
            };

            let mhost = item.host.clone();

            // self-throttle
            if dispatch_limit.check().is_err() {
                trace!("dispatch self-throttled");
                counter!(RESYNC_DISPATCH_THROTTLED_TOTAL).increment(1);
                if cancel.run(dispatch_limit.until_ready()).await.is_none() {
                    return Ok(()); // cancelled
                }
            }

            // try to dispatch
            trace!(did = %item.did, "trying to dispatch scheduled resync");
            let message = RepoMessage::ScheduledResync {
                was_due_at: item.due,
                permit,
            };
            if let Err(e) = repo_registry.try_send(&item.did, message) {
                counter!(RESYNC_DISPATCH_FAILED_TOTAL).increment(1);
                error!(?e, did = %item.did, "resync dispatch failed");

                match e {
                    RepoSendError::Draining(_) => {
                        trace!("resync scheduler exiting bc registry draining");
                        return Ok(());
                    }
                    e @ RepoSendError::EvictionStuck(_) => {
                        return Err(ResyncDriveError::DispatchStuck {
                            did: item.did,
                            source: e,
                        });
                    }
                    e => {
                        // TODO: handle transients by reinserting, (pausing?), continuing
                        // scheduler.insert(item); // restore, will retry
                        // continue;
                        panic!("crashing out for repo send error: {e}");
                    }
                }
            }
            trace!(did = %item.did, "dispatched scheduled resync");

            // yay. update host cursor, get its next thing or unschedule it
            let host_label = if mhost.is_some() { "known" } else { "unknown" };
            counter!(RESYNC_SCHEDULER_DISPATCHED_TOTAL, "host" => host_label).increment(1);
            scheduler.record_cursor(&mhost, item.due, item.did.clone());

            let host_key = mhost.as_ref().map(|h| h.name().clone());

            // get the next task for this host--
            // if there's nothing prefetched, refresh its prefetch buffer first
            if prefetch_buffer.get(&host_key).is_none_or(|b| b.is_empty()) {
                let peek_storage = storage.clone();
                let host_batch = tokio::task::spawn_blocking(move || {
                    item.peek_host_next_batch(PREFETCH_SIZE, &peek_storage)
                })
                .await
                .expect("storage not to panic")?;
                if !host_batch.is_empty() {
                    prefetch_buffer
                        .entry(host_key.clone())
                        .or_default()
                        .extend(host_batch);
                }
            }
            if let Some(next) = prefetch_buffer
                .get_mut(&host_key)
                .and_then(|b| b.pop_front())
            {
                scheduler.insert(next)
            } else {
                trace!(
                    host_label,
                    "no next due for host, unscheduling and cleaning up prefetch"
                );
                prefetch_buffer.remove(&host_key);
                scheduler.unschedule_host(&mhost);
            }
        }
        // broke out of the loop (nothing due or all hosts busy)
        trace!("all due resync dispatched, idling");

        let now = SystemTime::now();
        scheduler.record_state_gauges(now);
        let wait = match scheduler.wait(now) {
            None => IDLE_POLL, // nothing scheduled
            Some(d) if d.is_zero() => {
                // work due but every host is at max concurrency, wait a bit
                counter!(RESYNC_HOST_BACKPRESSURED_TOTAL).increment(1);
                BACKPRESSURE_POLL
            }
            Some(d) => d.min(IDLE_POLL), // soonest entry is still in the future
        };
        if !cancel.sleep(wait).await {
            return Ok(()); // cancelled
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::UNIX_EPOCH;

    use super::*;
    use crate::host::Hostname;

    fn h(s: &str) -> Arc<Host> {
        Arc::new(Host::raw(Hostname::new(s)))
    }

    fn t(ms: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_millis(ms)
    }

    fn qr(host: Arc<Host>, did: &str, due: SystemTime) -> QueuedResync {
        QueuedResync {
            host: Some(host),
            did: Did::raw(did),
            due,
        }
    }

    #[test]
    fn empty_scheduler_has_no_work() {
        let sched = ResyncScheduler::new();
        assert!(sched.is_empty());
        assert_eq!(sched.len(), 0);
        assert!(sched.next(t(0)).is_none());
        assert!(sched.wait(t(0)).is_none());
    }

    #[test]
    fn insert_then_next_returns_due_resync() {
        let sched = ResyncScheduler::new();
        sched.insert(qr(h("a.com"), "did:plc:a", t(10)));
        assert_eq!(sched.len(), 1);

        let got = sched.next(t(20)).expect("a is due");
        assert_eq!(got.host.unwrap().name().as_str(), "a.com");
        assert_eq!(got.did.as_str(), "did:plc:a");
        assert_eq!(got.due, t(10));
        // after pop, the host is no longer scheduled — caller's
        // responsibility to re-insert or unschedule.
        assert!(sched.is_empty());
    }

    #[test]
    fn next_returns_none_when_nothing_due() {
        let sched = ResyncScheduler::new();
        sched.insert(qr(h("a.com"), "did:plc:a", t(100)));
        assert!(sched.next(t(10)).is_none());
        // entry restored on the not-due path; host still scheduled.
        assert_eq!(sched.len(), 1);
    }

    #[test]
    fn next_skips_not_yet_due_to_find_due() {
        let sched = ResyncScheduler::new();
        sched.insert(qr(h("a.com"), "did:plc:a", t(100)));
        sched.insert(qr(h("b.com"), "did:plc:b", t(5)));
        let got = sched.next(t(10)).expect("b is due");
        assert_eq!(got.host.unwrap().name().as_str(), "b.com");
        // a was rotated past but kept scheduled.
        assert_eq!(sched.len(), 1);
    }

    #[test]
    fn next_skips_saturated_host_until_a_permit_frees() {
        let sched = ResyncScheduler::new();
        let sat = h("sat.com");
        let permits = sat.saturate_for_test(); // hold all of sat's concurrency
        assert!(sat.at_max_concurrency());
        sched.insert(qr(sat.clone(), "did:plc:sat", t(0)));

        // due, but the host is saturated: skipped, yet kept scheduled for later.
        assert!(
            sched.next(t(10)).is_none(),
            "saturated host is not delivered"
        );
        assert_eq!(sched.len(), 1, "but it stays scheduled");

        // free the permits → deliverable now.
        drop(permits);
        assert!(!sat.at_max_concurrency());
        let got = sched.next(t(10)).expect("deliverable once a permit frees");
        assert_eq!(got.did.as_str(), "did:plc:sat");
    }

    #[test]
    fn next_prefers_deliverable_host_over_saturated() {
        let sched = ResyncScheduler::new();
        let sat = h("sat.com");
        let free = h("free.com");
        let _permits = sat.saturate_for_test();
        // saturated host inserted first (rr front): next() must rotate past it.
        sched.insert(qr(sat.clone(), "did:plc:sat", t(0)));
        sched.insert(qr(free.clone(), "did:plc:free", t(0)));

        let got = sched.next(t(10)).expect("free host is deliverable");
        assert_eq!(got.host.unwrap().name().as_str(), "free.com");
        assert!(sched.next(t(10)).is_none(), "saturated host still skipped");
    }

    #[test]
    fn next_skips_backing_off_host_until_backoff_passes() {
        let sched = ResyncScheduler::new();
        let bo = h("bo.com");
        // a 429 with no Retry-After: default backoff, anchored to the real
        // clock (observe_response uses its own `now`), so probe with real-
        // clock vantages rather than t()
        bo.observe_response(http::StatusCode::TOO_MANY_REQUESTS, &http::HeaderMap::new());
        sched.insert(qr(bo.clone(), "did:plc:bo", t(0)));

        assert!(
            sched.next(SystemTime::now()).is_none(),
            "backing-off host is not delivered"
        );
        assert_eq!(sched.len(), 1, "but it stays scheduled");

        // from a vantage past the backoff deadline it's deliverable again
        let later = SystemTime::now() + Duration::from_secs(61);
        let got = sched.next(later).expect("deliverable once backoff passes");
        assert_eq!(got.did.as_str(), "did:plc:bo");
    }

    #[test]
    fn next_prefers_deliverable_host_over_backing_off() {
        let sched = ResyncScheduler::new();
        let bo = h("bo.com");
        let free = h("free.com");
        bo.observe_response(http::StatusCode::TOO_MANY_REQUESTS, &http::HeaderMap::new());
        // backing-off host inserted first (rr front): next() must rotate past it.
        sched.insert(qr(bo.clone(), "did:plc:bo", t(0)));
        sched.insert(qr(free.clone(), "did:plc:free", t(0)));

        let now = SystemTime::now();
        let got = sched.next(now).expect("free host is deliverable");
        assert_eq!(got.host.unwrap().name().as_str(), "free.com");
        assert!(sched.next(now).is_none(), "backing-off host still skipped");
    }

    #[test]
    fn round_robin_alternates_due_hosts() {
        let sched = ResyncScheduler::new();
        let a = h("a.com");
        let b = h("b.com");
        sched.insert(qr(a.clone(), "did:plc:a1", t(0)));
        sched.insert(qr(b.clone(), "did:plc:b1", t(0)));

        // a inserted first → at the front of round-robin
        let first = sched.next(t(10)).expect("a due");
        assert_eq!(first.host.unwrap().name().as_str(), "a.com");
        sched.insert(qr(a.clone(), "did:plc:a2", t(0)));

        // after a's re-insert, rr = [b, a]; next picks b
        let second = sched.next(t(10)).expect("b due");
        assert_eq!(second.host.unwrap().name().as_str(), "b.com");
        sched.insert(qr(b.clone(), "did:plc:b2", t(0)));

        // rr = [a, b]; next picks a
        let third = sched.next(t(10)).expect("a due again");
        assert_eq!(third.host.unwrap().name().as_str(), "a.com");
    }

    #[test]
    fn insert_replaces_existing_head() {
        let sched = ResyncScheduler::new();
        let a = h("a.com");
        sched.insert(qr(a.clone(), "did:plc:a1", t(100)));
        sched.insert(qr(a.clone(), "did:plc:a2", t(5)));
        assert_eq!(
            sched.len(),
            1,
            "same host's second insert replaces, not adds"
        );

        let got = sched.next(t(10)).expect("a now due at t=5");
        assert_eq!(got.did.as_str(), "did:plc:a2");
        assert_eq!(got.due, t(5));
    }

    #[test]
    fn insert_same_host_preserves_rr_position() {
        let sched = ResyncScheduler::new();
        let a = h("a.com");
        let b = h("b.com");
        sched.insert(qr(a.clone(), "did:plc:a1", t(10)));
        sched.insert(qr(b.clone(), "did:plc:b", t(10)));
        // re-insert a; if this bumped a to the back of rr, b would pop first.
        sched.insert(qr(a.clone(), "did:plc:a2", t(10)));

        let first = sched.next(t(20)).expect("a still at front");
        assert_eq!(first.host.unwrap().name().as_str(), "a.com");
        assert_eq!(first.did.as_str(), "did:plc:a2");
    }

    #[test]
    fn cursors_entry_is_not_re_added_by_discovery() {
        // regression for the re-dispatch storm: after dispatching an entry, a
        // re-scan of the durable queue (insert_if_absent, as re-bootstrap does)
        // must NOT re-add the still-lingering entry, and it must not come back
        // out of next().
        let sched = ResyncScheduler::new();
        let a = h("a.com");
        sched.insert(qr(a.clone(), "did:plc:a", t(0)));

        let item = sched.next(t(10)).expect("dispatched once");
        sched.record_cursor(&item.host, item.due(), item.did.clone());

        // re-bootstrap re-observes the lingering entry (still in the db queue):
        let re_added = sched.insert_if_absent_or_earlier(qr(a.clone(), "did:plc:a", t(0)));
        assert!(
            !re_added,
            "dispatched-and-lingering entry must not be re-added"
        );
        assert!(sched.next(t(10)).is_none(), "and must not be re-dispatched");

        // a *newer* entry for the same host (e.g. a reschedule) is still work:
        assert!(sched.insert_if_absent_or_earlier(qr(a.clone(), "did:plc:a", t(1))));
        sched.unschedule_host(&Some(a.clone()));

        // and after a cursors drop the safety re-scan may re-add again:
        sched.drop_cursors();
        assert!(sched.insert_if_absent_or_earlier(qr(a, "did:plc:a", t(0))));
    }

    #[test]
    fn unschedule_removes_host() {
        let sched = ResyncScheduler::new();
        let a = h("a.com");
        sched.insert(qr(a.clone(), "did:plc:a", t(10)));
        sched.unschedule_host(&Some(a.clone()));
        assert!(sched.is_empty());
        assert!(sched.next(t(20)).is_none());
    }

    #[test]
    fn unschedule_unknown_host_is_noop() {
        let sched = ResyncScheduler::new();
        sched.insert(qr(h("a.com"), "did:plc:a", t(10)));
        let b = h("b.com");
        sched.unschedule_host(&Some(b.clone()));
        assert_eq!(sched.len(), 1);
    }

    #[test]
    fn wait_returns_none_when_empty() {
        let sched = ResyncScheduler::new();
        assert!(sched.wait(t(0)).is_none());
    }

    #[test]
    fn wait_returns_zero_when_anything_due() {
        let sched = ResyncScheduler::new();
        sched.insert(qr(h("a.com"), "did:plc:a", t(5)));
        sched.insert(qr(h("b.com"), "did:plc:b", t(100)));
        let dur = sched.wait(t(10)).expect("some hosts scheduled");
        assert_eq!(dur, Duration::ZERO);
    }

    #[test]
    fn wait_returns_earliest_when_none_due() {
        let sched = ResyncScheduler::new();
        sched.insert(qr(h("a.com"), "did:plc:a", t(100)));
        sched.insert(qr(h("b.com"), "did:plc:b", t(50)));
        let dur = sched.wait(t(0)).expect("some hosts scheduled");
        assert_eq!(
            dur,
            Duration::from_millis(50 + DISPATCH_LAG_MIN.as_millis() as u64)
        );
    }

    // --- insert_if_absent_or_earlier (slot replacement) ---

    #[test]
    fn insert_earlier_replaces_occupied_slot() {
        let sched = ResyncScheduler::new();
        let a = h("a.com");
        sched.insert(qr(a.clone(), "did:plc:future", t(1000)));
        // a strictly-earlier durable entry for the same host replaces the head
        let replaced = sched.insert_if_absent_or_earlier(qr(a.clone(), "did:plc:sooner", t(100)));
        assert!(replaced, "earlier-due entry replaces the occupied slot");
        assert_eq!(sched.len(), 1, "replacement, not a second slot");

        let got = sched.next(t(2000)).expect("head is due");
        assert_eq!(
            got.did.as_str(),
            "did:plc:sooner",
            "slot now holds the earlier entry"
        );
        assert_eq!(got.due, t(100));
    }

    #[test]
    fn insert_not_earlier_leaves_the_slot() {
        let sched = ResyncScheduler::new();
        let a = h("a.com");
        sched.insert(qr(a.clone(), "did:plc:head", t(100)));
        // equal-due and later-due are both rejected: the slot already holds
        // equal-or-more-urgent work.
        assert!(!sched.insert_if_absent_or_earlier(qr(a.clone(), "did:plc:equal", t(100))));
        assert!(!sched.insert_if_absent_or_earlier(qr(a.clone(), "did:plc:later", t(500))));
        assert_eq!(sched.len(), 1);

        let got = sched.next(t(2000)).expect("head due");
        assert_eq!(got.did.as_str(), "did:plc:head", "original head untouched");
    }

    #[test]
    fn insert_earlier_still_respects_the_cursor() {
        let sched = ResyncScheduler::new();
        let a = h("a.com");
        // dispatch once so a cursor is recorded; the slot is then empty
        sched.insert(qr(a.clone(), "did:plc:head", t(100)));
        let item = sched.next(t(2000)).expect("dispatched");
        sched.record_cursor(&item.host, item.due(), item.did.clone());

        // an entry at-or-before the cursor is one we already dispatched (still
        // lingering in the durable queue): must NOT be re-added even though it's
        // "earlier-due" and the slot is empty.
        let re_added = sched.insert_if_absent_or_earlier(qr(a.clone(), "did:plc:head", t(50)));
        assert!(!re_added, "entry at-or-before the cursor is deduped");
        assert!(sched.next(t(2000)).is_none());
    }

    #[test]
    fn wedged_future_head_is_unwedged_by_due_now_work() {
        // regression: a host slot parked on a not-yet-due head must not starve
        // newly-arrived due-now work for that same host.
        let sched = ResyncScheduler::new();
        let a = h("a.com");

        sched.insert(qr(a.clone(), "did:plc:future", t(1_000_000)));
        assert!(
            sched.next(t(10)).is_none(),
            "future head isn't due -> nothing dispatched, host wedged pre-fix",
        );

        // due-now durable work arrives (as a rebootstrap scan would surface it)
        assert!(sched.insert_if_absent_or_earlier(qr(a.clone(), "did:plc:now", t(0))));

        let got = sched
            .next(t(10))
            .expect("due-now work dispatched after unwedge");
        assert_eq!(got.did.as_str(), "did:plc:now");
    }

    #[test]
    fn insert_earlier_preserves_round_robin_position() {
        let sched = ResyncScheduler::new();
        let a = h("a.com");
        let b = h("b.com");
        // a at the round-robin front with a future head; b due now
        sched.insert(qr(a.clone(), "did:plc:a_future", t(1000)));
        sched.insert(qr(b.clone(), "did:plc:b", t(0)));

        // replacing a's head must keep a's turn, not bump it behind b
        assert!(sched.insert_if_absent_or_earlier(qr(a.clone(), "did:plc:a_now", t(0))));

        let first = sched.next(t(10)).expect("something due");
        assert_eq!(
            first.host.unwrap().name().as_str(),
            "a.com",
            "a kept its round-robin turn through the replace",
        );
    }

    // --- no-host (identity-less) deprioritization ---

    fn qr_nohost(did: &str, due: SystemTime) -> QueuedResync {
        QueuedResync {
            host: None,
            did: Did::raw(did),
            due,
        }
    }

    #[test]
    fn no_host_alone_dispatches() {
        let sched = ResyncScheduler::new();
        sched.insert(qr_nohost("did:plc:nohost", t(5)));
        let got = sched
            .next(t(10))
            .expect("no-host is due as the only option");
        assert!(got.host.is_none());
        assert_eq!(got.did.as_str(), "did:plc:nohost");
    }

    #[test]
    fn hosted_preempts_no_host_when_both_due() {
        let sched = ResyncScheduler::new();
        // insert no-host first so it lands ahead of the hosted entry in
        // round-robin order. exposes the case where the scan visits the
        // no-host before the hosted one.
        sched.insert(qr_nohost("did:plc:nohost", t(5)));
        sched.insert(qr(h("a.com"), "did:plc:a", t(5)));

        let first = sched.next(t(10)).expect("hosted preempts");
        assert_eq!(first.host.unwrap().name().as_str(), "a.com");
    }

    #[test]
    fn no_host_stays_scheduled_across_hosted_dispatch() {
        let sched = ResyncScheduler::new();
        // no-host inserted first → ahead of the hosted entry in rr.
        // after dispatching the hosted one, the no-host must still be
        // findable on the next call (the caller never popped it from
        // its bucket, so the scheduler must keep tracking it).
        sched.insert(qr_nohost("did:plc:nohost", t(5)));
        sched.insert(qr(h("a.com"), "did:plc:a", t(5)));
        assert_eq!(sched.len(), 2);

        let first = sched.next(t(10)).expect("hosted first");
        assert_eq!(first.host.unwrap().name().as_str(), "a.com");

        assert_eq!(
            sched.len(),
            1,
            "no-host must remain scheduled after a hosted dispatch"
        );
        let second = sched.next(t(10)).expect("no-host as fallback");
        assert!(second.host.is_none());
        assert_eq!(second.did.as_str(), "did:plc:nohost");
    }

    #[test]
    fn no_host_deprioritized_through_multi_host_round() {
        // three entries all due: a no-host (inserted first, so at rr
        // front) and two hosted ones. across three next() calls, the
        // no-host must come out *last*, regardless of its rr position.
        let sched = ResyncScheduler::new();
        sched.insert(qr_nohost("did:plc:nohost", t(5)));
        sched.insert(qr(h("a.com"), "did:plc:a", t(5)));
        sched.insert(qr(h("b.com"), "did:plc:b", t(5)));

        let mut popped = Vec::new();
        for _ in 0..3 {
            let r = sched.next(t(10)).expect("due");
            popped.push(match r.host {
                Some(host) => host.name().as_str().to_string(),
                None => "<no-host>".to_string(),
            });
        }
        assert_eq!(
            popped.last().map(String::as_str),
            Some("<no-host>"),
            "no-host must be dispatched last; got order: {popped:?}"
        );
        assert!(popped[..2].iter().any(|s| s == "a.com"));
        assert!(popped[..2].iter().any(|s| s == "b.com"));
    }

    #[test]
    fn no_host_dispatches_when_nothing_hosted_is_due() {
        // hosted entry is in the future; no-host is due now.
        // no-host should dispatch as the fallback rather than blocking.
        let sched = ResyncScheduler::new();
        sched.insert(qr(h("a.com"), "did:plc:a", t(100)));
        sched.insert(qr_nohost("did:plc:nohost", t(5)));
        let got = sched
            .next(t(10))
            .expect("no-host should dispatch when hosted isn't due");
        assert!(got.host.is_none());
        assert_eq!(got.did.as_str(), "did:plc:nohost");
    }

    // --- bootstrap ---

    use crate::storage::DecodeError;
    use crate::storage::engine::mem::MemEngine;
    use crate::{HostRegistry, StorageBatch, StorageEngine};

    fn registry() -> Arc<HostRegistry> {
        HostRegistry::new_default()
    }

    #[tokio::test]
    async fn bootstrap_empty_queue_returns_zero() {
        let eng = MemEngine::new();
        let sched = Arc::new(ResyncScheduler::new());
        let n = bootstrap(sched.clone(), eng.clone(), registry())
            .await
            .expect("ok");
        assert_eq!(n, 0);
        assert!(sched.is_empty());
    }

    /// Build a valid 32-char `did:plc:` from a short test label. Padded with
    /// 'a' to the required length; distinct labels produce distinct DIDs.
    fn plc_did(label: &str) -> Did {
        let mut s = label.to_string();
        while s.len() < 24 {
            s.push('a');
        }
        s.truncate(24);
        Did::new(format!("did:plc:{s}")).expect("valid plc did")
    }

    fn stored(host: Arc<Host>, did: Did, due: SystemTime) -> QueuedResync {
        QueuedResync {
            host: Some(host),
            did,
            due,
        }
    }

    #[tokio::test]
    async fn bootstrap_hydrates_queue_entries() {
        let eng = MemEngine::new();
        let mut b = eng.batch();
        stored(h("example.com"), plc_did("abc"), t(1_000)).store(&mut b);
        b.commit().unwrap();

        let sched = Arc::new(ResyncScheduler::new());
        let n = bootstrap(sched.clone(), eng.clone(), registry())
            .await
            .expect("ok");
        assert_eq!(n, 1);
        assert_eq!(sched.len(), 1);

        let got = sched.next(t(10_000)).expect("a is due");
        assert_eq!(got.host.unwrap().name().as_str(), "example.com");
        assert_eq!(got.did, plc_did("abc"));
        assert_eq!(got.due, t(1_000));
    }

    #[tokio::test]
    async fn bootstrap_hydrates_one_head_per_host() {
        let eng = MemEngine::new();
        let mut b = eng.batch();
        // a.com: three entries, earliest at t=1_000
        stored(h("a.com"), plc_did("a1"), t(3_000)).store(&mut b);
        stored(h("a.com"), plc_did("a2"), t(1_000)).store(&mut b);
        stored(h("a.com"), plc_did("a3"), t(2_000)).store(&mut b);
        // b.com: two entries, earliest at t=500
        stored(h("b.com"), plc_did("b1"), t(500)).store(&mut b);
        stored(h("b.com"), plc_did("b2"), t(4_000)).store(&mut b);
        b.commit().unwrap();

        let sched = Arc::new(ResyncScheduler::new());
        let n = bootstrap(sched.clone(), eng.clone(), registry())
            .await
            .expect("ok");
        assert_eq!(n, 2, "one head per host, not one per queue entry");
        assert_eq!(sched.len(), 2);

        // verify the heads are the earliest-due per host
        let first = sched.next(t(10_000)).expect("a");
        assert_eq!(first.host.unwrap().name().as_str(), "a.com");
        assert_eq!(first.did, plc_did("a2"));
        assert_eq!(first.due, t(1_000));

        let second = sched.next(t(10_000)).expect("b");
        assert_eq!(second.host.unwrap().name().as_str(), "b.com");
        assert_eq!(second.did, plc_did("b1"));
        assert_eq!(second.due, t(500));
    }

    #[tokio::test]
    async fn bootstrap_preserves_byte_sort_order_in_round_robin() {
        let eng = MemEngine::new();
        let mut b = eng.batch();
        // store out of byte-sort order to verify bootstrap doesn't depend
        // on insertion order — it depends on the durable scan order.
        stored(h("c.com"), plc_did("c"), t(100)).store(&mut b);
        stored(h("a.com"), plc_did("a"), t(100)).store(&mut b);
        stored(h("b.com"), plc_did("b"), t(100)).store(&mut b);
        b.commit().unwrap();

        let sched = Arc::new(ResyncScheduler::new());
        bootstrap(sched.clone(), eng.clone(), registry())
            .await
            .expect("ok");

        // hosts come out of next() in bytewise hostname order (the order
        // NextQueuedByHost yielded them, which the scheduler's round-robin
        // preserved as insertion order).
        let mut popped = Vec::new();
        while let Some(r) = sched.next(t(10_000)) {
            popped.push(r.host.unwrap().name().as_str().to_string());
        }
        assert_eq!(popped, vec!["a.com", "b.com", "c.com"]);
    }

    #[tokio::test]
    async fn bootstrap_propagates_queue_error() {
        let eng = MemEngine::new();
        // inject a malformed entry: no NUL separator after the prefix.
        let mut b = eng.batch();
        let mut bad_key = Vec::new();
        bad_key.extend_from_slice(b"rq|");
        bad_key.extend_from_slice(b"bytes_without_a_separator");
        b.put_queue(&bad_key, &[]);
        b.commit().unwrap();

        let sched = Arc::new(ResyncScheduler::new());
        let err = bootstrap(sched.clone(), eng.clone(), registry())
            .await
            .expect_err("bad queue entry");
        assert!(matches!(
            err,
            LoadError::Decode(DecodeError::MissingNullSeparator)
        ));
        assert!(sched.is_empty(), "no partial hydration before the error");
    }
}
