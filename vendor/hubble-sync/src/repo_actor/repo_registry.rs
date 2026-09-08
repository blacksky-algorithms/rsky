//! did -> actor messaging + actor lifecycle management
//!
//! the registry is who creates (and eventually unloads) `RepoActors`s, ensuring
//! that exactly zero or one actor is alive for a given `did` at any time.
//!
//! eviction: longest-idle entry is asked to unload.

use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use futures_util::stream::{FuturesUnordered, StreamExt};
use metrics::{counter, gauge};
use tokio::sync::Semaphore;
use tokio::sync::mpsc::{self, error::TrySendError};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::actor::{ExitSignal, RepoActor, RepoActorContext, RepoAnchor, RepoMessage};
use crate::identity::Resolve;
use crate::metrics::{
    REPO_REGISTRY_ACTIVE, REPO_REGISTRY_EVICTION_STUCK_TOTAL, REPO_REGISTRY_EVICTIONS_TOTAL,
    REPO_REGISTRY_LOADS_TOTAL,
};
use crate::{Did, Host, HostRegistry, PrefixedEngine, StorageEngine, SyncConfig, SyncConsumer};

#[cfg(test)]
use super::EvictableState;

#[derive(Debug, thiserror::Error)]
pub enum RepoSendError {
    #[error("registry full and could not evict any repos")]
    EvictionStuck(RepoMessage),

    #[error("repo actor mailbox is full (might be starving)")]
    Backpressure(RepoMessage),

    /// returns a message sent after hubble-sync started shutting down
    #[error("hubble-sync is shutting down")]
    Draining(RepoMessage),
}

impl RepoSendError {
    pub fn into_inner(self) -> RepoMessage {
        match self {
            Self::EvictionStuck(m) => m,
            Self::Backpressure(m) => m,
            Self::Draining(m) => m,
        }
    }
}

type Entries = HashMap<Did, Entry>;

struct Entry {
    anchor: RepoAnchor,
    handle: JoinHandle<()>,
    last_touched: Instant,
}

pub trait RepoSender: Send + Sync {
    fn try_send(&self, did: &Did, msg: RepoMessage) -> Result<(), RepoSendError>;
}

pub struct RepoRegistry<S: StorageEngine, A: SyncConsumer<Engine = S>, R: Resolve> {
    storage: PrefixedEngine<S>,
    hosts: Arc<HostRegistry>,
    upstream: Arc<Host>,
    resolver: Arc<R>,
    consumer_app: Arc<A>,
    cancel: CancellationToken,
    big_repo: Arc<Semaphore>,
    reactive_permit_wait: Duration,
    spill_dir: Arc<Path>,
    config: SyncConfig,
    inner: Arc<Mutex<Entries>>,
    cleaner_tx: mpsc::UnboundedSender<ExitSignal>,
    drained: AtomicBool,
}

impl<S: StorageEngine, A: SyncConsumer<Engine = S>, R: Resolve> fmt::Debug
    for RepoRegistry<S, A, R>
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("RepoRegistry")
            .field("config", &self.config)
            .field("repos", &self.len())
            .finish_non_exhaustive()
    }
}

impl<S: StorageEngine, A: SyncConsumer<Engine = S>, R: Resolve> RepoRegistry<S, A, R> {
    pub fn new(
        storage: PrefixedEngine<S>,
        hosts: Arc<HostRegistry>,
        resolver: Arc<R>,
        consumer_app: Arc<A>,
        cancel: CancellationToken,
        config: SyncConfig,
    ) -> Self {
        let inner: Arc<Mutex<Entries>> = Default::default();
        let upstream = hosts
            .get(&config.upstream.hostname)
            .expect("valid upstream config");
        let big_repo = Arc::new(Semaphore::new(config.big_repo_resync_limit));
        let reactive_permit_wait = config.reactive_permit_wait_timeout;
        let spill_dir = Arc::from(config.huge_repo_spill_dir.as_path());

        let (cleaner_tx, cleaner_rx) = mpsc::unbounded_channel();
        tokio::spawn(cleaner_loop(cleaner_rx, inner.clone()));

        Self {
            storage,
            hosts,
            upstream,
            resolver,
            consumer_app,
            cancel,
            big_repo,
            reactive_permit_wait,
            spill_dir,
            config,
            inner,
            cleaner_tx,
            drained: false.into(),
        }
    }

    fn actor_context(&self) -> RepoActorContext<S, A, R> {
        RepoActorContext {
            storage: self.storage.clone(),
            hosts: self.hosts.clone(),
            upstream: self.upstream.clone(),
            upstream_kind: self.config.upstream.kind,
            upstream_get_repo_token: self.config.upstream.get_repo_token.clone(),
            resolver: self.resolver.clone(),
            consumer_app: self.consumer_app.clone(),
            intake_capacity: self.config.repo_intake_capacity,
            queue_limit: self.config.repo_tasks_limit,
            cancel: self.cancel.clone(),
            big_repo: self.big_repo.clone(),
            reactive_permit_wait: self.reactive_permit_wait,
            spill_dir: self.spill_dir.clone(),
            cleaner: self.cleaner_tx.clone(),
        }
    }

    pub fn try_send(&self, did: &Did, mut task: RepoMessage) -> Result<(), RepoSendError> {
        let mut entries = self.inner.lock().expect("registry mutex");
        if self.drained.load(Ordering::SeqCst) {
            return Err(RepoSendError::Draining(task));
        }

        if let Some(entry) = entries.get_mut(did) {
            entry.last_touched = Instant::now();
            match entry.anchor.try_send(task) {
                Ok(()) => {
                    counter!(REPO_REGISTRY_LOADS_TOTAL, "result" => "hit").increment(1);
                    return Ok(());
                }
                Err(TrySendError::Full(t)) => {
                    counter!(REPO_REGISTRY_LOADS_TOTAL, "result" => "hit").increment(1);
                    return Err(RepoSendError::Backpressure(t));
                }
                Err(TrySendError::Closed(t)) => {
                    // actor exited but was still in map: fall through to revive
                    task = t;
                }
            };
        }
        let result = if entries.remove(did).is_some() {
            "closed_revive"
        } else {
            "miss"
        };
        counter!(REPO_REGISTRY_LOADS_TOTAL, "result" => result).increment(1);

        // try to get some room clearing if at cap
        if entries.len() >= self.config.max_repo_actors {
            // evict a batch so we don't re-scan (under the lock) on every insert.
            // scales with the cap: 0->1 in the small-cap tests, ~128 at 32k actors.
            let batch = (self.config.max_repo_actors >> 8).max(1);
            let evicted = Self::evict_lru_batch(&mut entries, batch);
            if evicted == 0 {
                counter!(REPO_REGISTRY_EVICTION_STUCK_TOTAL).increment(1);
                return Err(RepoSendError::EvictionStuck(task));
            }
            counter!(REPO_REGISTRY_EVICTIONS_TOTAL).increment(evicted as u64);
        }

        let (anchor, handle) = RepoActor::revive(did.clone(), self.actor_context());
        entries.insert(
            did.clone(),
            Entry {
                anchor: anchor.clone(),
                handle,
                last_touched: Instant::now(),
            },
        );
        gauge!(REPO_REGISTRY_ACTIVE).set(entries.len() as f64);

        // TODO might be our problem if we can't send to a freshly-revived actor
        match anchor.try_send(task) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(t)) => Err(RepoSendError::Backpressure(t)),
            Err(TrySendError::Closed(t)) => {
                tracing::info!(task = ?t, "dropping task: woken actor closed");
                Ok(())
            }
        }
    }

    pub fn len(&self) -> usize {
        self.inner.lock().expect("registry mutex").len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.lock().expect("registry mutex").is_empty()
    }

    pub fn big_repo_permits_in_use(&self) -> usize {
        self.config
            .big_repo_resync_limit
            .saturating_sub(self.big_repo.available_permits())
    }

    pub fn big_repo_permit_limit(&self) -> usize {
        self.config.big_repo_resync_limit
    }

    /// evict up to `count` of the longest-idle evictable actors in one pass.
    ///
    /// batching amortizes the O(n) scan across `count` evictions: at cap we drop
    /// a whole batch, so the next `count`-1 inserts skip the scan (and its
    /// lock-held O(n) walk) entirely. returns the number actually evicted (0 =
    /// none were evictable).
    fn evict_lru_batch(entries: &mut Entries, count: usize) -> usize {
        // one O(n) pass; the single-evict path already cloned every evictable
        // did, so per-scan cost is unchanged -- we just scan `count`x less often.
        let mut evictable: Vec<(Instant, Did)> = entries
            .iter()
            .filter(|(_, e)| e.anchor.is_evictable())
            .map(|(did, e)| (e.last_touched, did.clone()))
            .collect();

        let n = count.min(evictable.len());
        if n == 0 {
            return 0;
        }
        // partial-select the `n` oldest to the front -- O(len), no full sort
        if n < evictable.len() {
            evictable.select_nth_unstable_by_key(n - 1, |e| e.0);
        }
        for (_, did) in &evictable[..n] {
            entries
                .remove(did)
                .expect("just-selected evictable actor already gone??");
        }
        n
    }

    /// close every actor's mailbox, drain, and exit
    ///
    /// this is ONLY safe to call once, during shutdown
    pub async fn drain(&self, timeout: Duration) {
        let drained_before = self.drained.swap(true, Ordering::SeqCst);
        assert!(!drained_before, "BUG: repo registry already drained");

        let entries = std::mem::take(&mut *self.inner.lock().expect("registry mutex"));

        let mut handles: FuturesUnordered<JoinHandle<()>> =
            entries.into_values().map(|entry| entry.handle).collect();

        if handles.is_empty() {
            return;
        }

        let count = handles.len();
        let now = Instant::now();
        let deadline = (now + timeout).into();
        tracing::info!(count, deadline_in = ?timeout, "draining repo actors");

        let mut drained = 0;

        loop {
            match tokio::time::timeout_at(deadline, handles.next()).await {
                Ok(Some(res)) => {
                    drained += 1;
                    gauge!(REPO_REGISTRY_ACTIVE).increment(-1.);
                    if let Err(join_err) = res {
                        tracing::warn!(drained, %join_err, "repo actor join-errored");
                    } else {
                        tracing::trace!(drained, "repo actor drained");
                    }
                }
                Ok(None) => {
                    gauge!(REPO_REGISTRY_ACTIVE).set(0.);
                    tracing::info!(count, "all repo actors drained.");
                    return;
                }
                Err(_elapsed) => break,
            }
        }

        tracing::warn!(
            count,
            drained,
            remaining = count - drained,
            ?timeout,
            "repo actor drain timed out, aborting remaining",
        );

        for handle in handles {
            handle.abort();
        }
        gauge!(REPO_REGISTRY_ACTIVE).set(0.);
    }
}

impl<S, A, R> RepoSender for RepoRegistry<S, A, R>
where
    S: StorageEngine,
    A: SyncConsumer<Engine = S>,
    R: Resolve,
{
    fn try_send(&self, did: &Did, message: RepoMessage) -> Result<(), RepoSendError> {
        RepoRegistry::try_send(self, did, message)
    }
}

async fn cleaner_loop(mut rx: mpsc::UnboundedReceiver<ExitSignal>, entries: Arc<Mutex<Entries>>) {
    while let Some((did, state)) = rx.recv().await {
        // got a did to clean up
        let mut entries = entries.lock().expect("registry mutex");

        let Some(entry) = entries.get(&did) else {
            // already removed from map (lru evicted)
            continue;
        };

        // make sure the found entry is the one we're cleaning, not a freshly-
        // revived new copy
        if Arc::ptr_eq(&entry.anchor.state, &state) {
            entries.remove(&did);
            gauge!(REPO_REGISTRY_ACTIVE).set(entries.len() as f64);
        }
    }
}

#[cfg(test)]
impl<S: StorageEngine, A: SyncConsumer<Engine = S>, R: Resolve> RepoRegistry<S, A, R> {
    /// Test-only: revive an actor without firing a `try_send` against
    /// it. `try_send`'s `note_send` would put the freshly-revived
    /// actor in a non-evictable state, defeating tests that need to
    /// observe an evictable LRU candidate.
    pub(super) fn revive_for_test(&self, did: Did) {
        let mut entries = self.inner.lock().expect("registry mutex");
        let (anchor, handle) = RepoActor::revive(did.clone(), self.actor_context());
        entries.insert(
            did,
            Entry {
                anchor,
                handle,
                last_touched: Instant::now(),
            },
        );
    }

    pub(super) fn contains(&self, did: &Did) -> bool {
        self.inner.lock().expect("registry mutex").contains_key(did)
    }

    pub(super) fn is_entry_evictable(&self, did: &Did) -> bool {
        self.inner
            .lock()
            .expect("registry mutex")
            .get(did)
            .map(|e| e.anchor.is_evictable())
            .unwrap_or(false)
    }

    /// Insert an entry whose actor has already exited (closed channel), as if
    /// the actor exited but the cleaner hasn't reaped it yet. Returns the dead
    /// entry's `EvictableState` so a test can tell incarnations apart.
    pub(super) fn insert_dead_for_test(&self, did: Did) -> Arc<EvictableState> {
        let anchor = RepoAnchor::dead_for_test();
        let state = anchor.state.clone();
        let handle = tokio::spawn(async {});
        self.inner.lock().expect("registry mutex").insert(
            did,
            Entry {
                anchor,
                handle,
                last_touched: Instant::now(),
            },
        );
        state
    }

    pub(super) fn entry_state(&self, did: &Did) -> Option<Arc<EvictableState>> {
        self.inner
            .lock()
            .expect("registry mutex")
            .get(did)
            .map(|e| e.anchor.state.clone())
    }

    pub(super) fn send_exit_signal_for_test(&self, did: Did, state: Arc<EvictableState>) {
        self.cleaner_tx.send((did, state)).expect("cleaner alive");
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use tokio_util::sync::CancellationToken;

    use super::*;

    use crate::StorageBatch;
    use crate::commit::Commit;
    use crate::identity::{HubbleSyncResolver, SigningKey};
    use crate::resync::ResyncData;
    use crate::storage::engine::mem::MemEngine;
    use crate::storage::repo::{RepoIdentity, RepoInfo, SyncStatus};
    use crate::sync_consumer::{AppResult, SimpleSyncConsumer};
    use crate::{AccountStatus, RepoContext};

    /// No-op consumer; the actor's process handlers are
    /// `unimplemented!()` so the consumer is never actually called.
    struct StubConsumer;
    impl SimpleSyncConsumer for StubConsumer {
        type Engine = MemEngine;

        fn apply_commit(
            &self,
            _commit: Commit,
            _repo: &RepoContext,
            _batch: &mut <Self::Engine as StorageEngine>::Batch,
        ) -> AppResult<Self::Engine, ()> {
            Ok(())
        }
        fn apply_resync(
            &self,
            _data: &mut ResyncData,
            _repo: &RepoContext,
            _batch: &mut <Self::Engine as StorageEngine>::Batch,
        ) -> AppResult<Self::Engine, ()> {
            Ok(())
        }
        fn apply_status(
            &self,
            _prev_status: &AccountStatus,
            _repo: &RepoContext,
            _batch: &mut <Self::Engine as StorageEngine>::Batch,
        ) -> AppResult<Self::Engine, ()> {
            Ok(())
        }
    }

    fn setup(
        max_repo_actors: usize,
    ) -> (
        RepoRegistry<MemEngine, StubConsumer, HubbleSyncResolver>,
        PrefixedEngine<MemEngine>,
        Arc<HostRegistry>,
    ) {
        let storage = MemEngine::new_prefixed();
        let hosts = HostRegistry::new_default();
        let plc_url = "https://plc.directory";
        let resolver = Arc::new(HubbleSyncResolver::new(
            "@bad-example.com",
            plc_url,
            hosts.clone(),
        ));
        let consumer = Arc::new(StubConsumer);
        let config = SyncConfig {
            max_repo_actors,
            repo_tasks_limit: 16,
            repo_intake_capacity: 4,
            plc_url: plc_url.to_string(),
            ..Default::default()
        };
        let cancel = CancellationToken::new();
        let reg = RepoRegistry::new(
            storage.clone(),
            hosts.clone(),
            resolver,
            consumer,
            cancel,
            config,
        );
        (reg, storage, hosts)
    }

    /// Pre-populate storage so the actor's hydrate finds a complete repo
    /// with a fresh resolved identity. Without this, the bootstrap path
    /// fires `ResolveIdentity`, which attempts a real HTTP call we can't
    /// service in unit tests.
    fn install_resolved_repo(storage: &PrefixedEngine<MemEngine>, hosts: &HostRegistry, did: &Did) {
        let pds = hosts.get("pds.example.com").expect("interned");
        let now = SystemTime::now();
        let info = RepoInfo {
            upstream_status: AccountStatus::Active,
            moderation: None,
            sync_status: SyncStatus::Synchronized,
            identity: RepoIdentity {
                pds_host: pds,
                signing_key: SigningKey::raw(vec![0u8; 32]),
                supposed_handle: Some("alice.example".to_string()),
                resolved_at: now,
            },
            first_seen_at: now,
            resyncs: 0,
            pds_changes: 0,
            handle_changes: 0,
            last_resync: None,
            first_resync_needs_permit: None,
        };
        let mut b = storage.batch();
        info.store(did, &mut b);
        b.commit().unwrap();
    }

    /// Build a valid 32-char `did:plc:` from a short test label.
    fn plc_did(label: &str) -> Did {
        let mut s = label.to_string();
        while s.len() < 24 {
            s.push('a');
        }
        s.truncate(24);
        Did::new(format!("did:plc:{s}")).expect("valid plc did")
    }

    async fn yield_until<F: FnMut() -> bool>(label: &str, mut pred: F) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !pred() {
            if Instant::now() > deadline {
                panic!("timed out waiting for: {label}");
            }
            tokio::task::yield_now().await;
        }
    }

    fn unmod(did: &Did) -> RepoMessage {
        RepoMessage::Moderate {
            event: Box::new(crate::ModerateEvent {
                at: SystemTime::now(),
                subject: did.clone(),
                action: None,
                source: None,
                message: None,
                reference: None,
            }),
            reply: None,
        }
    }

    #[tokio::test]
    async fn try_send_revives_new_actor_on_first_call() {
        let (reg, _storage, _hosts) = setup(8);
        let did = plc_did("a");

        assert!(!reg.contains(&did));
        reg.try_send(&did, unmod(&did)).expect("first send");
        assert!(reg.contains(&did));
        assert_eq!(reg.len(), 1);

        // Don't yield: the actor will eventually try to process the
        // Moderate task and panic at `unimplemented!()`. Letting the
        // test finish before scheduling avoids the noise.
    }

    #[tokio::test]
    async fn try_send_reuses_existing_entry() {
        let (reg, _storage, _hosts) = setup(8);
        let did = plc_did("reuse");

        reg.try_send(&did, unmod(&did)).expect("first");
        reg.try_send(&did, unmod(&did)).expect("second");

        assert_eq!(reg.len(), 1);
    }

    #[tokio::test]
    async fn eviction_at_cap_picks_evictable_lru() {
        // max=2. Revive A, then B (without sending so they reach
        // evictable). Once both are evictable, send for C: the LRU
        // (A) must be removed and C inserted, leaving {B, C}.
        let (reg, storage, hosts) = setup(2);
        let a = plc_did("a");
        let b = plc_did("b");
        let c = plc_did("c");

        // Pre-resolve A and B so their actors don't fire bootstrap-resolve
        // (which would block on HTTP and miss the yield deadline).
        install_resolved_repo(&storage, &hosts, &a);
        install_resolved_repo(&storage, &hosts, &b);

        reg.revive_for_test(a.clone());
        // Tiny sleep to make A's last_touched strictly older than B's.
        tokio::time::sleep(Duration::from_millis(2)).await;
        reg.revive_for_test(b.clone());

        yield_until("A and B both evictable", || {
            reg.is_entry_evictable(&a) && reg.is_entry_evictable(&b)
        })
        .await;

        reg.try_send(&c.clone(), unmod(&a))
            .expect("c send triggers eviction");

        assert_eq!(reg.len(), 2);
        assert!(!reg.contains(&a), "A (LRU) should have been evicted");
        assert!(reg.contains(&b));
        assert!(reg.contains(&c));
    }

    #[tokio::test]
    async fn eviction_stuck_when_nothing_is_evictable() {
        // Fill the cap with sends (each one fires `note_send`, leaving
        // its actor non-evictable). Don't yield, so the actors stay in
        // the non-evictable state. A new try_send must surface
        // `EvictionStuck`.
        let (reg, _storage, _hosts) = setup(2);
        let a = plc_did("a");
        let b = plc_did("b");
        let c = plc_did("c");

        reg.try_send(&a, unmod(&a)).expect("a");
        reg.try_send(&b, unmod(&b)).expect("b");

        let err = reg
            .try_send(&c, unmod(&c))
            .expect_err("at-cap with no evictable candidates must error");
        assert!(
            matches!(err, RepoSendError::EvictionStuck(_)),
            "expected EvictionStuck, got {err:?}"
        );
    }

    #[tokio::test]
    async fn try_send_over_a_dead_entry_revives_without_surfacing_closed() {
        let (reg, _storage, _hosts) = setup(8);
        let did = plc_did("dead");

        // an actor that exited but the cleaner hasn't reaped yet: present in the
        // map, channel closed.
        let dead_state = reg.insert_dead_for_test(did.clone());
        assert!(reg.contains(&did));

        // the send must NOT surface Closed to the caller -- under the same lock
        // it reaps the dead entry and revives a fresh actor in its place.
        reg.try_send(&did, unmod(&did))
            .expect("dead entry is transparently revived, not surfaced as Closed");

        assert_eq!(
            reg.len(),
            1,
            "reaped one, revived one: still exactly one entry"
        );
        let live_state = reg.entry_state(&did).expect("entry present");
        assert!(
            !Arc::ptr_eq(&live_state, &dead_state),
            "the entry is a fresh incarnation, not the dead one",
        );
        // don't yield: the fresh actor would process the Moderate task and panic
        // at unimplemented!()
    }

    #[tokio::test]
    async fn cleaner_ignores_a_stale_exit_signal_for_a_replaced_entry() {
        // what an inline revive leaves behind: `did`'s entry has been replaced
        // with a new incarnation (different EvictableState), and then the OLD
        // incarnation's ExitSignal reaches the cleaner. the state-match must
        // make it a no-op -- otherwise the cleaner would reap the live entry.
        let (reg, _storage, _hosts) = setup(8);
        let did = plc_did("replaced");
        let barrier = plc_did("barrier");

        let old_state = reg.insert_dead_for_test(did.clone());
        let new_state = reg.insert_dead_for_test(did.clone()); // replaces the entry
        assert!(
            !Arc::ptr_eq(&old_state, &new_state),
            "distinct incarnations"
        );
        let barrier_state = reg.insert_dead_for_test(barrier.clone());

        // FIFO into the cleaner: the stale signal for the OLD incarnation (must
        // no-op on state mismatch), then a matching signal for the barrier (must
        // reap it).
        reg.send_exit_signal_for_test(did.clone(), old_state);
        reg.send_exit_signal_for_test(barrier.clone(), barrier_state);

        // once the barrier is reaped, the cleaner has processed both in order.
        yield_until("cleaner reaped the barrier", || !reg.contains(&barrier)).await;

        assert!(
            reg.contains(&did),
            "a stale exit signal (old incarnation) must not reap the replaced entry",
        );
    }
}
