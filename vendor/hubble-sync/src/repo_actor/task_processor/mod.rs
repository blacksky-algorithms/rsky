mod event_validation;
mod moderate_outcome;

pub use moderate_outcome::ModerateOutcome;

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::firehose::{FirehoseCommit, FirehoseSync};
use jacquard_api::com_atproto::sync::subscribe_repos::{Account as FirehoseAccountEvent, Identity};
use metrics::{counter, histogram};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, oneshot};
use tokio::task::spawn_blocking;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, trace, warn};

use super::{Task, WorkingGauge};
use crate::Commit;
use crate::firehose::{FirehoseEvent, FirehosePayload, Seq};
use crate::host::{HostRequestError, LimitWaited, ResolvedHost};
use crate::identity::{ResolutionError, Resolve, ResolvedIdentity, SigningKey, Validity};
use crate::metrics::{
    ACCOUNT_OUTCOMES_TOTAL, COMMIT_OUTCOMES_TOTAL, IDENTITY_REFRESH_OUTCOMES_TOTAL, RESYNC_COUNT,
    RESYNC_DURATION_SECONDS, RESYNC_OUTCOMES_TOTAL, RESYNC_PHASE_SECONDS, RESYNC_SIZE,
    SYNC_OUTCOMES_TOTAL,
};
use crate::resync::{
    BigRepoPermits, ResyncData, ResyncError, Resyncable, TransientResyncError, load_repo,
};
use crate::storage::engine::{PrefixedBatch, PrefixedEngine, StorageBatch, StorageEngine};
use crate::storage::repo::{
    AccountStatus, AccountStatusEvent, AccountStatusUpstream, DesyncReason, Desynchronized, Repo,
    ResyncInfo, SyncStatus,
};
use crate::{
    CancelExt, CommitObject, ConsumerAppError, DaslCid, Did, Host, ModerateEvent, RepoSlots,
    SensitiveToken, StorageError, SyncConsumer, Tid, UpstreamKind,
};
use event_validation::validate_commit_signature;

const STREAM_CAR_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, thiserror::Error)]
pub enum ProcessError<E: StorageError> {
    #[error("storage: {0}")]
    Storage(#[source] E),
    #[error("resync: {0}")]
    Resync(#[from] ResyncError),
    #[error("consumer app: {0}")]
    ConsumerApp(#[source] ConsumerAppError<E>),
}

type PEResult<S, T> = Result<T, ProcessError<<S as StorageEngine>::Error>>;

/// a lil borrowed shell for consumer app context
#[derive(Debug)]
pub struct RepoContext<'a>(&'a Repo);

impl<'a> RepoContext<'a> {
    pub(crate) fn new(repo: &'a Repo) -> Self {
        RepoContext(repo)
    }
    pub fn did(&self) -> &Did {
        self.0.did()
    }
    pub fn status(&self) -> AccountStatus {
        self.0.info().account_status()
    }
    pub fn pds(&self) -> &Arc<Host> {
        &self.0.info().identity.pds_host
    }
    /// the repo's current signing key
    pub fn signing_key(&self) -> &SigningKey {
        &self.0.info().identity.signing_key
    }
    pub fn is_active(&self) -> bool {
        self.status().is_active()
    }
    pub fn is_gone(&self) -> bool {
        self.status().is_gone()
    }
    pub fn rev(&self) -> Option<Tid> {
        self.0.sync_state().map(|s| s.rev)
    }
    /// the current MST root (commit object's `data` cid), if synced
    pub fn data(&self) -> Option<DaslCid> {
        self.0.sync_state().map(|s| s.prev)
    }
    /// just from the did doc, not bidirectionally verified
    pub fn unverified_handle(&self) -> Option<&str> {
        self.0.info().identity.supposed_handle.as_deref()
    }
    pub fn first_seen_at(&self) -> SystemTime {
        self.0.info().first_seen_at
    }
    pub fn resyncs(&self) -> u32 {
        self.0.info().resyncs
    }
    pub fn pds_changes(&self) -> u32 {
        self.0.info().pds_changes
    }
    pub fn handle_changes(&self) -> u32 {
        self.0.info().handle_changes
    }
    pub fn last_resync(&self) -> Option<&ResyncInfo> {
        self.0.info().last_resync.as_ref()
    }
    pub fn commits_seen(&self) -> Option<u32> {
        self.0.sync_state().map(|s| s.commits)
    }
    pub fn desync(&self) -> Option<&Desynchronized> {
        self.0.current_desync()
    }
    pub fn locally_moderated(&self) -> bool {
        self.0.info().moderation.is_some()
    }
    /// full sync status (see [`Self::desync`] for just the desync details)
    pub fn sync_status(&self) -> &SyncStatus {
        &self.0.info().sync_status
    }
    /// account status as reported upstream (usually you want [`Self::status`])
    pub fn upstream_status(&self) -> &AccountStatus {
        &self.0.info().upstream_status
    }
}

#[derive(Debug)]
pub struct ResyncContext<'a> {
    repo: &'a Repo,
    upstream: &'a Host,
    upstream_kind: UpstreamKind,
    upstream_get_repo_token: Option<&'a SensitiveToken>,
    resolved: Option<Arc<Host>>,
    cancel: &'a CancellationToken,
    permit: Option<OwnedSemaphorePermit>,
    big_permits: BigRepoPermits,
    reactive_permit_wait: Duration,
    spill_dir: &'a Path,
}
impl<'a> ResyncContext<'a> {
    pub fn did(&self) -> &Did {
        self.repo.did()
    }
    pub fn status(&self) -> AccountStatus {
        self.repo.info().account_status()
    }
    pub fn pds(&self) -> &'a Arc<Host> {
        &self.repo.info().identity.pds_host
    }
    pub fn upstream(&self) -> &Host {
        self.upstream
    }
    pub fn is_active(&self) -> bool {
        self.repo.info().is_active()
    }
    pub fn rev(&self) -> Option<Tid> {
        self.repo.sync_state().as_ref().map(|s| s.rev)
    }
    /// note that a resync needed a big-repo permit
    ///
    /// this is for custom resync data, for a first-seen repo that fails its
    /// first resync attempt, to let hubble-sync know that it should *pre-
    /// acquire* a permit for the retry.
    pub async fn acquire_big_permit(&self) -> OwnedSemaphorePermit {
        self.big_permits.acquire_owned().await
    }
    pub async fn load_default(&mut self) -> Result<ResyncData, ResyncError> {
        let (target, token) = match self.upstream_kind {
            UpstreamKind::Relay
                if matches!(
                    &self.repo.info().sync_status,
                    SyncStatus::Desynchronized(d) if matches!(d.reason, DesyncReason::FirstSeen)
                ) =>
            {
                // for a first-seen with relay upstream (relay backfill), we
                // *don't* make the getRepo call directly to the relay and
                // follow that redirect.
                // https://bsky.app/profile/bnewbold.net/post/3mrfa6qr2jk2d
                (self.pds().as_ref(), None)
            }
            UpstreamKind::Relay => (self.upstream, None),
            UpstreamKind::PdsDirect => (self.upstream, self.upstream_get_repo_token),
        };

        // set up the initial request
        let request_start = Instant::now();
        let (reader, extensions) =
            target
                .get_repo(self.repo.did(), token)
                .await
                .map_err(|e| match &e {
                    HostRequestError::Cancelled => ResyncError::Cancelled,
                    HostRequestError::Xrpc { error, .. } => match error.error.as_ref() {
                        "RepoDeactivated" => ResyncError::Status(AccountStatus::Deactivated),
                        "RepoSuspended" => ResyncError::Status(AccountStatus::Suspended),
                        "RepoTakendown" => ResyncError::Status(AccountStatus::Takendown),
                        "RepoNotFound" => ResyncError::RepoMissing,
                        "NotFound" => ResyncError::RepoMissing, // bsky pds bug
                        other => {
                            info!(
                            ident_pds_host = %self.pds().name(),
                            fetched_via = %target.name(),
                            error = %other,
                            message = ?error.message,
                            "load_default getRepo: unrecognized xrpc error");
                            TransientResyncError::Request(e.to_string()).into()
                        }
                    },
                    err => {
                        debug!(
                        ident_pds_host = %self.pds().name(),
                        fetched_via = %target.name(),
                        ?err,
                        "load_default getRepo errored");
                        TransientResyncError::Request(e.to_string()).into()
                    }
                })?;

        // limit_wait: queued at host limits (all hops); request: net wire time.
        let limit_waited = extensions
            .get::<LimitWaited>()
            .map(|w| w.0)
            .unwrap_or_default();
        histogram!(RESYNC_PHASE_SECONDS, "phase" => "limit_wait")
            .record(limit_waited.as_secs_f64());
        histogram!(RESYNC_PHASE_SECONDS, "phase" => "request").record(
            request_start
                .elapsed()
                .saturating_sub(limit_waited)
                .as_secs_f64(),
        );

        self.resolved = extensions.get::<ResolvedHost>().map(|r| r.0.clone());

        // grab our per-acquired permit, if we have one
        let permit = self.permit.take();

        // actually load the repo
        let drain_start = Instant::now();
        let loaded = self
            .cancel
            .timeout(
                STREAM_CAR_TIMEOUT,
                load_repo(
                    reader,
                    &self.big_permits,
                    permit,
                    self.reactive_permit_wait,
                    self.spill_dir,
                ),
            )
            .await
            .ok_or(ResyncError::Cancelled)?
            .map_err(TransientResyncError::Timeout)?;
        histogram!(RESYNC_PHASE_SECONDS, "phase" => "drain")
            .record(drain_start.elapsed().as_secs_f64());
        loaded
    }
}

pub(super) struct TaskProcessor<S: StorageEngine, A: SyncConsumer<Engine = S>, R: Resolve> {
    pub(super) storage: PrefixedEngine<S>,
    pub(super) resolver: Arc<R>,
    pub(super) consumer_app: Arc<A>,
    pub(super) cancel: CancellationToken,
    pub(super) big_repo: Arc<Semaphore>,
    pub(super) reactive_permit_wait: Duration,
    pub(super) spill_dir: Arc<Path>,
    pub(super) upstream: Arc<Host>,
    pub(super) upstream_kind: UpstreamKind,
    /// bearer token for getRepo requests to a pds-direct upstream
    pub(super) upstream_get_repo_token: Option<SensitiveToken>,
    /// Option so it can be moved (`take`n) temporarily into a `spawn_blocking`
    /// closure without cloning.
    ///
    /// just clone it in most cases, which is safe.
    ///
    /// for high-rate events (firehose commits), take care when `take`ing to
    /// ensure that it's *always* put back as Some(repo), even on error paths.
    ///
    /// boxed to keep the tokio task small on the stack (many actors)
    pub(super) repo: Option<Box<Repo>>,
    pub(super) commit_slot: Option<A::CommitState>,
    pub(super) info_slot: Option<A::InfoState>,
}

impl<S: StorageEngine, A: SyncConsumer<Engine = S>, R: Resolve> TaskProcessor<S, A, R> {
    /// process one repo task
    ///
    /// tasks are not necessarily 1:1: with messages sent to the actor: the intake
    /// may coallesce, inject, or drop messages and tasks
    #[tracing::instrument(skip_all, fields(
        did = %self.repo().did(),
        task = task.name(),
        seq = tracing::field::Empty,
    ))]
    pub(super) async fn process(&mut self, task: Task) -> PEResult<S, ()> {
        #[expect(unused, reason = "raii guard for count")]
        let g = WorkingGauge::start(task.name());

        let res = match task {
            Task::Firehose(event) => self.process_firehose(event).await,
            Task::ScheduledResync { was_due_at, permit } => {
                self.process_scheduled_resync(was_due_at, permit).await
            }
            Task::RefreshIdentity { reply } => self.process_refresh(reply).await,
            Task::Moderate { event, reply } => self.process_moderate(event, reply).await,
            Task::ReconcileScope => self.process_reconcile_scope().await,
        };
        if self.repo.is_none() {
            error!("process handler failed to restore Some(repo)");
        }
        res
    }

    async fn process_firehose(&mut self, event: FirehoseEvent) -> PEResult<S, ()> {
        tracing::Span::current().record("seq", event.seq.as_u64());
        match event.payload {
            FirehosePayload::Commit(commit) => self.process_commit(*commit).await,
            FirehosePayload::Sync(sync) => self.process_sync(*sync).await,
            FirehosePayload::Account(account) => {
                self.process_account(*account, event.host, event.seq).await
            }
            FirehosePayload::Identity(identity) => self.process_identity(*identity).await,
        }
    }

    async fn process_commit(&mut self, commit: FirehoseCommit) -> PEResult<S, ()> {
        trace!("started processing #commit event");

        // See [`crate::firehose::event_validation::FirehoseCommit::prevalidate`]
        // for the spec's validation steps 1–3.
        // We are picking up at step 4:
        // https://www.ietf.org/archive/id/draft-holmgren-at-synchronization-00.html#section-4.5-3.4.1

        // ...but before step 4: just drop if we know we aren't in sync (or in scope) anyway
        if !self.repo().is_in_scope() {
            trace!("dropping #commit for out-of-scope repo");
            counter!(COMMIT_OUTCOMES_TOTAL, "outcome" => "drop_out_of_scope").increment(1);
            return Ok(());
        }
        if !self.repo().is_synchronized() {
            trace!("dropping #commit for desynchronized account");
            counter!(COMMIT_OUTCOMES_TOTAL, "outcome" => "drop_desynced").increment(1);
            return Ok(());
        }

        // 4. verify the commit signature

        // failing here is just a dropped event
        if !self
            .verify_commit_object(&commit.commit, SystemTime::now())
            .await?
        {
            debug!("failed to verify commit");
            counter!(COMMIT_OUTCOMES_TOTAL, "outcome" => "drop_sig").increment(1);
            return Ok(());
        }

        // TODO: ratchet host to sync1.1 if this commit is sync1.1-ish.
        // TODO: non-sync-1.1 handling

        // not written in the spec, but we bail here after step 4 if the
        // account is not active.
        // this stays behind the signature check because we don't want to
        // trigger upstream status checks if the event isn't even authentic.
        if !self.repo().is_upstream_active() {
            // TODO: consult upstream in case our status became out of sync
            warn!("dropping commit for inactive account");
            counter!(COMMIT_OUTCOMES_TOTAL, "outcome" => "drop_inactive").increment(1);
            return Ok(());
        }

        // 5, 6. rev is newer than ours, and prevData links up.

        // we delegate these to AccountSyncState, which handles account state
        // transitions and commit drops if necessary.

        let now = SystemTime::now();
        let storage = self.storage.clone();
        let consumer_app = self.consumer_app.clone();
        let mut updating_repo = self.repo().clone(); // TODO: .take() + CoW for commits
        let mut commit_slot = self.commit_slot.clone();
        let mut info_slot = self.info_slot.clone();

        let (maybe_updated, (commit_slot, info_slot)) = spawn_blocking(move || {
            let mut batch = storage.batch();

            match updating_repo.sync_next(&commit, now, &mut batch) {
                None => {
                    counter!(COMMIT_OUTCOMES_TOTAL, "outcome" => "drop_stale").increment(1);
                    return Ok((None, (commit_slot, info_slot))); // drop
                }
                Some(ok) if !ok => {
                    // desync
                    counter!(COMMIT_OUTCOMES_TOTAL, "outcome" => "desync").increment(1);
                    batch.commit().map_err(ProcessError::Storage)?;
                    return Ok((Some(updating_repo), (commit_slot, info_slot)));
                }
                _ => {}
            }

            // all the checks passed.
            // this commit is legit.
            // it's your turrrrrrn.
            let mut slots = RepoSlots::new(&mut commit_slot, &mut info_slot);
            match consumer_app.apply_commit(
                Commit::new(&commit),
                &mut slots,
                &RepoContext::new(&updating_repo),
                batch.inner_mut(), // the consumer app writes its own keyspace
            ) {
                Err(e @ ConsumerAppError::Fatal { reason: _ }) => {
                    error!("handle consumer app fatal error");
                    return Err(ProcessError::ConsumerApp(e));
                }
                Err(ConsumerAppError::Storage(e)) => {
                    return Err(ProcessError::Storage(e));
                }
                Err(ConsumerAppError::Desynchronize { reason }) => {
                    let now = SystemTime::now();
                    let desync_reason = DesyncReason::AppRequested {
                        reason,
                        at_commit_rev: Some(commit.rev),
                    };
                    updating_repo.desynchronize(desync_reason, None, now, &mut batch);
                    counter!(COMMIT_OUTCOMES_TOTAL, "outcome" => "desync").increment(1);
                    // no return: continue to commit path
                }
                Ok(()) => {
                    counter!(COMMIT_OUTCOMES_TOTAL, "outcome" => "applied").increment(1);
                }
            }

            // slots update if any
            updating_repo.set_slots::<_, A, _>(slots, &mut batch);

            // FINALIZE THIS YAAAAA
            batch.commit().map_err(ProcessError::Storage)?;
            Ok((Some(updating_repo), (commit_slot, info_slot)))
        })
        .await
        .expect("task not to panic")?;

        if let Some(updated) = maybe_updated {
            self.repo = Some(Box::new(updated));
        } // else changes dropped

        self.commit_slot = commit_slot;
        self.info_slot = info_slot;

        Ok(())
    }

    async fn process_sync(&mut self, sync: FirehoseSync) -> PEResult<S, ()> {
        trace!("started processing #sync event");

        // ...just drop if we know we aren't in sync (or scope) anyway
        if !self.repo().is_in_scope() {
            trace!("dropping #sync for out-of-scope repo");
            counter!(SYNC_OUTCOMES_TOTAL, "outcome" => "drop_out_of_scope").increment(1);
            return Ok(());
        }
        if !self.repo().is_synchronized() {
            trace!("dropping #sync for desynchronized account");
            counter!(SYNC_OUTCOMES_TOTAL, "outcome" => "drop_desynced").increment(1);
            return Ok(());
        }

        // ..or if we know we're synchronized at some point after the #sync
        if let Some(rev) = self.repo().rev()
            && sync.rev <= rev
        {
            trace!(our_rev = %rev, sync_rev = %sync.rev, "dropping #sync with old rev");
            counter!(SYNC_OUTCOMES_TOTAL, "outcome" => "drop_stale").increment(1);
            return Ok(());
        }

        // or if it's not even authentic
        let now = SystemTime::now();
        if !self.verify_commit_object(&sync.commit, now).await? {
            debug!("dropping #sync with invalid signature");
            counter!(SYNC_OUTCOMES_TOTAL, "outcome" => "drop_sig").increment(1);
            return Ok(());
        }

        let mut updating = self.repo().clone();
        let storage = self.storage.clone();
        let now = SystemTime::now();
        let updated = spawn_blocking(move || -> PEResult<S, Repo> {
            let mut batch = storage.batch();
            let reason = DesyncReason::FirehoseSync { rev: sync.rev };
            updating.desynchronize(reason, None, now, &mut batch);
            batch.commit().map_err(ProcessError::Storage)?;
            Ok(updating)
        })
        .await
        .expect("task not to panic")?;

        self.repo = Some(Box::new(updated));
        counter!(SYNC_OUTCOMES_TOTAL, "outcome" => "desync").increment(1);
        trace!("finished processing #sync event: resync scheduled");
        Ok(())
    }

    /// re-evaluate scope against current policy with no network work
    async fn process_reconcile_scope(&mut self) -> PEResult<S, ()> {
        let mut updating = self.repo().clone();
        let storage = self.storage.clone();
        let now = SystemTime::now();
        let updated = spawn_blocking(move || -> PEResult<S, Repo> {
            let mut batch = storage.batch();
            updating.reconcile_scope(now, &mut batch);
            batch.commit().map_err(ProcessError::Storage)?;
            Ok(updating)
        })
        .await
        .expect("task not to panic")?;

        self.repo = Some(Box::new(updated));
        Ok(())
    }

    /// handle account events from subscribeRepos
    ///
    /// critical: right now this *trusts* any message received from upstream, so
    /// eg. a `delete` account event is just applied. an upstream relay *should*
    /// already be validating that account events come from hosts with authority
    /// over those accounts. if a relay fails to do that check, any account can
    /// be DOSed by any PDS upstream of the relay.
    ///
    /// if we ever subscribe to a pds directly, it would be our responsibility
    /// to do that check.
    async fn process_account(
        &mut self,
        event: FirehoseAccountEvent,
        host: Arc<Host>,
        seq: Seq,
    ) -> PEResult<S, ()> {
        use jacquard_api::com_atproto::sync::subscribe_repos::AccountStatus as AS;
        let (new_status, new_desync_reason) = match (event.active, event.status) {
            (true, None) => (AccountStatus::Active, None),
            (true, Some(AS::Desynchronized)) => (
                AccountStatus::Active,
                Some(DesyncReason::FirehoseAccountDesynchronized),
            ),
            (true, Some(AS::Throttled)) => (
                AccountStatus::Active,
                Some(DesyncReason::FirehoseAccountThrottled),
            ),
            (true, Some(AS::Other(status))) => {
                info!(%status, "#account.status unknown, leaving Active/normal");
                (AccountStatus::Active, None)
            }
            (true, Some(unrecognized)) => {
                info!(%unrecognized, "#account.status unexpected for `active=true`, leaving Active/normal");
                (AccountStatus::Active, None)
            }
            (false, None) => {
                info!("#account status missing for `active=false`, setting inactive");
                (
                    AccountStatus::Inactive("#account.inactive-empty".to_string()),
                    None,
                )
            }
            (false, Some(AS::Deactivated)) => (AccountStatus::Deactivated, None),
            (false, Some(AS::Suspended)) => (AccountStatus::Suspended, None),
            (false, Some(AS::Takendown)) => (AccountStatus::Takendown, None),
            (false, Some(AS::Deleted)) => (AccountStatus::Deleted, None),
            (false, Some(AS::Desynchronized)) => {
                info!("#account `desynchronized` unexpected for `active=false`, setting inactive");
                (
                    AccountStatus::Inactive("#account.inactive-desynchronized".to_string()),
                    None,
                )
            }
            (false, Some(AS::Throttled)) => {
                info!("#account `throttled` unexpected for `active=false`, setting inactive");
                (
                    AccountStatus::Inactive("#account.inactive-throttled".to_string()),
                    None,
                )
            }
            (false, Some(AS::Other(unrecognized))) => {
                info!(%unrecognized, "#account.status unexpected for `active=false`, setting inactive");
                (
                    AccountStatus::Inactive(format!("#account.inactive-{unrecognized}")),
                    None,
                )
            }
        };

        let mut changed = false;

        // update our status from upstream
        let now = SystemTime::now();
        let seq = seq.as_u64() as i64;
        if new_status != self.repo().info().upstream_status {
            debug!(?new_status, "applying #account event status change");
            self.set_account_status_from_upstream(AccountStatusEvent {
                subject: self.repo().did().clone(),
                at: now,
                upstream: Some(AccountStatusUpstream { host, seq }),
                status: new_status,
            })
            .await?; // TODO host in log
            changed = true;
        }

        // TODO: for now we're not setting sync state to "deactivated"
        // i think my idea was that we'd do that on the first valid dropped commit or something...

        // desync if the event from upstream implies that :/
        if self.repo().is_synchronized()
            && let Some(reason) = new_desync_reason
        {
            debug!(?reason, "applying #account desync from synchronized state");
            let mut updating = self.repo().clone();
            let storage = self.storage.clone();
            let now = SystemTime::now();
            let updated = spawn_blocking(move || -> PEResult<S, Repo> {
                let mut batch = storage.batch();
                updating.desynchronize(reason, None, now, &mut batch);
                batch.commit().map_err(ProcessError::Storage)?;
                Ok(updating)
            })
            .await
            .expect("task not to panic")?;
            self.repo = Some(Box::new(updated));
            changed = true;
        }

        let outcome = if changed { "changed" } else { "noop" };
        counter!(ACCOUNT_OUTCOMES_TOTAL, "outcome" => outcome).increment(1);
        trace!(?changed, "finished processing #account event");
        Ok(())
    }

    async fn process_identity(&mut self, identity: Identity) -> PEResult<S, ()> {
        trace!(new_handle = ?identity.handle, "started processing #identity");
        let now = SystemTime::now();
        let refreshed = self
            .refresh_identity(now, "firehose_identity")
            .await?
            .is_ok();
        // TODO: when we actually deal with handles, we may want to force a refresh
        trace!(?refreshed, "finished processing #identity");
        Ok(())
    }

    #[tracing::instrument(skip(self, was_due_at), fields(
        current_pds = tracing::field::Empty,
        fetched_from = tracing::field::Empty,
    ))]
    async fn process_scheduled_resync(
        &mut self,
        was_due_at: SystemTime,
        permit: OwnedSemaphorePermit,
    ) -> PEResult<S, ()> {
        trace!(late_by = ?was_due_at.elapsed(), "started processing scheduled resync");

        // spurious resyncs *should* only be possible when the in-memory resync
        // schedule slots become stale and emit old resyncs. (we have at-least-
        // once semantics there so it's not too concerning)
        let Some(desync) = self.repo().current_desync() else {
            debug!(
                was_due_at_unix = ?was_due_at.duration_since(UNIX_EPOCH),
                was_due_since_now = ?was_due_at.elapsed(),
                "spurious resync task (repo not desynchronized), ignoring"
            );
            counter!(RESYNC_OUTCOMES_TOTAL, "outcome" => "spurious").increment(1);
            return Ok(());
        };
        if !desync.is_at(was_due_at) {
            debug!(
                expected_due_unix = ?was_due_at.duration_since(UNIX_EPOCH),
                found_due_unix = ?desync.due_at().duration_since(UNIX_EPOCH),
                diff = ?was_due_at.duration_since(desync.due_at()),
                "spurious resync task (wrong `due_at`), ignoring");
            counter!(RESYNC_OUTCOMES_TOTAL, "outcome" => "spurious").increment(1);
            return Ok(());
        }
        let reason = desync.reason.clone();

        let current_pds = self.repo().info().identity.pds_host.clone();
        tracing::Span::current().record("current_pds", current_pds.name().as_str());

        let resync_started = Instant::now();
        let now = SystemTime::now();

        let mut commit_slot = self.commit_slot.clone();
        let mut info_slot = self.info_slot.clone();
        let mut slots = RepoSlots::new(&mut commit_slot, &mut info_slot);

        let big_permits = BigRepoPermits::new(self.big_repo.clone());
        let (data_res, fetched_from) = self.get_resync_data(&mut slots, big_permits.clone()).await;

        if let Some(ref resolved) = fetched_from {
            tracing::Span::current().record("fetched_from", resolved.name().as_str());
        }

        let big_permit_used = big_permits.was_used();
        let mut data = match data_res {
            Ok(dr) => dr,
            Err(ResyncError::Cancelled) => {
                // didn't get a chance to complete, leave it queued in place
                trace!("resync cancelled, abandoning (will remain in queue)");
                counter!(RESYNC_OUTCOMES_TOTAL, "outcome" => "cancelled").increment(1);
                return Ok(());
            }
            Err(ResyncError::NoPermitAvailable) => {
                trace!("resync failed to preacquire a permit, delaying");
                counter!(RESYNC_OUTCOMES_TOTAL, "outcome" => "preacquire_no_permit").increment(1);
                return self.delay_resync_for_permit().await;
            }
            Err(ResyncError::ReactivePermitTimeout) => {
                info!("resync failed to reactively acquire a permit on time, delaying");
                counter!(RESYNC_OUTCOMES_TOTAL, "outcome" => "permit_timeout").increment(1);
                return self.delay_resync_for_permit().await;
            }
            Err(ResyncError::Transient(err)) => {
                // temporary or possibly-non-permanent fail, queue up a retry
                info!(?err, "transient resync error, rescheduling");
                counter!(RESYNC_OUTCOMES_TOTAL, "outcome" => "reschedule").increment(1);
                return self
                    .reschedule_resync(reason, "transient error", big_permit_used)
                    .await;
            }
            Err(ResyncError::WrongDid { expected, got }) => {
                // the data was for the wrong account... queue up a retry i guess?
                info!(%expected, %got, "wrong DID from resync data, rescheduling");
                counter!(RESYNC_OUTCOMES_TOTAL, "outcome" => "reschedule").increment(1);
                return self
                    .reschedule_resync(reason, "wrong did", big_permit_used)
                    .await;
            }
            Err(ResyncError::Status(reported_status)) => {
                info!(status = ?reported_status, "resync errored with inactive account status (updating)");
                // the PDS (assuming we reached the PDS) returned an error
                // error indicating the status of the requested repo. update our
                // understanding of it.
                counter!(RESYNC_OUTCOMES_TOTAL, "outcome" => "status").increment(1);
                return self
                    .set_account_status_from_upstream(AccountStatusEvent {
                        subject: self.repo().did().clone(),
                        at: SystemTime::now(),
                        upstream: Some(AccountStatusUpstream {
                            host: self.upstream.clone(),
                            seq: -1,
                        }),
                        status: reported_status,
                    })
                    .await;
            }
            Err(ResyncError::RepoMissing) => {
                // TODO: this needs a slight rethink!
                info!(
                    desync_reason = ?reason,
                    "resync: repo not found from host. for now: marking gone");
                counter!(RESYNC_OUTCOMES_TOTAL, "outcome" => "deleted").increment(1);
                return self
                    .set_account_status_from_upstream(AccountStatusEvent {
                        subject: self.repo().did().clone(),
                        at: now,
                        upstream: Some(AccountStatusUpstream {
                            host: self.upstream.clone(),
                            seq: -1,
                        }),
                        status: AccountStatus::Deleted,
                    })
                    .await;

                // *probably* the thing to do here is desynchronize with a
                // DesyncReason::NotFoundUpstream or something. retry backoff
                // schedule once daily for 7 days then give up etc.

                // old stuff:

                // let hostname = self.repo().info().identity.pds_host.name();
                // if desync.resync_attempts < 48 {
                //     info!(
                //         desync_reason = ?reason,
                //         host = %hostname,
                //         "resync: repo not found from host. refreshing identity (migration check) + rescheduling");
                //     #[expect(unused_must_use, reason = "don't care about refresh outcome")]
                //     self.refresh_identity(now, "resync_repo_missing").await?;
                //     counter!(RESYNC_OUTCOMES_TOTAL, "outcome" => "repo_missing").increment(1);
                //     return self
                //         .reschedule_resync(reason, "repo missing", big_permit_used)
                //         .await;
                // }
                // info!(
                //     desync_reason = ?reason,
                //     host = %hostname,
                //     "resync: retries exhausted for repo not found. setting deleted");
                // // for now we set the account status, but should we be setting
                // // the sync status? sync status has `Gone` with a reason and
                // // timestamp (settable via repo.terminate(), vs account status
                // // whose transition reaches the consumer app. going with
                // // consumer app visibility for now.
                // //
                // // but it feels a bit messy because this is a local moderation
                // // state effectively
                // counter!(RESYNC_OUTCOMES_TOTAL, "outcome" => "deleted").increment(1);
                // return self
                //     .set_account_status_from_upstream(AccountStatusEvent {
                //         subject: self.repo().did().clone(),
                //         at: SystemTime::now(),
                //         upstream: Some(AccountStatusUpstream { host, seq: -1 }),
                //         status: AccountStatus::Deleted,
                //     })
                //     .await;
            }
        };

        // make sure the commit object is actually *for* this repo
        let commit = data.commit();

        if commit.did != *self.repo().did() {
            info!(
                commit_did = %commit.did,
                resync_reason = ?reason,
                "resync data was for the wrong DID. rescheduling resync.");
            counter!(RESYNC_OUTCOMES_TOTAL, "outcome" => "reschedule").increment(1);
            return self
                .reschedule_resync(reason, "wrong did", big_permit_used)
                .await;
        }

        // and verify the signature
        if !self
            .verify_commit_object(data.commit(), SystemTime::now())
            .await?
        {
            debug!(resync_reason = ?reason, "failed to verify commit from resync data. rescheduling resync.");
            counter!(RESYNC_OUTCOMES_TOTAL, "outcome" => "reschedule").increment(1);
            return self
                .reschedule_resync(reason, "signature fail", big_permit_used)
                .await;
        }

        // note if any app state slots were changed
        let dirtiness = slots.dirtiness();

        // rev checked in resynchronize() path, in [`AccountSyncState::reset`]

        // resync-apply and sync1.1 transition
        let now = SystemTime::now();
        let rev = data.commit().rev;
        let storage = self.storage.clone();
        let consumer_app = self.consumer_app.clone();
        let mut updating_repo = self.repo().clone();

        let (updated, (commit_slot, info_slot)) = spawn_blocking(move || -> PEResult<S, _> {
            let mut batch = storage.batch();

            let mut slots = RepoSlots::new_from(&mut commit_slot, &mut info_slot, dirtiness);

            // do our part
            if !updating_repo.resynchronize(reason, data.commit(), now, &mut batch) {
                if big_permit_used {
                    updating_repo.set_needs_first_permit(&mut batch);
                }
                // a custom resync *could* write to slots
                updating_repo.set_slots::<_, A, _>(slots, &mut batch);
                batch.commit().map_err(ProcessError::Storage)?;

                counter!(RESYNC_OUTCOMES_TOTAL, "outcome" => "reschedule").increment(1);
                return Ok((updating_repo, (commit_slot, info_slot)));
            };

            // your turn
            match consumer_app.apply_resync(
                &mut data,
                &mut slots,
                &RepoContext::new(&updating_repo),
                batch.inner_mut(), // the consumer app writes its own keyspace
            ) {
                Err(err @ ConsumerAppError::Fatal { .. }) => {
                    error!(%err, "consumer app's apply_resync");
                    return Err(ProcessError::ConsumerApp(err));
                }
                Err(ConsumerAppError::Storage(err)) => {
                    error!(%err, "consumer app's apply_resync");
                    return Err(ProcessError::Storage(err));
                }
                Err(ConsumerAppError::Desynchronize { reason }) => {
                    let now = SystemTime::now();
                    let desync_reason = DesyncReason::AppRequested {
                        reason,
                        at_commit_rev: Some(rev),
                    };
                    updating_repo.desynchronize(desync_reason, None, now, &mut batch);
                    if big_permit_used {
                        updating_repo.set_needs_first_permit(&mut batch);
                    }
                    counter!(RESYNC_OUTCOMES_TOTAL, "outcome" => "reschedule").increment(1);
                    // no return: continue to commit path
                }
                Ok(()) => {
                    // aaaand the stats, if we succeeded
                    // (it feels a little unfortunate that this is separate from `.resynchronize` but oh well)
                    let resync_info = ResyncInfo::new(
                        now,
                        data.size(),
                        data.count(),
                        fetched_from,
                        Some(resync_started.elapsed()),
                    );
                    updating_repo.record_resync_info(resync_info, &mut batch);

                    if let Some(size) = data.size() {
                        histogram!(RESYNC_SIZE).record(size as f64);
                    }
                    if let Some(count) = data.count() {
                        histogram!(RESYNC_COUNT).record(count as f64);
                    }
                    histogram!(RESYNC_DURATION_SECONDS)
                        .record(resync_started.elapsed().as_secs_f64());
                    counter!(RESYNC_OUTCOMES_TOTAL, "outcome" => "synced").increment(1);
                }
            }

            // and if any slots state changed
            updating_repo.set_slots::<_, A, _>(slots, &mut batch);

            batch.commit().map_err(ProcessError::Storage)?;
            Ok((updating_repo, (commit_slot, info_slot)))
        })
        .await
        .expect("task not to panic")?;

        self.repo = Some(Box::new(updated));
        self.commit_slot = commit_slot;
        self.info_slot = info_slot;
        trace!("scheduled resync applied");

        Ok(())
    }

    async fn delay_resync_for_permit(&mut self) -> PEResult<S, ()> {
        let mut repo = self.repo().clone();
        let storage = self.storage.clone();
        let delay = delay_jitter(repo.did());
        let updated = spawn_blocking(move || -> PEResult<S, Repo> {
            let mut batch = storage.batch();
            let now = SystemTime::now();
            repo.delay_resync_until(now + delay, &mut batch);
            repo.set_needs_first_permit(&mut batch);
            batch.commit().map_err(ProcessError::Storage)?;
            Ok(repo)
        })
        .await
        .expect("task not to panic")?;
        self.repo = Some(Box::new(updated));
        Ok(())
    }

    async fn reschedule_resync(
        &mut self,
        reason: DesyncReason,
        err: &'static str,
        needed_permit: bool,
    ) -> PEResult<S, ()> {
        let mut repo = self.repo().clone();
        let storage = self.storage.clone();
        let updated = spawn_blocking(move || -> PEResult<S, Repo> {
            let mut batch = storage.batch();
            // desynchronize bumps count + pushes backoff
            let now = SystemTime::now();
            repo.desynchronize(reason, Some(err), now, &mut batch);
            if needed_permit {
                repo.set_needs_first_permit(&mut batch);
            }
            batch.commit().map_err(ProcessError::Storage)?;
            Ok(repo)
        })
        .await
        .expect("task not to panic")?;
        self.repo = Some(Box::new(updated));
        Ok(())
    }

    /// fetch the data for handling a resync
    ///
    /// calls the consumer app's `resync()` impl
    ///
    /// - checks the commit DID
    /// - caller must verify commit signature
    async fn get_resync_data(
        &self,
        slots: &mut RepoSlots<'_, A::CommitState, A::InfoState>,
        big_permits: BigRepoPermits,
    ) -> (Result<A::ResyncData, ResyncError>, Option<Arc<Host>>) {
        let repo = self.repo();
        let did = repo.did();

        let should_preacquire = repo.info().first_resync_needs_permit.is_some()
            || repo
                .info()
                .last_resync
                .as_ref()
                .zip(repo.sync_state())
                .is_some_and(|(last, sync)| {
                    A::ResyncData::is_big(
                        last.size,
                        last.count,
                        sync.commits,
                        sync.records_count_delta,
                    )
                });

        let permit = if should_preacquire {
            // known-big, pre-acquire, but don't hold the actor alive if none
            // are available
            let Some(permit) = big_permits.try_acquire_owned() else {
                return (Err(ResyncError::NoPermitAvailable), None);
            };
            // TODO: counter metric here probably
            Some(permit)
        } else {
            None
        };

        let mut context = ResyncContext {
            repo,
            upstream: &self.upstream,
            upstream_kind: self.upstream_kind,
            upstream_get_repo_token: self.upstream_get_repo_token.as_ref(),
            resolved: None,
            cancel: &self.cancel,
            permit,
            big_permits,
            reactive_permit_wait: self.reactive_permit_wait,
            spill_dir: &self.spill_dir,
        };

        // get the resync data (consumer-overrideable)
        let data = match self.consumer_app.resync(slots, &mut context).await {
            Ok(d) => d,
            Err(e) => return (Err(e), context.resolved),
        };

        // ensure the commit is actually *for* this repo
        if data.commit().did != *did {
            let err = Err(ResyncError::WrongDid {
                expected: did.clone(),
                got: data.commit().did.clone(),
            });
            return (err, context.resolved);
        }

        (Ok(data), context.resolved)
    }

    async fn process_moderate(
        &mut self,
        event: ModerateEvent,
        reply: Option<oneshot::Sender<ModerateOutcome>>,
    ) -> PEResult<S, ()> {
        let now = SystemTime::now();
        let res = self.set_moderation(event, now).await;

        if let Some(reply) = reply {
            let outcome = match &res {
                Ok(_) => ModerateOutcome::Applied,
                Err(ProcessError::ConsumerApp(e)) => ModerateOutcome::Failed {
                    error: "ConsumerAppError",
                    message: e.to_string(),
                },
                Err(other) => ModerateOutcome::Failed {
                    error: "InternalError",
                    message: other.to_string(),
                },
            };

            if let Err(outcome) = reply.send(outcome) {
                warn!(?outcome, "moderation outcome oneshot send failed");
            }
        }

        res
    }

    async fn process_refresh(
        &mut self,
        reply: Option<oneshot::Sender<ResolvedIdentity>>,
    ) -> PEResult<S, ()> {
        let now = SystemTime::now();
        let outcome = self.refresh_identity(now, "requested").await?;
        if let Some(reply) = reply {
            let res = reply.send(outcome);
            if res.is_err() {
                tracing::trace!("resolve identity didn't reply -- caller dropped");
            }
        }
        Ok(())
    }

    fn repo(&self) -> &Repo {
        self.repo.as_ref().expect("repo present in processor")
    }

    /// check the signature on a commit event
    ///
    /// refreshes identity on retryable failure: takes `&mut self` to mutate and
    /// persist on successful identity refresh (so, careful with clones).
    ///
    /// returns true if the commit is valid
    async fn verify_commit_object(
        &mut self,
        commit: &CommitObject,
        now: SystemTime,
    ) -> PEResult<S, bool> {
        let ident = &self.repo().info().identity;

        match validate_commit_signature(&ident.signing_key, commit) {
            Ok(()) => {
                trace!("commit signature validation succeeded!");
                return Ok(true);
            }
            Err(e) if !e.could_retry_with_another_key() => {
                trace!(err = %e, "commit signature validation failed and cannot be retried");
                return Ok(false);
            }
            _ => {} // worth a retry
        };

        // worth a retry with another key, so attempt a refresh
        let prev_key = self.repo().info().identity.signing_key.clone();
        if self.refresh_identity(now, "sig_retry").await?.is_err() {
            // could not (or failed to) refresh: fail
            trace!("commit signature validation failed and could not be retried");
            return Ok(false);
        }

        // if the key hasn't changed, there's nothing more for us to try
        let new_key = &self.repo().info().identity.signing_key;
        if *new_key == prev_key {
            trace!("commit signature validation failed and key unchanged after identity refresh");
            return Ok(false);
        }

        // otherwise, one last shot:
        Ok(if let Err(e) = validate_commit_signature(new_key, commit) {
            trace!(err = %e, "commit signature validation failed even with refreshed key");
            false
        } else {
            trace!("commit signature validation succeeded with refreshed key!");
            true
        })
    }

    async fn refresh_identity(
        &mut self,
        now: SystemTime,
        trigger: &'static str,
    ) -> PEResult<S, ResolvedIdentity> {
        let repo = self.repo();
        let did = repo.did();
        let age = repo.info().identity.age(now);
        if !Validity::can_refresh(did, age) {
            trace!("identity too fresh to refresh");
            counter!(IDENTITY_REFRESH_OUTCOMES_TOTAL, "outcome" => "snoozed", "trigger" => trigger)
                .increment(1);
            return Ok(Err(ResolutionError::TooSoon(
                Validity::duration_until_refresh(age),
            )));
        }
        match self.resolver.resolve(did, now).await {
            Ok(new_identity) => {
                let mut updating = self.repo().clone();
                let storage = self.storage.clone();
                let identity = new_identity.clone();
                let updated = spawn_blocking(move || -> PEResult<S, Repo> {
                    let mut batch = storage.batch();
                    updating.set_identity(identity, now, &mut batch);
                    batch.commit().map_err(ProcessError::Storage)?;
                    Ok(updating)
                })
                .await
                .expect("task not to panic")?;

                self.repo = Some(Box::new(updated));

                counter!(IDENTITY_REFRESH_OUTCOMES_TOTAL, "outcome" => "refreshed", "trigger" => trigger)
                    .increment(1);
                Ok(Ok(new_identity))
            }
            Err(err) => {
                debug!(%err, "identity refresh failed");
                counter!(IDENTITY_REFRESH_OUTCOMES_TOTAL, "outcome" => "failed", "trigger" => trigger)
                    .increment(1);
                Ok(Err(err))
            }
        }
    }

    async fn set_account_status_from_upstream(
        &mut self,
        event: AccountStatusEvent,
    ) -> PEResult<S, ()> {
        self.apply_status_change(event.at, move |repo, batch| {
            repo.set_upstream_status(event, batch);
        })
        .await
    }

    async fn set_moderation(&mut self, event: ModerateEvent, now: SystemTime) -> PEResult<S, ()> {
        self.apply_status_change(now, move |repo, batch| {
            repo.set_moderation(event, batch);
        })
        .await
    }

    async fn apply_status_change<F>(&mut self, now: SystemTime, mutate: F) -> PEResult<S, ()>
    where
        F: FnOnce(&mut Repo, &mut PrefixedBatch<S::Batch>) + Send + 'static,
    {
        let storage = self.storage.clone();
        let consumer_app = self.consumer_app.clone();
        let mut commit_slot = self.commit_slot.clone();
        let mut info_slot = self.info_slot.clone();
        let mut updating = self.repo().clone();

        let (updated, (commit_slot, info_slot)) = spawn_blocking(move || -> PEResult<S, _> {
            let mut batch = storage.batch();

            // 1. hubble-sync's own update (staged on clone of actor's repo)
            let prev_status = updating.info().account_status();
            mutate(&mut updating, &mut batch);

            // bypass app callback if the *effective* status hasn't changed
            if updating.info().account_status() == prev_status {
                batch.commit().map_err(ProcessError::Storage)?;
                return Ok((updating, (commit_slot, info_slot)));
            }

            // 1.5 coordinate deleted sync state if upstream set deleted
            let is_delete = matches!(updating.info().account_status(), AccountStatus::Deleted);
            if is_delete {
                updating.terminate("account status -> deleted".to_string(), now, &mut batch);
            }

            // 2. consumer app's update
            let mut slots = RepoSlots::new(&mut commit_slot, &mut info_slot);
            let context = RepoContext::new(&updating);
            // the consumer app writes its own keyspace
            match consumer_app.apply_status(&prev_status, &mut slots, &context, batch.inner_mut()) {
                Ok(()) => {}
                Err(ConsumerAppError::Desynchronize { reason }) => {
                    let reason = DesyncReason::AppRequested {
                        reason,
                        at_commit_rev: None,
                    };
                    let now = SystemTime::now();
                    updating.desynchronize(reason, None, now, &mut batch);
                }
                Err(e) => return Err(ProcessError::ConsumerApp(e)),
            }

            if is_delete {
                // 1.5 continued: also delete slot state if deleted
                if slots.commit.is_dirty() && slots.commit.get().is_some() {
                    warn!(
                        slot = "commit",
                        "dropping commit state update: account status => deleted"
                    );
                }
                if slots.info.is_dirty() && slots.info.get().is_some() {
                    warn!(
                        slot = "info",
                        "dropping info state update: account status => deleted"
                    );
                }
                slots.commit.clear();
                slots.info.clear();
            }

            // let slots possibly update whether deleting (cleared) or not
            updating.set_slots::<_, A, _>(slots, &mut batch);

            // 3. atomic batch commit both
            batch.commit().map_err(ProcessError::Storage)?;

            // 4. return the mutated repo for actor to update to
            Ok((updating, (commit_slot, info_slot)))
        })
        .await
        .expect("storage didn't panic")?;

        // we don't get here if any error happend up to the batch -- actor local
        // state stays the same as it was before this function was called.
        self.repo = Some(Box::new(updated));
        self.commit_slot = commit_slot;
        self.info_slot = info_slot;
        Ok(())
    }
}

/// jittered delay for retrying a non-failing resync (eg. big-repo acquire fail)
///
/// jitters on DID, which should be nicely distributed unless there's a strange
/// DID-correlated retry event.
fn delay_jitter(did: &Did) -> Duration {
    use std::hash::{Hash, Hasher};
    const LOWER: Duration = Duration::from_secs(30);
    const RANGE_S: u64 = 60;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    did.as_str().hash(&mut h);
    LOWER + Duration::from_secs(h.finish() % RANGE_S)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::future::Future;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::identity::SigningKey;
    use crate::resync_scheduler::QueuedResync;
    use crate::storage::engine::mem::MemEngine;
    use crate::storage::repo::{Awoken, RepoIdentity, RepoInfo};
    use crate::sync_consumer::{AppResult, SimpleSyncConsumer};
    use crate::{HostRegistry, RepoContext};

    /// No-op consumer, just to satisfy the processor's generic bounds.
    struct StubConsumer;
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

    /// Counts resolve attempts and always fails.
    ///
    /// Signature verification's refresh fallback is the only network reach in
    /// the #commit/#sync paths, so the count tells us whether an event got
    /// past the early drop guards (dropped events must never touch identity).
    struct TouchResolver(AtomicUsize);
    impl Resolve for TouchResolver {
        fn resolve(
            &self,
            _did: &Did,
            _now: SystemTime,
        ) -> impl Future<Output = ResolvedIdentity> + Send {
            self.0.fetch_add(1, Ordering::SeqCst);
            async { Err(ResolutionError::Network("test resolver".to_string())) }
        }
    }

    fn plc_did(label: &str) -> Did {
        let mut s = label.to_string();
        while s.len() < 24 {
            s.push('a');
        }
        s.truncate(24);
        Did::new(format!("did:plc:{s}")).expect("valid plc did")
    }

    const REPO_PDS: &str = "pds.example.com";

    /// install a repo in the given sync state, primed so that signature
    /// verification (if reached) recurses into an identity refresh:
    /// - the raw key has no multicodec prefix -> BadKey, retryable-with-
    ///   another-key
    /// - the identity is past the refresh minimum age, so the refresh attempt
    ///   reaches the resolver
    fn install_repo(
        storage: &PrefixedEngine<MemEngine>,
        hosts: &HostRegistry,
        did: &Did,
        sync_status: SyncStatus,
    ) {
        let resolved_at = SystemTime::now() - Duration::from_secs(600);
        let info = RepoInfo {
            upstream_status: AccountStatus::Active,
            moderation: None,
            sync_status,
            identity: RepoIdentity {
                pds_host: hosts.get(REPO_PDS).expect("interned"),
                signing_key: SigningKey::raw(vec![0u8; 32]),
                supposed_handle: Some("alice.example".to_string()),
                resolved_at,
            },
            first_seen_at: resolved_at,
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

    fn processor(
        did: &Did,
        sync_status: SyncStatus,
    ) -> (
        TaskProcessor<MemEngine, StubConsumer, TouchResolver>,
        Arc<TouchResolver>,
        Arc<HostRegistry>,
    ) {
        let storage = MemEngine::new_prefixed();
        let hosts = HostRegistry::new_default();
        install_repo(&storage, &hosts, did, sync_status);

        let (awoken, _slots) =
            Repo::wake::<_, (), ()>(&storage, &hosts, did.clone(), SystemTime::now())
                .expect("wake ok");
        let Awoken::Resolved(repo) = awoken else {
            panic!("test precondition: expected a resolved repo");
        };

        let resolver = Arc::new(TouchResolver(AtomicUsize::new(0)));
        let tp = TaskProcessor {
            storage,
            resolver: resolver.clone(),
            consumer_app: Arc::new(StubConsumer),
            cancel: CancellationToken::new(),
            big_repo: Arc::new(Semaphore::new(1)),
            reactive_permit_wait: Duration::from_secs(1),
            spill_dir: Arc::from(PathBuf::from("test-spill")),
            upstream: hosts.get("example.com").expect("interned"),
            upstream_kind: UpstreamKind::PdsDirect,
            upstream_get_repo_token: None,
            repo: Some(Box::new(repo)),
            commit_slot: None,
            info_slot: None,
        };
        (tp, resolver, hosts)
    }

    fn test_commit_object(did: &Did) -> CommitObject {
        CommitObject {
            did: did.clone(),
            data: "bafyreieebzlwyqijbjyzfpjhzicsniw3h3zc3hpen2m66jgg543kpnpg4e"
                .parse()
                .expect("valid cid"),
            rev: "3mqpk4zzyed2t".parse().expect("valid tid"),
            prev: None,
            sig: vec![0u8; 64],
        }
    }

    fn dummy_commit(did: &Did) -> FirehoseCommit {
        let commit = test_commit_object(did);
        FirehoseCommit {
            data: commit.data,
            prev: commit.data,
            rev: commit.rev,
            ops: vec![],
            since: None,
            blocks: HashMap::new(),
            commit,
        }
    }

    fn dummy_sync(did: &Did) -> FirehoseSync {
        let commit = test_commit_object(did);
        FirehoseSync {
            rev: commit.rev,
            commit,
        }
    }

    fn queued_for(
        tp: &TaskProcessor<MemEngine, StubConsumer, TouchResolver>,
        host: Arc<Host>,
    ) -> bool {
        QueuedResync::peek(&tp.storage, Some(host))
            .expect("peek")
            .is_some()
    }

    #[tokio::test]
    async fn reconcile_scope_brings_out_of_scope_repo_into_scope() {
        // an out-of-scope repo whose host is now in scope (the default registry
        // is Everything) re-desyncs with CameIntoScope and re-enters the queue.
        let did = plc_did("reconcilein");
        let (mut tp, _resolver, hosts) = processor(
            &did,
            SyncStatus::OutOfScope {
                since: SystemTime::now(),
            },
        );

        tp.process_reconcile_scope().await.expect("processed");

        match &tp.repo().info().sync_status {
            SyncStatus::Desynchronized(d) => {
                assert!(matches!(d.reason, DesyncReason::CameIntoScope));
            }
            other => panic!("expected desynchronized, got {other:?}"),
        }
        assert!(queued_for(&tp, hosts.get(REPO_PDS).expect("interned")));
    }

    #[tokio::test]
    async fn reconcile_scope_is_noop_for_in_scope_repo() {
        // a synchronized (already in-scope) repo is left untouched: no spurious
        // desync, nothing queued.
        let did = plc_did("reconcilenoop");
        let (mut tp, _resolver, hosts) = processor(&did, SyncStatus::Synchronized);

        tp.process_reconcile_scope().await.expect("processed");

        assert!(matches!(
            tp.repo().info().sync_status,
            SyncStatus::Synchronized
        ));
        assert!(!queued_for(&tp, hosts.get(REPO_PDS).expect("interned")));
    }

    #[tokio::test]
    async fn commit_for_out_of_scope_repo_drops_without_reaching_identity() {
        let did = plc_did("ooscommit");
        let (mut tp, resolver, hosts) = processor(
            &did,
            SyncStatus::OutOfScope {
                since: SystemTime::now(),
            },
        );

        tp.process_commit(dummy_commit(&did))
            .await
            .expect("processed");

        assert!(matches!(
            tp.repo().info().sync_status,
            SyncStatus::OutOfScope { .. }
        ));
        assert_eq!(
            resolver.0.load(Ordering::SeqCst),
            0,
            "dropped events must never reach signature verification / identity refresh",
        );
        assert!(!queued_for(&tp, hosts.get(REPO_PDS).expect("interned")));
    }

    #[tokio::test]
    async fn commit_for_in_scope_repo_reaches_verification() {
        // control for the drop test: the same commit against a synchronized
        // repo gets past the guards to signature verification, whose refresh
        // fallback touches the resolver
        let did = plc_did("insccommit");
        let (mut tp, resolver, _hosts) = processor(&did, SyncStatus::Synchronized);

        tp.process_commit(dummy_commit(&did))
            .await
            .expect("processed");

        assert_eq!(
            resolver.0.load(Ordering::SeqCst),
            1,
            "un-dropped commits reach signature verification",
        );
        // unverifiable signature: still just a drop, no state change
        assert!(matches!(
            tp.repo().info().sync_status,
            SyncStatus::Synchronized
        ));
    }

    #[tokio::test]
    async fn sync_for_out_of_scope_repo_drops_without_desync_or_identity() {
        let did = plc_did("oossync");
        let (mut tp, resolver, hosts) = processor(
            &did,
            SyncStatus::OutOfScope {
                since: SystemTime::now(),
            },
        );

        tp.process_sync(dummy_sync(&did)).await.expect("processed");

        assert!(
            matches!(tp.repo().info().sync_status, SyncStatus::OutOfScope { .. }),
            "#sync must not desynchronize an out-of-scope repo",
        );
        assert_eq!(resolver.0.load(Ordering::SeqCst), 0);
        assert!(!queued_for(&tp, hosts.get(REPO_PDS).expect("interned")));
    }

    #[tokio::test]
    async fn sync_for_in_scope_repo_reaches_verification() {
        let did = plc_did("inscsync");
        let (mut tp, resolver, hosts) = processor(&did, SyncStatus::Synchronized);

        tp.process_sync(dummy_sync(&did)).await.expect("processed");

        assert_eq!(
            resolver.0.load(Ordering::SeqCst),
            1,
            "un-dropped #sync events reach signature verification",
        );
        // unverifiable signature: dropped there, so no desync either
        assert!(matches!(
            tp.repo().info().sync_status,
            SyncStatus::Synchronized
        ));
        assert!(!queued_for(&tp, hosts.get(REPO_PDS).expect("interned")));
    }

    // --- permit-contention delay ---

    #[test]
    fn delay_jitter_is_deterministic_and_bounded() {
        let did = plc_did("jitter");
        assert_eq!(
            delay_jitter(&did),
            delay_jitter(&did),
            "same DID -> same delay (deterministic; spreads the herd without an RNG)",
        );
        let v = delay_jitter(&did);
        assert!(
            v >= Duration::from_secs(30) && v < Duration::from_secs(90),
            "within the 30..90s window, got {v:?}",
        );
        // distinct DIDs spread across the window (all-20-collide is impossible)
        let spread: std::collections::HashSet<_> = (0..20)
            .map(|i| delay_jitter(&plc_did(&format!("j{i}"))))
            .collect();
        assert!(spread.len() > 1, "DID jitter spreads across the window");
    }

    #[tokio::test]
    async fn delay_resync_for_permit_preserves_reason_and_attempts_and_flags() {
        // permit contention is not a repo failure: the delay path must keep the
        // desync reason + attempt count (unlike `desynchronize`, which would
        // bump attempts and grow the exponential backoff), just nudge `due` out.
        let did = plc_did("permitdelay");
        let before = SystemTime::now();
        let status = SyncStatus::Desynchronized(Desynchronized::new(
            DesyncReason::FirstSeen,
            before,
            3, // pre-existing attempts, to prove they're preserved
            None,
        ));
        let (mut tp, _resolver, _hosts) = processor(&did, status);

        tp.delay_resync_for_permit().await.expect("delayed");

        match &tp.repo().info().sync_status {
            SyncStatus::Desynchronized(d) => {
                assert!(
                    matches!(d.reason, DesyncReason::FirstSeen),
                    "reason preserved (not a failure/reason change)",
                );
                assert_eq!(
                    d.resync_attempts, 3,
                    "attempts unchanged -- contention must not inflate the backoff",
                );
                let due = d.due_at();
                assert!(
                    due > before + Duration::from_secs(29)
                        && due < before + Duration::from_secs(91),
                    "due nudged into the jitter window, {:?} past `before`",
                    due.duration_since(before),
                );
            }
            other => panic!("expected still-Desynchronized, got {other:?}"),
        }
        assert!(
            tp.repo().info().first_resync_needs_permit.is_some(),
            "flagged to pre-acquire so the retry fails cheap if still contended",
        );
    }
}
