//! per-repo actor: serializes all writes for that repo
//!
//! repo actors own:
//! - persisted repo state, including secondary indexes (resync queue entry)
//! - the task channel receiver (only way to contact the actor)
//! - references to other resources
//!
//! messages (`RepoMessage`) at the mailbox get pre-processed by the Intake, and
//! then sent as tasks, one-at-a-time, to the TaskProcessor. messages include
//! firehose events, resync requests, and anything else that can mutate a repo.
//!
//! this serialization side-steps a lot of the scheduling work that other
//! atproto sync1.1 client implementations have to deal with -- we just happily
//! don't have to deal with TOCTOU, write conflicts, state transition edges, ...
//!
//! this niceness extends to the app using hubble-sync, but *only* if they don't
//! touch any cross-repo shared resources. which... most apps probably need to,
//! at some point. how much this ends up being tricky depends on the app.
//!
//! ```txt
//!                 +- RepoActor (a tokio task) ---------------+
//!                 |                                          |
//! (anchor.try_send)-+>[intake_rx]-┐       lil mpsc mailbox   |
//! finds or wakes  |               v                          |
//! the repo actor  |      ┌-------------┐  overflow policy,   |
//!                 |      | intake loop |  push or coalesce   |
//!                 |      | [vecdeque]  |  messages into      |
//!                 |      └------┬------┘  tasks              |
//!                 |             v                            |
//!                 |     ┌--------------┐  pulls and runs     |
//!                 |     | process loop |  tasks, does repo   |
//!                 |     └-------┬------┘  state mutations    |
//!                 |             v                            |
//!                 | app callback, storage atomic write batch |
//!                 +------------------------------------------+
//! ```
//!
//! i think it would be neat to let the intake loop manage identity refreshes.
//! it would not be allowed to persist the result, but could resolve an app's
//! refresh oneshot channel, and put the pending results into the task queue for
//! the process loop to commit (in order). it could also coalesce/throttle
//! app-driven refreshes.
//!
//! to think about: when desychronized, we probably mostly just miss identity
//! and account events. even maybe account-delete events? with perhaps poor
//! likelihood of having things converge. needs more thought. for one -- if we
//! think a repo is `deactivated` but we receive events for it, we should prob
//! refresh state from upstream.
//!
//! to consider: subscribe to plc event stream as well?

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, mpsc::error::TrySendError, oneshot};
use tokio::task::{JoinHandle, spawn_blocking};
use tokio_util::sync::CancellationToken;
use tracing::trace;

use super::{
    EvictableState, IdentityRefresh, IdentityRefreshOutcome, InitialResolve, InitialResolveOutcome,
    ProcessError, TaskProcessor, WorkingGauge,
};
use crate::firehose::FirehoseEvent;
use crate::identity::{Resolve, ResolvedIdentity};
use crate::storage::StorageEngine;
use crate::storage::repo::{Awoken, Repo};
use crate::{
    Did, Host, HostRegistry, ModerateEvent, ModerateOutcome, PrefixedEngine, SensitiveToken,
    SyncConsumer, UpstreamKind,
};

/// some work the system can request the repo actor to do
///
/// wraps the enum privately, since some variants only be generated within the
/// actor
#[derive(Debug)]
pub enum RepoMessage {
    /// a (raw, unverified) firehose event directed at this repo
    Firehose(FirehoseEvent),
    /// a previously-scheduled resync is now due for this repo
    ScheduledResync {
        was_due_at: SystemTime,
        permit: OwnedSemaphorePermit,
    },
    /// a request to update our resolved identity
    RefreshIdentity {
        // app code can generate this if the db-saved identity is stale
        reply: Option<oneshot::Sender<ResolvedIdentity>>,
    },
    /// a previously-scheduled identity resolution retry or backfill-discovered
    /// resolution is due
    ScheduledIdentityResolve {
        resolve_permit: Option<OwnedSemaphorePermit>,
    },
    /// admin-applied account status transition (or, possibly, app-supplied?)
    Moderate {
        event: Box<ModerateEvent>,
        reply: Option<oneshot::Sender<ModerateOutcome>>,
    },
    /// re-evaluate this repo's sync scope against current policy
    ReconcileScope,
}

/// repo actor's actual work (internal), executed by TaskProcessor
///
/// internal type: public consumers use `RepoMessage to construct the specific
/// tasks they are allowed to construct.
///
/// members of this enum need to be small. box up big payloads. the largest
/// variant drives the in-memory size multiplier of repo task queues.
#[derive(Debug)]
pub(super) enum Task {
    Firehose(FirehoseEvent),
    ScheduledResync {
        was_due_at: SystemTime,
        permit: OwnedSemaphorePermit,
    },
    RefreshIdentity {
        reply: Option<oneshot::Sender<ResolvedIdentity>>,
    },
    Moderate {
        event: ModerateEvent,
        reply: Option<oneshot::Sender<ModerateOutcome>>,
    },
    ReconcileScope,
}

impl Task {
    pub fn name(&self) -> &'static str {
        use crate::firehose::FirehosePayload as F;
        match &self {
            Task::Firehose(f) => match f.payload {
                F::Commit(_) => "Firehose#commit",
                F::Sync(_) => "Firehose#sync",
                F::Account(_) => "Firehose#account",
                F::Identity(_) => "Firehose#identity",
            },
            Task::ScheduledResync { .. } => "ScheduledResync",
            Task::RefreshIdentity { .. } => "RefreshIdentity",
            Task::Moderate { .. } => "Moderate",
            Task::ReconcileScope => "ReconcileScope",
        }
    }
}

/// the way in, for tasks etc.
#[derive(Debug, Clone)]
pub struct RepoAnchor {
    tx: mpsc::Sender<RepoMessage>,
    pub(super) state: Arc<EvictableState>,
}

impl RepoAnchor {
    /// send a task to the actor, if we can
    ///
    /// channel-full means the actor's intake loop can't keep up, which
    /// typically shouldn't happen. consumers might back off and retry.
    ///
    /// TODO: what does closed mean here? should we be reviving the actor?
    pub fn try_send(&self, message: RepoMessage) -> Result<(), TrySendError<RepoMessage>> {
        let res = self.tx.try_send(message).map_err(|e| match e {
            TrySendError::Full(m) => TrySendError::Full(m),
            TrySendError::Closed(m) => TrySendError::Closed(m),
        });
        self.state.registry_note_send();
        res
    }
    /// the actor left the loop, dropping the receiving end of its queue
    ///
    /// ie., it's fully stale and can be evicted from the registry
    pub fn is_evictable(&self) -> bool {
        self.state.registry_check_evictable()
    }
}

#[cfg(test)]
impl RepoAnchor {
    /// an anchor whose channel is already closed -- models an actor that exited
    /// before the cleaner reaped its registry entry, so `try_send` yields
    /// `Closed`.
    pub(crate) fn dead_for_test() -> Self {
        let (tx, rx) = mpsc::channel(1);
        drop(rx); // receiver gone -> tx.try_send returns Closed
        Self {
            tx,
            state: Arc::new(EvictableState::new()),
        }
    }
}

pub struct RepoActorExitGuard {
    did: Did,
    state: Arc<EvictableState>,
    cleaner: mpsc::UnboundedSender<ExitSignal>,
    revived_at: Instant,
}

pub type ExitSignal = (Did, Arc<EvictableState>);

impl Drop for RepoActorExitGuard {
    fn drop(&mut self) {
        // record alive-time before the channel send: even if the cleaner
        // receiver is gone, the histogram still gets the data point.
        metrics::histogram!(crate::metrics::REPO_ACTOR_ALIVE_SECONDS)
            .record(self.revived_at.elapsed().as_secs_f64());
        // unbounded send is sync.
        // errors if the receiver is gone -- nothing to clean up then, ignore
        let _ = self.cleaner.send((self.did.clone(), self.state.clone()));
    }
}

/// intake-owned actor state
struct Intake {
    intake_rx: mpsc::Receiver<RepoMessage>,
    queue: VecDeque<Task>,
    queue_limit: usize,
    last_dequeued: Option<&'static str>,
}

impl Intake {
    fn new(rx: mpsc::Receiver<RepoMessage>, queue_limit: usize) -> Self {
        Self {
            intake_rx: rx,
            queue: VecDeque::new(),
            queue_limit,
            last_dequeued: None,
        }
    }

    fn intake(&mut self, message: RepoMessage) {
        use RepoMessage as M;
        let task = match message {
            M::Firehose(e) => Task::Firehose(e),
            M::ScheduledResync { was_due_at, permit } => {
                // if there's already one in the queue, don't push another
                // (TODO: verify if scheduled resync short-circuits noop)
                if self
                    .queue
                    .iter()
                    .any(|t| matches!(t, Task::ScheduledResync { .. }))
                {
                    trace!("intake: dropping scheduled resync: one already queued");
                    return;
                }
                Task::ScheduledResync { was_due_at, permit }
            }
            M::RefreshIdentity { reply } => Task::RefreshIdentity { reply },
            M::Moderate { event, reply } => Task::Moderate {
                event: *event,
                reply,
            },
            M::ReconcileScope => {
                // coalesce: one pending reconcile is enough
                if self.queue.iter().any(|t| matches!(t, Task::ReconcileScope)) {
                    trace!("intake: dropping reconcile-scope: one already queued");
                    return;
                }
                Task::ReconcileScope
            }
            M::ScheduledIdentityResolve { resolve_permit } => {
                // TODO: right now we assume that this gets handled in wake().
                // this is slightly sketch, and means a ScheduledIdentityResolve
                // on an already-awake actor will implicitly just get dropped.
                // that might be the right thing, but we should be more explicit
                // about what's happening/why with that.
                drop(resolve_permit); // just to be explicit
                return;
            }
        };

        if self.queue.len() >= self.queue_limit {
            let mut by_name = std::collections::HashMap::new();
            for t in &self.queue {
                by_name.entry(t.name()).and_modify(|e| *e += 1).or_insert(1);
            }
            todo!(
                "handle repo queue overflow. last: {:?}.\nby name: {by_name:?}",
                self.last_dequeued
            );
        }
        self.last_dequeued = Some(task.name());
        self.queue.push_back(task);
    }

    fn next_task(&mut self) -> Option<Task> {
        let t = self.queue.pop_front()?;
        self.last_dequeued = Some(t.name());
        Some(t)
    }
}

pub(crate) struct RepoActorContext<S: StorageEngine, A: SyncConsumer<Engine = S>, R: Resolve> {
    pub storage: PrefixedEngine<S>,
    pub hosts: Arc<HostRegistry>,
    pub upstream: Arc<Host>,
    pub upstream_kind: UpstreamKind,
    pub upstream_get_repo_token: Option<SensitiveToken>,
    pub resolver: Arc<R>,
    pub consumer_app: Arc<A>,
    pub cancel: CancellationToken,
    pub big_repo: Arc<Semaphore>,
    pub reactive_permit_wait: Duration,
    pub spill_dir: Arc<Path>,
    pub cleaner: mpsc::UnboundedSender<ExitSignal>,
    pub intake_capacity: usize,
    pub queue_limit: usize,
}

/// the thing! the repo actor! here!
pub struct RepoActor<S: StorageEngine, A: SyncConsumer<Engine = S>, R: Resolve> {
    intake: Intake,
    processor: TaskProcessor<S, A, R>,
    state: Arc<EvictableState>,
    #[expect(dead_code, reason = "just here to implement Drop")]
    exit_guard: RepoActorExitGuard,
}

impl<S: StorageEngine, A: SyncConsumer<Engine = S>, R: Resolve> RepoActor<S, A, R> {
    // like `revive` but with some pre-configured tasks
    pub fn revive(did: Did, ctx: RepoActorContext<S, A, R>) -> (RepoAnchor, JoinHandle<()>) {
        let RepoActorContext {
            storage,
            hosts,
            upstream,
            upstream_kind,
            upstream_get_repo_token,
            resolver,
            consumer_app,
            intake_capacity,
            queue_limit,
            cancel,
            big_repo,
            reactive_permit_wait,
            spill_dir,
            cleaner,
        } = ctx;
        // create the channels right away so we can give back a handle...
        let (tx, rx) = mpsc::channel(intake_capacity);
        let state = Arc::new(EvictableState::new());

        let anchor = RepoAnchor {
            tx: tx.clone(),
            state: state.clone(),
        };

        let now = SystemTime::now(); // system time for db
        let revived_at = Instant::now(); // monotonic for in-mem lifecycle

        let exit_guard = RepoActorExitGuard {
            did: did.clone(),
            state: state.clone(),
            cleaner,
            revived_at,
        };

        // ...but don't block caller (eg., firehose!) with our startup work
        let handle = tokio::spawn(async move {
            let (awoken, (commit_slot, info_slot)) = {
                #[expect(unused, reason = "raii counter guard")]
                let g = WorkingGauge::start("Bootstrap#wake");

                let storage = storage.clone();
                let hosts = hosts.clone();
                let did = did.clone();
                let Ok((awoken, slots)) = spawn_blocking(move || {
                    Repo::wake::<_, A::CommitState, A::InfoState>(&storage, &hosts, did, now)
                })
                .await
                .expect("repo wake task not to panic")
                .inspect_err(|e| tracing::error!(?e, "wake failed")) else {
                    metrics::counter!(crate::metrics::REPO_ACTOR_REVIVES_TOTAL, "wake" => "failed")
                        .increment(1);
                    return;
                };
                (awoken, slots)
            };

            let intake = Intake::new(rx, queue_limit);

            let processor = match awoken {
                Awoken::Resolved(repo) => {
                    metrics::counter!(crate::metrics::REPO_ACTOR_REVIVES_TOTAL, "wake" => "resolved")
                        .increment(1);
                    // TODO: might still use something here for stale not expired
                    // if repo.needs_identity_refresh() {
                    //     intake.intake(Task::ResolveIdentity { reply: None });
                    // }
                    TaskProcessor {
                        storage,
                        resolver,
                        consumer_app,
                        cancel,
                        big_repo,
                        reactive_permit_wait,
                        spill_dir,
                        upstream,
                        upstream_kind,
                        upstream_get_repo_token,
                        repo: Some(Box::new(repo)),
                        commit_slot,
                        info_slot,
                    }
                }
                Awoken::Pending(pending) => {
                    #[expect(unused, reason = "raii counter guard")]
                    let g = WorkingGauge::start("Bootstrap#initialResolve");

                    metrics::counter!(crate::metrics::REPO_ACTOR_REVIVES_TOTAL, "wake" => "pending")
                        .increment(1);
                    let Ok(outcome) = InitialResolve {
                        storage: storage.clone(),
                        resolver: resolver.clone(),
                        pending,
                        upstream,
                        upstream_kind,
                        upstream_get_repo_token,
                        consumer_app: consumer_app.clone(),
                        cancel,
                        big_repo,
                        reactive_permit_wait,
                        spill_dir,
                        now,
                    }
                    .run()
                    .await
                    .inspect_err(|e| tracing::error!(?e, "bootstrap storage error")) else {
                        metrics::counter!(crate::metrics::INITIAL_RESOLVE_OUTCOMES_TOTAL, "outcome" => "storage_error")
                            .increment(1);
                        // TODO: a storage error should be fatal
                        return;
                    };
                    match outcome {
                        InitialResolveOutcome::GetUp(p) => {
                            metrics::counter!(crate::metrics::INITIAL_RESOLVE_OUTCOMES_TOTAL, "outcome" => "get_up")
                                .increment(1);
                            p
                        }
                        InitialResolveOutcome::Snooze => {
                            metrics::counter!(crate::metrics::INITIAL_RESOLVE_OUTCOMES_TOTAL, "outcome" => "snooze")
                                .increment(1);
                            return;
                        }
                        InitialResolveOutcome::Retry => {
                            metrics::counter!(crate::metrics::INITIAL_RESOLVE_OUTCOMES_TOTAL, "outcome" => "retry")
                                .increment(1);
                            return;
                        }
                    }
                }
                Awoken::Refreshing { repo, pending } => {
                    #[expect(unused, reason = "raii counter guard")]
                    let g = WorkingGauge::start("Bootstrap#identityRefresh");

                    metrics::counter!(crate::metrics::REPO_ACTOR_REVIVES_TOTAL, "wake" => "refreshing")
                        .increment(1);
                    let mut processor = TaskProcessor {
                        storage: storage.clone(),
                        resolver: resolver.clone(),
                        consumer_app: consumer_app.clone(),
                        cancel: cancel.clone(),
                        big_repo: big_repo.clone(),
                        reactive_permit_wait,
                        spill_dir: spill_dir.clone(),
                        upstream,
                        upstream_kind,
                        upstream_get_repo_token,
                        repo: Some(Box::new(repo)),
                        commit_slot,
                        info_slot,
                    };
                    // attempt refresh: failure leaves it stale
                    let repo_ref = processor.repo.as_mut().expect("repo present");
                    let outcome = IdentityRefresh {
                        storage: storage.clone(),
                        resolver: resolver.clone(),
                        pending,
                    }
                    .run(repo_ref, now)
                    .await;
                    match outcome {
                        Err(e) => {
                            tracing::error!(?e, "refresh storage error");
                            metrics::counter!(crate::metrics::IDENTITY_REFRESH_OUTCOMES_TOTAL, "outcome" => "storage_error", "trigger" => "wake")
                                .increment(1);
                            // TODO: a storage error should be fatal
                            return;
                        }
                        Ok(IdentityRefreshOutcome::Refreshed) => {
                            metrics::counter!(crate::metrics::IDENTITY_REFRESH_OUTCOMES_TOTAL, "outcome" => "refreshed", "trigger" => "wake")
                                .increment(1);
                        }
                        Ok(IdentityRefreshOutcome::Snoozed) => {
                            metrics::counter!(crate::metrics::IDENTITY_REFRESH_OUTCOMES_TOTAL, "outcome" => "snoozed", "trigger" => "wake")
                                .increment(1);
                        }
                        Ok(IdentityRefreshOutcome::Failed) => {
                            metrics::counter!(crate::metrics::IDENTITY_REFRESH_OUTCOMES_TOTAL, "outcome" => "failed", "trigger" => "wake")
                                .increment(1);
                        }
                    }
                    processor
                }
            };
            let _ = Self {
                intake,
                processor,
                state,
                exit_guard,
            }
            .run()
            .await
            .inspect_err(|e| tracing::error!("maybe should be panic? {e}"));
        });

        (anchor, handle)
    }

    async fn run(mut self) -> Result<(), ProcessError<S::Error>> {
        while self.run_step().await? {}
        while let Some(t) = self.intake.next_task() {
            self.processor.process(t).await?
        }
        Ok(())
    }

    async fn run_step(&mut self) -> Result<bool, ProcessError<S::Error>> {
        let Some(task) = self.intake.queue.pop_front() else {
            // no tasks for the actor
            if !self.make_evictable().await {
                // some work came in, loop back to process it
                return Ok(true);
            }
            // we are actually evictable now, park on intake
            let Some(t) = self.intake.intake_rx.recv().await else {
                // channel closed, time to exit
                return Ok(false);
            };
            // new task came in! enter it in the queue
            self.intake.intake(t);
            // and keep going
            return Ok(true);
        };

        // we have a processing task! set up work on it
        let process_fut = self.processor.process(task);
        tokio::pin!(process_fut);
        loop {
            // keep the intake going while we process
            tokio::select! {
                biased;
                res = &mut process_fut => {
                    // done processing!
                    res?;
                    return Ok(true);
                }
                maybe = self.intake.intake_rx.recv() => { // recv is cancel-safe
                    let Some(t) = maybe else {
                        // channel closed, time to finish up then exit
                        (&mut process_fut).await?;
                        return Ok(false);
                    };
                    // new task arrived! enqueue while still processing
                    self.intake.intake(t);
                }
            }
        }
    }

    /// mark ourselves evictable
    ///
    /// if tasks come in while we're trying to update, we intake and retry
    async fn make_evictable(&mut self) -> bool {
        loop {
            let snapshot = self.state.actor_snapshot();

            if let Ok(t) = self.intake.intake_rx.try_recv() {
                self.intake.intake(t);
                return false; // didn't become evictable (found work to do)
            }

            if self.state.actor_make_evictable_since(snapshot) {
                return true; // became evictable, caller can park on recv
            }
            // else snapshot conflict (a send raced): loop and retry
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;

    use super::*;

    use crate::identity::{HubbleSyncResolver, SigningKey};
    use crate::resync::ResyncData;
    use crate::storage::engine::mem::MemEngine;
    use crate::storage::repo::{RepoIdentity, RepoInfo, SyncStatus};
    use crate::sync_consumer::{AppResult, SimpleSyncConsumer};
    use crate::{AccountStatus, Commit, RepoContext, StorageBatch};

    /// No-op consumer. The actor's `Process::process` currently
    /// `unimplemented!()`s on every Task variant, so this stub never gets
    /// called for real — it exists only to satisfy the actor's generic
    /// bounds.
    pub(super) struct StubConsumer;
    impl SimpleSyncConsumer for StubConsumer {
        type Engine = MemEngine;

        fn apply_commit(
            &self,
            _commit: Commit<'_>,
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

    fn setup() -> (
        PrefixedEngine<MemEngine>,
        Arc<HostRegistry>,
        Arc<HubbleSyncResolver>,
        Arc<StubConsumer>,
    ) {
        let hosts = HostRegistry::new_default();
        (
            MemEngine::new_prefixed(),
            hosts.clone(),
            Arc::new(HubbleSyncResolver::new(
                "@bad-example.com",
                "https://plc.directory",
                hosts,
            )),
            Arc::new(StubConsumer),
        )
    }

    /// Pre-populate storage so the actor's hydrate finds a complete repo
    /// with a fresh resolved identity. Without this, the bootstrap path
    /// fires `ResolveIdentity`, which attempts a real HTTP call we can't
    /// service in unit tests (and would blow past the test's yield deadline).
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

    /// Yield the runtime in a bounded loop until `pred()` is true.
    /// Panics on timeout so a stuck test fails loudly rather than
    /// hanging the runner.
    async fn yield_until<F: FnMut() -> bool>(label: &str, mut pred: F) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !pred() {
            if Instant::now() > deadline {
                panic!("timed out waiting for: {label}");
            }
            tokio::task::yield_now().await;
        }
    }

    fn test_context<S: StorageEngine, A: SyncConsumer<Engine = S>, R: Resolve>(
        storage: PrefixedEngine<S>,
        hosts: Arc<HostRegistry>,
        resolver: Arc<R>,
        consumer: Arc<A>,
        cleaner: mpsc::UnboundedSender<ExitSignal>,
    ) -> RepoActorContext<S, A, R> {
        let upstream = hosts.get("example.com").unwrap();
        RepoActorContext {
            storage,
            hosts,
            upstream,
            upstream_kind: UpstreamKind::PdsDirect,
            upstream_get_repo_token: None,
            resolver,
            consumer_app: consumer,
            cancel: CancellationToken::new(),
            big_repo: Arc::new(Semaphore::new(10)),
            reactive_permit_wait: Duration::from_secs(1),
            spill_dir: Arc::from(PathBuf::from("test-spill")),
            cleaner,
            intake_capacity: 4,
            queue_limit: 16,
        }
    }

    #[tokio::test]
    async fn idle_actor_becomes_evictable() {
        // A freshly-revived actor with no work queued should reach the
        // evictable state on its own once it enters `make_evictable`.
        let (storage, hosts, resolver, consumer) = setup();
        let (cleaner_tx, _cleaner_rx) = mpsc::unbounded_channel();

        let did = plc_did("idle");
        install_resolved_repo(&storage, &hosts, &did);

        let (anchor, _) = RepoActor::revive(
            did,
            test_context(storage, hosts, resolver, consumer, cleaner_tx),
        );

        // Initially not evictable: state is constructed with mismatched
        // (send=0, at=u32::MAX). The actor must run `make_evictable`
        // to claim it.
        assert!(!anchor.is_evictable());

        yield_until("actor to become evictable", || anchor.is_evictable()).await;
    }

    #[tokio::test]
    async fn dropping_anchor_signals_cleaner_with_matching_state() {
        // When the only Sender drops, the actor's intake closes, the
        // run loop exits, the actor struct drops, and the exit guard
        // fires a signal carrying *exactly* the same `did` and
        // `Arc<EvictableState>` the anchor held. The cleaner relies
        // on `Arc::ptr_eq` against these for safe removal.
        let (storage, hosts, resolver, consumer) = setup();
        let (cleaner_tx, mut cleaner_rx) = mpsc::unbounded_channel();

        let did = plc_did("exit");
        install_resolved_repo(&storage, &hosts, &did);

        let (anchor, _) = RepoActor::revive(
            did.clone(),
            test_context(storage, hosts, resolver, consumer, cleaner_tx),
        );
        let state_arc = anchor.state.clone();
        drop(anchor);

        let signal = tokio::time::timeout(Duration::from_secs(2), cleaner_rx.recv())
            .await
            .expect("cleaner signal arrives within timeout")
            .expect("cleaner channel still open");

        assert!(
            signal.0.ptr_eq(&did),
            "cleaner signal carries the same did Arc"
        );
        assert!(
            Arc::ptr_eq(&signal.1, &state_arc),
            "cleaner signal carries the same EvictableState Arc"
        );
    }

    #[tokio::test]
    async fn anchor_send_marks_state_non_evictable() {
        // `RepoAnchor::try_send` does `tx.try_send` then
        // `note_send` — by the time it returns, the state must be
        // non-evictable, even if the actor had previously claimed it.
        let (storage, hosts, resolver, consumer) = setup();
        let (cleaner_tx, _cleaner_rx) = mpsc::unbounded_channel();

        let did = plc_did("send");
        install_resolved_repo(&storage, &hosts, &did);

        let (anchor, _) = RepoActor::revive(
            did.clone(),
            test_context(storage, hosts, resolver, consumer, cleaner_tx),
        );

        yield_until("actor to become evictable", || anchor.is_evictable()).await;
        assert!(anchor.is_evictable());

        anchor
            .try_send(RepoMessage::Moderate {
                event: Box::new(ModerateEvent {
                    subject: did,
                    at: SystemTime::now(),
                    action: None,
                    source: None,
                    message: None,
                    reference: None,
                }),
                reply: None,
            })
            .expect("send to a freshly-evictable actor succeeds");

        assert!(
            !anchor.is_evictable(),
            "state is non-evictable immediately after a successful send"
        );

        // The actor will eventually panic when it processes the
        // `Moderate(None)` task (handlers are `unimplemented!()`); we
        // don't yield further so the spawned task is dropped on
        // runtime shutdown without that panic firing.
    }

    #[test]
    fn intake_coalesces_scheduled_resync_and_releases_the_dropped_permit() {
        // the resync concurrency limit rides as a permit on ScheduledResync.
        // when intake coalesces a second resync into an already-queued one, the
        // dropped message's permit MUST be released -- a leak here silently
        // drains the limit until no resync can ever dispatch again.
        let sem = Arc::new(Semaphore::new(2));
        let held = sem.clone().try_acquire_owned().expect("permit");
        let coalesced = sem.clone().try_acquire_owned().expect("permit");
        assert_eq!(sem.available_permits(), 0, "both permits held to start");

        let (_tx, rx) = mpsc::channel(4);
        let mut intake = Intake::new(rx, 16);

        intake.intake(RepoMessage::ScheduledResync {
            was_due_at: SystemTime::now(),
            permit: held,
        });
        intake.intake(RepoMessage::ScheduledResync {
            was_due_at: SystemTime::now(),
            permit: coalesced,
        });

        // exactly one queued; the coalesced-away message released its permit
        let queued = intake
            .queue
            .iter()
            .filter(|t| matches!(t, Task::ScheduledResync { .. }))
            .count();
        assert_eq!(queued, 1, "second resync coalesced, not queued");
        assert_eq!(
            sem.available_permits(),
            1,
            "the coalesced message's permit is released, the queued one is held",
        );

        // draining + dropping the surviving task releases its permit too
        let task = intake.next_task().expect("the queued resync");
        assert!(matches!(task, Task::ScheduledResync { .. }));
        drop(task);
        assert_eq!(
            sem.available_permits(),
            2,
            "the queued resync's permit releases when its task is dropped",
        );
    }
}
