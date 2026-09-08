//! the aggregate a repo persisted state
//!
//! canonical state is in two pieces: account and sync
//!
//! - sync (see `repo_state_sync`): minimal state for sync1.1 inductive proof,
//!   updates on every repo commit.
//! - account (see `repo_state_account`): everything else: account status,
//!   resolved identity, overall sync state, etc.
//!
//! additionally there is one index (see `repo_index_resync`) which is a host-
//! partitioned, time-ordered index of every active repo in desynchronized
//! sync state.
//!
//! this module provides a higher-level interface to repo state, maintaining
//! consistency across all three key ranges through any changes.

mod info;
mod info_idx_resync;
mod info_idx_state_count;
mod info_idx_status_count;
mod info_log_status;
mod info_slot;
mod pending;
mod pending_idx_retry;
mod prev;
mod prev_slot;

use super::*;

pub use info::{
    AccountStatus, AccountStatusKind, AccountStatusSource, DesyncReason, DesyncReasonKind,
    Desynchronized, ModAction, Moderation, RepoIdentity, RepoInfo, ResyncInfo, SyncStatus,
    SyncStatusKind,
};
pub use info_idx_resync::NextQueuedByHost;
pub use info_idx_state_count::RepoCountsByState;
pub use info_idx_status_count::RepoCountsByStatus;
pub use info_log_status::{AccountStatusEvent, AccountStatusUpstream};
pub use pending::PendingIdentity;
pub use pending_idx_retry::PendingIdentityQueueEntry;
pub use prev::{AccountSyncState, Sync11Outcome};

use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tracing::{debug, info, warn};

use crate::commit::CommitObject;
use crate::firehose::FirehoseCommit;
use crate::identity::Validity;
use crate::resync_scheduler::QueuedResync;
use crate::{
    Did, Host, HostRegistry, RepoSlot, RepoSlots, StorageBatch, StorageError, SyncConsumer, Tid,
};
use info_slot::InfoSlot;
use prev::Sync11DesyncReason;
use prev_slot::PrevSlot;

pub enum Awoken {
    Resolved(Repo),
    Pending(PendingIdentity),
    Refreshing {
        repo: Repo,
        pending: PendingIdentity,
    },
}

/// a repo's loaded app-state slots (commit, info)
type LoadedSlots<C, I> = (Option<C>, Option<I>);
type LoadResult<S, T> = Result<T, LoadError<<S as StorageEngine>::Error>>;

#[derive(Debug, Clone)]
pub struct Repo {
    did: Did,
    info: RepoInfo,
    sync: Option<AccountSyncState>,
}

impl Repo {
    // blocking!
    pub fn wake<S: StorageEngine, C: RepoSlot, I: RepoSlot>(
        storage: &S,
        hosts: &HostRegistry,
        did: Did,
        now: SystemTime,
    ) -> LoadResult<S, (Awoken, LoadedSlots<C, I>)> {
        // happy path: existing repos
        if let Some(info) = RepoInfo::load(storage, hosts, &did)? {
            let slots = Self::load_app_slots::<S, C, I>(&did, storage)?;
            let sync = AccountSyncState::load(storage, &did)?;
            let repo = Self {
                did: did.clone(),
                info,
                sync,
            };
            if repo.requires_identity_refresh(now) {
                // if we cannot move forward with the existing identity (hard-expired)
                let pending = PendingIdentity::load(storage, &did)?
                    .unwrap_or_else(|| PendingIdentity::new(did, now));
                return Ok((Awoken::Refreshing { repo, pending }, slots));
            }
            return Ok((Awoken::Resolved(repo), slots));
        }
        // back again path: initial identity resolution pending
        if let Some(p) = PendingIdentity::load(storage, &did)? {
            let slots = Self::load_app_slots::<S, C, I>(&did, storage)?;
            return Ok((Awoken::Pending(p), slots));
        }
        // first sight, brand new, for this identity
        Ok((
            Awoken::Pending(PendingIdentity::new(did, now)),
            (None, None),
        ))
    }

    /// bootstrap-promote constructor (pending resolved)
    pub fn create_resolved<E: StorageError, B: StorageBatch<E>>(
        did: Did,
        identity: RepoIdentity,
        status: AccountStatus,
        now: SystemTime,
        batch: &mut B,
    ) -> Self {
        let info = RepoInfo {
            upstream_status: status.clone(),
            moderation: None,
            sync_status: SyncStatus::Synchronized, // transitioned in a sec before commit
            identity,
            first_seen_at: now,
            resyncs: 0,
            pds_changes: 0,
            handle_changes: 0,
            last_resync: None,
            first_resync_needs_permit: None,
        };
        let mut repo = Self {
            did,
            info,
            sync: None,
        };

        let actual_sync_status = if status.is_active() {
            if repo.is_host_in_scope() {
                SyncStatus::Desynchronized(Desynchronized::new(
                    DesyncReason::FirstSeen,
                    now,
                    0,
                    None,
                ))
            } else {
                SyncStatus::OutOfScope { since: now }
            }
        } else if status.is_gone() {
            SyncStatus::Gone {
                since: now,
                reason: format!("create_resolved {}", status.kind().name()),
            }
        } else {
            SyncStatus::NonActive {
                since: now,
                rev: None,
            }
        };
        repo.init_sync_status(actual_sync_status, batch);
        AccountStatus::reconcile_for(None, Some(&repo.info.account_status()), batch);
        repo
    }

    /// a direct view into repo state (for reads only: bypasses actor system)
    pub(crate) fn load<S: StorageEngine>(
        storage: &S,
        hosts: &HostRegistry,
        did: &Did,
    ) -> LoadResult<S, Option<Self>> {
        let Some(info) = RepoInfo::load(storage, hosts, did)? else {
            return Ok(None);
        };
        let sync = AccountSyncState::load(storage, did)?;
        Ok(Some(Self {
            did: did.clone(),
            info,
            sync,
        }))
    }

    /// a direct view into multiple repos (for reads only: bypasses actors)
    pub(crate) fn list_synced<S: StorageEngine>(
        storage: &S,
        hosts: &HostRegistry,
        after: Option<&Did>,
        limit: NonZeroUsize,
    ) -> LoadResult<S, Vec<(Did, RepoInfo, AccountSyncState)>> {
        // strict next-possible-key for after
        let from = after
            .map(|prev_did| {
                let mut next_possible = prev_did.as_str().as_bytes().to_vec();
                next_possible.push(0x00);
                next_possible
            })
            .unwrap_or(vec![]);

        let mut repos = Vec::with_capacity(limit.into());
        for entry in AccountSyncState::scan(storage, &from).take(limit.into()) {
            let (did, sync_state) = entry?;
            let info = RepoInfo::load(storage, hosts, &did)?.ok_or_else(|| {
                LoadError::Integrity(format!("repo sync state without info for {did:?}"))
            })?;
            repos.push((did, info, sync_state));
        }
        Ok(repos)
    }

    /// bootstrap any app-slot state
    pub fn load_app_slots<S: StorageEngine, C: RepoSlot, I: RepoSlot>(
        did: &Did,
        storage: &S,
    ) -> LoadResult<S, LoadedSlots<C, I>> {
        let prev_slot = PrevSlot::load::<S, C>(did, storage)?;
        let info_slot = InfoSlot::load::<S, I>(did, storage)?;
        Ok((prev_slot, info_slot))
    }
    pub fn set_slots<S: StorageEngine, A: SyncConsumer<Engine = S>, B: StorageBatch<S::Error>>(
        &self,
        slots: RepoSlots<'_, A::CommitState, A::InfoState>,
        batch: &mut B,
    ) {
        if slots.commit.is_dirty() {
            if let Some(t) = slots.commit.get() {
                PrevSlot::store(self.did(), t, batch);
            } else {
                PrevSlot::delete(self.did(), batch);
            }
        }
        if slots.info.is_dirty() {
            if let Some(t) = slots.info.get() {
                InfoSlot::store(self.did(), t, batch);
            } else {
                InfoSlot::delete(self.did(), batch);
            }
        }
    }

    pub fn did(&self) -> &Did {
        &self.did
    }

    pub fn info(&self) -> &RepoInfo {
        &self.info
    }

    pub fn sync_state(&self) -> Option<&AccountSyncState> {
        self.sync.as_ref()
    }

    pub fn rev(&self) -> Option<Tid> {
        self.sync_state().map(|s| s.rev)
    }

    pub fn is_active(&self) -> bool {
        self.info.is_active()
    }

    pub fn is_in_scope(&self) -> bool {
        !matches!(self.info.sync_status, SyncStatus::OutOfScope { .. })
    }

    pub fn is_host_in_scope(&self) -> bool {
        self.info.identity.pds_host.in_scope()
    }

    pub fn is_upstream_active(&self) -> bool {
        self.info.upstream_status.is_active()
    }

    pub fn is_gone(&self) -> bool {
        self.info.account_status().is_gone()
    }

    pub fn is_synchronized(&self) -> bool {
        matches!(self.info.sync_status, SyncStatus::Synchronized)
    }

    pub fn pds_host(&self) -> Arc<Host> {
        self.info.identity.pds_host.clone()
    }

    pub fn current_desync(&self) -> Option<&Desynchronized> {
        let SyncStatus::Desynchronized(desync) = &self.info.sync_status else {
            return None;
        };
        Some(desync)
    }

    /// mark a repo as needing a permit to resync if, on its first resync, it
    /// acquired one (or tried to) and failed
    ///
    /// since we can't record `last_resync` for the hint on failure.
    ///
    /// noops if `last_resync` is present.
    pub fn set_needs_first_permit<E: StorageError, B: StorageBatch<E>>(&mut self, batch: &mut B) {
        if self.info.last_resync.is_some() {
            debug!("not setting needs_first_permit because `last_resync` is present");
            return;
        }
        self.info.first_resync_needs_permit = Some(());
        self.info.store(&self.did, batch);
    }

    /// Apply a validated (steps 1–4) commit to this repo
    ///
    /// (Steps 5 and 6 are applied in this process)
    ///
    /// https://www.ietf.org/archive/id/draft-holmgren-at-synchronization-00.html#section-4.5-3.5.1
    pub fn sync_next<E: StorageError, B: StorageBatch<E>>(
        &mut self,
        commit: &FirehoseCommit,
        now: SystemTime,
        batch: &mut B,
    ) -> Option<bool> {
        let Some(ref mut sync) = self.sync else {
            // first commit we have seen!

            // TODO: we should be able to distinguish first-commit-from-repo
            // from first-commit-we-happen-to-see-from-them: if it's truly a
            // repo's first commit, then should be able to proceed without
            // resync!

            // for now: just desynchronize
            // TODO: metrics, trace
            self.desynchronize(DesyncReason::FirstSeen, None, now, batch);
            return Some(false);
        };

        // 5, 6. validation steps applied here
        match sync.apply(&self.did, commit, now, batch) {
            Sync11Outcome::Drop(reason) => {
                debug!(did = %self.did, rev = %commit.rev, ?reason, "sync1.1 commit drop");
                // TODO: metric
                None
            }
            Sync11Outcome::Desync(Sync11DesyncReason::DoesNotFollow) => {
                let reason = DesyncReason::FirehoseFail {
                    bad_commit_rev: Some(commit.rev),
                };
                self.desynchronize(reason, None, now, batch);
                Some(false)
            }
            Sync11Outcome::Desync(Sync11DesyncReason::FutureRev) => {
                let reason = DesyncReason::FutureRev { rev: commit.rev };
                self.desynchronize(reason, None, now, batch);
                Some(false)
            }
            Sync11Outcome::Pass => Some(true),
        }
    }

    /// update repo info and sync state
    ///
    /// note: does *not* set the `last_resync` info -- that gets applied after
    /// the consumer app gets a chance to work with the resynchronized state.
    pub fn resynchronize<E: StorageError, B: StorageBatch<E>>(
        &mut self,
        reason: DesyncReason,
        commit: &CommitObject,
        now: SystemTime,
        batch: &mut B,
    ) -> bool {
        let sync = match AccountSyncState::reset(self.sync.as_ref(), commit, now) {
            Ok(s) => s,
            Err(err) => {
                debug!(did = %self.did, resync_reason = ?reason, ?err, "resync failed");
                // bumps count and pushes due_at
                self.desynchronize(reason, None, now, batch);
                return false;
            }
        };
        sync.store(self.did(), batch);
        self.sync = Some(sync);

        // TODO: probably put these counts in a method on info
        self.info.resyncs = self.info.resyncs.saturating_add(1);

        // set_sync_status reconciles (removes us from) the resync queue index
        self.set_sync_status(SyncStatus::Synchronized, batch);

        true
    }

    pub fn record_resync_info<E: StorageError, B: StorageBatch<E>>(
        &mut self,
        info: ResyncInfo,
        batch: &mut B,
    ) {
        self.info.last_resync = Some(info);
        self.info.first_resync_needs_permit = None; // exclusive with last_resync
        self.info.store(&self.did, batch);
    }

    pub fn desynchronize<E: StorageError, B: StorageBatch<E>>(
        &mut self,
        reason: DesyncReason,
        resync_error: Option<&'static str>,
        now: SystemTime,
        batch: &mut B,
    ) {
        // can't transition sync state if we're out-of-scope without risking
        // accidentally bringing a repo out of out-of-scope
        if let SyncStatus::OutOfScope { since } = self.info.sync_status {
            debug!(did = %self.did, ?reason, out_since = ?since, "ignoring desynchronize for out-of-scope repo");
            return;
        }

        let method = self.did().method();
        // TODO: resync_error should never be None except on the first desync
        // ..we could probably make that a bit more type-explicit
        let attempts = match &self.info().sync_status {
            SyncStatus::Desynchronized(d) => {
                if resync_error.is_none() {
                    warn!(did = %self.did, ?reason, prev_reason = ?d.reason, "possible bug: desynchronize called without error when already desynchronized");
                }
                if d.is_same_reason(&reason) && !d.error_changed(resync_error) {
                    d.resync_attempts.saturating_add(1)
                } else {
                    info!(
                        did = %self.did,
                        prev_count = d.resync_attempts,
                        prev_reason = ?d.reason,
                        prev_error = ?d.last_resync_error,
                        count = 1,
                        ?reason,
                        error = ?resync_error,
                        "desync reason or error changed, resetting attempt count");
                    1
                }
            }
            _ => 1,
        };
        let new_status = match Desynchronized::backoff(&reason, attempts, method) {
            Some(backoff) => SyncStatus::Desynchronized(Desynchronized::new(
                reason,
                now + backoff,
                attempts,
                resync_error,
            )),
            None => {
                debug!("desynchronizing -> repo Gone due to None backoff");
                // TODO: this currently leave the sync state in place, which is
                // on purpose, but make sure that exiting Gone either
                // desynchronizes (undelete) or doesn't (finally reachable)
                SyncStatus::Gone {
                    since: now,
                    reason: "resync backoff exhausted".to_string(),
                }
            }
        };
        self.set_sync_status(new_status, batch);
    }

    pub fn terminate<E: StorageError, B: StorageBatch<E>>(
        &mut self,
        reason: String,
        now: SystemTime,
        batch: &mut B,
    ) {
        // clear out the sync state
        AccountSyncState::delete(&self.did, batch);
        self.sync = None;

        // todo: tricky, for later: make sure transitions out of Gone either
        // desynchronize or not depending on the reason for leaving (deleted vs
        // retry-exhaustion)
        let new_status = SyncStatus::Gone { since: now, reason };
        self.set_sync_status(new_status, batch);
    }

    /// bump a resync's due_at (used to match an identity retry)
    pub fn delay_resync_until<E: StorageError, B: StorageBatch<E>>(
        &mut self,
        until: SystemTime,
        batch: &mut B,
    ) {
        if let SyncStatus::Desynchronized(d) = &self.info.sync_status {
            let mut delayed = d.clone();
            delayed.set_due(until);
            self.set_sync_status(SyncStatus::Desynchronized(delayed), batch);
        }
    }

    pub fn set_upstream_status<E: StorageError, B: StorageBatch<E>>(
        &mut self,
        event: AccountStatusEvent,
        batch: &mut B,
    ) -> AccountStatus {
        let prev = self.info.clone();
        self.info.upstream_status = event.status.clone();
        QueuedResync::reconcile_for(&self.did, &prev, &self.info, batch);
        AccountStatus::reconcile_for(
            Some(&prev.account_status()),
            Some(&self.info.account_status()),
            batch,
        );
        self.info.store(&self.did, batch);
        event.insert(batch);
        prev.upstream_status
    }

    pub fn set_moderation<E: StorageError, B: StorageBatch<E>>(
        &mut self,
        moderate: ModerateEvent,
        batch: &mut B,
    ) {
        let ModerateEvent {
            at,
            subject,
            action,
            source,
            message,
            reference,
        } = moderate;
        let prev_status = self.info.account_status();
        self.info.moderation = action.map(|a| Moderation { action: a, at });
        AccountStatus::reconcile_for(Some(&prev_status), Some(&self.info.account_status()), batch);
        // TODO: possibly need queued resync reconcilliation later
        self.info.store(&self.did, batch);
        super::moderation_log::ModerateEvent {
            at,
            subject,
            action,
            source,
            message,
            reference,
        }
        .insert(batch);
    }

    /// like set_sync_status but when no previous status exists (create_resolved)
    fn init_sync_status<E: StorageError, B: StorageBatch<E>>(
        &mut self,
        initial: SyncStatus,
        batch: &mut B,
    ) {
        let prev = self.info.clone();
        self.info.sync_status = initial;
        QueuedResync::reconcile_for(&self.did, &prev, &self.info, batch);
        SyncStatus::reconcile_for(None, Some(&self.info.sync_status), batch);
        self.info.store(&self.did, batch);
    }

    pub fn set_sync_status<E: StorageError, B: StorageBatch<E>>(
        &mut self,
        new_status: SyncStatus,
        batch: &mut B,
    ) -> SyncStatus {
        let prev = self.info.clone();
        self.info.sync_status = new_status;
        QueuedResync::reconcile_for(&self.did, &prev, &self.info, batch);
        SyncStatus::reconcile_for(Some(&prev.sync_status), Some(&self.info.sync_status), batch);
        self.info.store(&self.did, batch);
        prev.sync_status
    }

    pub fn set_identity<E: StorageError, B: StorageBatch<E>>(
        &mut self,
        new_id: RepoIdentity,
        now: SystemTime,
        batch: &mut B,
    ) -> RepoIdentity {
        let prev = self.info.clone();
        self.info.identity = new_id;
        QueuedResync::reconcile_for(&self.did, &prev, &self.info, batch);
        self.info.store(&self.did, batch);
        self.reconcile_scope(now, batch);
        prev.identity
    }

    // TODO: follow app style, don't do an ad-hoc manual reconcile helper here
    pub(crate) fn reconcile_scope<E: StorageError, B: StorageBatch<E>>(
        &mut self,
        now: SystemTime,
        batch: &mut B,
    ) {
        match (&self.info.sync_status, self.is_host_in_scope()) {
            (SyncStatus::OutOfScope { .. }, true) => {
                // departing out-of-scope (coming into scope)
                let status = SyncStatus::Desynchronized(Desynchronized::new(
                    DesyncReason::CameIntoScope,
                    now,
                    0,
                    None,
                ));
                self.set_sync_status(status, batch);
            }
            (SyncStatus::OutOfScope { .. }, false) => {} // noop
            (SyncStatus::Gone { .. }, _) => {}           // leave gone
            (_, false) => {
                // became out of scope (leaving in-scope)
                self.set_sync_status(SyncStatus::OutOfScope { since: now }, batch);
            }
            (_, true) => {} // in scope and tracked
        }
    }

    // TODO: rename to "should" or something
    pub fn needs_identity_refresh(&self, now: SystemTime) -> bool {
        let age = now
            .duration_since(self.info.identity.resolved_at)
            .unwrap_or(Duration::ZERO);
        Validity::for_identity(&self.did, age).should_refresh()
    }

    pub fn requires_identity_refresh(&self, now: SystemTime) -> bool {
        let age = now
            .duration_since(self.info.identity.resolved_at)
            .unwrap_or(Duration::ZERO);
        Validity::for_identity(&self.did, age).must_refresh()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::identity::SigningKey;
    use crate::storage::engine::mem::MemEngine;

    fn plc_did(label: &str) -> Did {
        let mut s = label.to_string();
        while s.len() < 24 {
            s.push('a');
        }
        s.truncate(24);
        Did::new(format!("did:plc:{s}")).expect("valid plc did")
    }

    fn web_did(host: &str) -> Did {
        Did::new(format!("did:web:{host}")).expect("valid web did")
    }

    fn registry() -> Arc<HostRegistry> {
        HostRegistry::new_default()
    }

    /// Anchor for the test suite's clock. Real wall-clock so identity-age
    /// offsets below relate to the same instant `wake`'s `now` parameter does.
    fn now() -> SystemTime {
        SystemTime::now()
    }

    /// Fixed clock anchor for deterministic boundary tests. Picked well past
    /// UNIX_EPOCH so that multi-year offsets backwards don't underflow into
    /// pre-epoch territory (the storage layer's `unix_ms_u64` serde panics
    /// on pre-epoch SystemTimes).
    fn fixed_now() -> SystemTime {
        std::time::UNIX_EPOCH + Duration::from_secs(1_700_000_000)
    }

    // Identity validity windows (must match values in `identity/validity.rs`).
    const PLC_VALID_FOR: Duration = Duration::from_secs(21_600); // 6h
    const PLC_EXPIRE_AT: Duration = Duration::from_secs(3 * 86_400); // 3d
    const WEB_VALID_FOR: Duration = Duration::from_secs(86_400); // 24h

    fn install_repo_info(
        storage: &MemEngine,
        hosts: &HostRegistry,
        did: &Did,
        resolved_at: SystemTime,
    ) {
        let pds_host = hosts.get("pds.example.com").expect("interned");
        let info = RepoInfo {
            upstream_status: AccountStatus::Active,
            moderation: None,
            sync_status: SyncStatus::Synchronized,
            identity: RepoIdentity {
                pds_host,
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

    fn install_pending(
        storage: &MemEngine,
        did: &Did,
        first_seen: SystemTime,
        reason: &str,
    ) -> PendingIdentity {
        // construct, store-failed once so it lands in pi| (and pq|)
        let mut p = PendingIdentity::new(did.clone(), first_seen);
        let mut b = storage.batch();
        p.store_failed(reason, first_seen, &mut b);
        b.commit().unwrap();
        p
    }

    fn sample_identity(hosts: &HostRegistry) -> RepoIdentity {
        RepoIdentity {
            pds_host: hosts.get("pds.example.com").expect("interned"),
            signing_key: SigningKey::raw(vec![0u8; 32]),
            supposed_handle: Some("alice.example".to_string()),
            resolved_at: fixed_now(),
        }
    }

    #[test]
    fn set_needs_first_permit_persists() {
        let eng = MemEngine::new();
        let reg = registry();
        let did = plc_did("permit");

        let mut b = eng.batch();
        let mut repo = Repo::create_resolved(
            did.clone(),
            sample_identity(&reg),
            AccountStatus::Active,
            fixed_now(),
            &mut b,
        );
        b.commit().unwrap();
        assert!(repo.info().first_resync_needs_permit.is_none());

        let mut b = eng.batch();
        repo.set_needs_first_permit(&mut b);
        b.commit().unwrap();
        assert_eq!(repo.info().first_resync_needs_permit, Some(()));

        // survives a reload from storage
        let reloaded = Repo::load(&eng, &reg, &did).unwrap().expect("stored");
        assert_eq!(reloaded.info().first_resync_needs_permit, Some(()));
    }

    #[test]
    fn record_resync_info_clears_needs_first_permit() {
        let eng = MemEngine::new();
        let reg = registry();
        let did = plc_did("permit");

        let mut b = eng.batch();
        let mut repo = Repo::create_resolved(
            did.clone(),
            sample_identity(&reg),
            AccountStatus::Active,
            fixed_now(),
            &mut b,
        );
        repo.set_needs_first_permit(&mut b);
        b.commit().unwrap();
        assert_eq!(repo.info().first_resync_needs_permit, Some(()));

        let mut b = eng.batch();
        repo.record_resync_info(
            ResyncInfo::new(fixed_now(), Some(1024), Some(10), None, None),
            &mut b,
        );
        b.commit().unwrap();
        assert!(repo.info().first_resync_needs_permit.is_none());
        assert!(repo.info().last_resync.is_some());
    }

    #[test]
    fn create_resolved_active_queues_a_first_sync() {
        let eng = MemEngine::new();
        let reg = registry();
        let did = plc_did("active");

        let mut b = eng.batch();
        let repo = Repo::create_resolved(
            did.clone(),
            sample_identity(&reg),
            AccountStatus::Active,
            fixed_now(),
            &mut b,
        );
        b.commit().unwrap();

        assert!(matches!(
            &repo.info().sync_status,
            SyncStatus::Desynchronized(d) if matches!(d.reason, DesyncReason::FirstSeen)
        ));
        assert_eq!(repo.info().upstream_status, AccountStatus::Active);
        assert_eq!(repo.info().account_status(), AccountStatus::Active);

        // active + in-scope: lands in the resync queue for its first sync
        let queued: Vec<Did> = NextQueuedByHost::new(&eng, &reg)
            .map(|r| r.expect("queue entry decodes").did)
            .collect();
        assert_eq!(queued, vec![did]);
    }

    #[test]
    fn create_resolved_takendown_is_gone_and_unqueued() {
        let eng = MemEngine::new();
        let reg = registry();
        let did = plc_did("takendown");

        let mut b = eng.batch();
        let repo = Repo::create_resolved(
            did.clone(),
            sample_identity(&reg),
            AccountStatus::Takendown,
            fixed_now(),
            &mut b,
        );
        b.commit().unwrap();

        assert!(matches!(repo.info().sync_status, SyncStatus::Gone { .. }));
        // the reported status is recorded, not clobbered back to Active
        assert_eq!(repo.info().upstream_status, AccountStatus::Takendown);
        assert_eq!(repo.info().account_status(), AccountStatus::Takendown);

        // gone: never fetched, so nothing in the resync queue
        let queued: Vec<Did> = NextQueuedByHost::new(&eng, &reg)
            .map(|r| r.expect("queue entry decodes").did)
            .collect();
        assert!(
            queued.is_empty(),
            "gone repo must not be queued: {queued:?}"
        );
    }

    #[test]
    fn create_resolved_deactivated_is_non_active_and_unqueued() {
        let eng = MemEngine::new();
        let reg = registry();
        let did = plc_did("deact");

        let mut b = eng.batch();
        let repo = Repo::create_resolved(
            did.clone(),
            sample_identity(&reg),
            AccountStatus::Deactivated,
            fixed_now(),
            &mut b,
        );
        b.commit().unwrap();

        assert!(matches!(
            repo.info().sync_status,
            SyncStatus::NonActive { .. }
        ));
        assert_eq!(repo.info().upstream_status, AccountStatus::Deactivated);
        assert_eq!(repo.info().account_status(), AccountStatus::Deactivated);

        let queued: Vec<Did> = NextQueuedByHost::new(&eng, &reg)
            .map(|r| r.expect("queue entry decodes").did)
            .collect();
        assert!(
            queued.is_empty(),
            "deactivated repo must not be queued: {queued:?}"
        );
    }

    #[test]
    fn set_needs_first_permit_noop_when_last_resync_present() {
        let eng = MemEngine::new();
        let reg = registry();
        let did = plc_did("permit");

        let mut b = eng.batch();
        let mut repo = Repo::create_resolved(
            did.clone(),
            sample_identity(&reg),
            AccountStatus::Active,
            fixed_now(),
            &mut b,
        );
        // a completed resync exists → the first-resync hint must stay unset
        repo.record_resync_info(
            ResyncInfo::new(fixed_now(), Some(1024), Some(10), None, None),
            &mut b,
        );
        b.commit().unwrap();
        assert!(repo.info().last_resync.is_some());

        let mut b = eng.batch();
        repo.set_needs_first_permit(&mut b);
        b.commit().unwrap();
        assert!(
            repo.info().first_resync_needs_permit.is_none(),
            "hint must stay unset while last_resync is present",
        );
    }

    #[test]
    fn wake_on_unseen_did_returns_fresh_pending() {
        let eng = MemEngine::new();
        let reg = registry();
        let did = plc_did("unseen");

        let awoken = Repo::wake::<_, (), ()>(&eng, &reg, did.clone(), now())
            .expect("wake ok")
            .0;
        match awoken {
            Awoken::Pending(p) => {
                assert_eq!(p.did, did);
                assert!(p.is_ready(now()), "fresh pending is ready immediately");
                // never seen -> on-disk lookup should still be empty
                assert!(PendingIdentity::load(&eng, &did).unwrap().is_none());
            }
            _ => panic!("expected Awoken::Pending(fresh)"),
        }
    }

    #[test]
    fn wake_on_stored_pending_only_returns_loaded_pending() {
        let eng = MemEngine::new();
        let reg = registry();
        let did = plc_did("recent_fail");
        install_pending(&eng, &did, now(), "first attempt failed");

        let awoken = Repo::wake::<_, (), ()>(&eng, &reg, did.clone(), now())
            .expect("wake ok")
            .0;
        match awoken {
            Awoken::Pending(p) => {
                assert_eq!(p.did, did);
                // loaded — next_try_at is in the future (backoff applied)
                assert!(
                    !p.is_ready(now()),
                    "loaded pending with backoff is not immediately ready"
                );
            }
            _ => panic!("expected Awoken::Pending(loaded)"),
        }
    }

    #[test]
    fn wake_on_valid_resolved_repo_returns_resolved() {
        let eng = MemEngine::new();
        let reg = registry();
        let did = plc_did("valid");
        // resolved_at == now → Valid (age=0)
        install_repo_info(&eng, &reg, &did, now());

        let awoken = Repo::wake::<_, (), ()>(&eng, &reg, did.clone(), now())
            .expect("wake ok")
            .0;
        match awoken {
            Awoken::Resolved(repo) => {
                assert_eq!(repo.did(), &did);
                assert!(!repo.needs_identity_refresh(now()));
                assert!(!repo.requires_identity_refresh(now()));
            }
            _ => panic!("expected Awoken::Resolved"),
        }
    }

    #[test]
    fn wake_on_stale_but_not_expired_repo_returns_resolved() {
        // Stale = should_refresh, but NOT must_refresh — wake's branch is
        // based on `requires_identity_refresh` (must_refresh), so stale
        // identities still go through the Resolved happy path.
        let eng = MemEngine::new();
        let reg = registry();
        let did = plc_did("stale");
        // 12h ago: past PLC valid (6h) but well within expire (3d) → Stale
        let stale_at = now() - Duration::from_secs(12 * 3600);
        install_repo_info(&eng, &reg, &did, stale_at);

        let awoken = Repo::wake::<_, (), ()>(&eng, &reg, did.clone(), now())
            .expect("wake ok")
            .0;
        match awoken {
            Awoken::Resolved(repo) => {
                assert!(
                    repo.needs_identity_refresh(now()),
                    "stale identity wants a refresh"
                );
                assert!(
                    !repo.requires_identity_refresh(now()),
                    "stale identity doesn't *require* refresh — still usable"
                );
            }
            _ => panic!("expected Awoken::Resolved (stale)"),
        }
    }

    #[test]
    fn wake_on_expired_repo_with_no_pending_returns_refreshing_fresh() {
        let eng = MemEngine::new();
        let reg = registry();
        let did = plc_did("expired");
        // 4 days ago: past PLC expire (3d) → Expired
        let expired_at = now() - Duration::from_secs(4 * 86_400);
        install_repo_info(&eng, &reg, &did, expired_at);

        let awoken = Repo::wake::<_, (), ()>(&eng, &reg, did.clone(), now())
            .expect("wake ok")
            .0;
        match awoken {
            Awoken::Refreshing { repo, pending } => {
                assert_eq!(repo.did(), &did);
                assert!(repo.requires_identity_refresh(now()));
                assert_eq!(pending.did, did);
                assert!(
                    pending.is_ready(now()),
                    "fresh in-memory pending is ready now"
                );
                // no pr| was loaded — fresh in-memory only
                assert!(PendingIdentity::load(&eng, &did).unwrap().is_none());
            }
            _ => panic!("expected Awoken::Refreshing (fresh pending)"),
        }
    }

    #[test]
    fn wake_on_expired_repo_with_pending_returns_refreshing_loaded() {
        let eng = MemEngine::new();
        let reg = registry();
        let did = plc_did("expired_retry");
        let expired_at = now() - Duration::from_secs(4 * 86_400);
        install_repo_info(&eng, &reg, &did, expired_at);
        install_pending(&eng, &did, now(), "previous resolve failed");
        let stored_loaded = PendingIdentity::load(&eng, &did)
            .unwrap()
            .expect("just stored");

        let awoken = Repo::wake::<_, (), ()>(&eng, &reg, did.clone(), now())
            .expect("wake ok")
            .0;
        match awoken {
            Awoken::Refreshing { repo, pending } => {
                assert_eq!(repo.did(), &did);
                // loaded pending: matches what load() would return — both
                // round-trip through the same ms-precision serde helpers.
                assert_eq!(pending.next_try_at(), stored_loaded.next_try_at());
                // and the backoff puts it in the future, so it's not yet ready
                assert!(!pending.is_ready(now()));
            }
            _ => panic!("expected Awoken::Refreshing (loaded pending)"),
        }
    }

    // --- clock-threaded boundary tests ---
    // These pin the exact validity-window transitions and the clock-skew
    // safety net, none of which were testable while validity read the wall
    // clock directly.

    #[test]
    fn wake_at_plc_valid_stale_boundary_returns_resolved_stale() {
        // resolved_at exactly PLC_VALID_FOR ago: age == VALID_FOR boundary,
        // tips into Stale (not Valid). Stale needs but does not *require*
        // refresh, so wake returns Resolved.
        let eng = MemEngine::new();
        let reg = registry();
        let did = plc_did("plc_v_s_boundary");
        let n = fixed_now();
        let resolved_at = n - PLC_VALID_FOR;
        install_repo_info(&eng, &reg, &did, resolved_at);

        let awoken = Repo::wake::<_, (), ()>(&eng, &reg, did.clone(), n)
            .expect("wake ok")
            .0;
        match awoken {
            Awoken::Resolved(repo) => {
                assert!(repo.needs_identity_refresh(n));
                assert!(!repo.requires_identity_refresh(n));
            }
            _ => panic!("expected Resolved at the valid/stale boundary"),
        }
    }

    #[test]
    fn wake_at_plc_stale_expired_boundary_returns_refreshing() {
        // resolved_at exactly PLC_EXPIRE_AT ago: age == EXPIRE_AT, tips
        // into Expired. wake produces Refreshing (must_refresh=true).
        let eng = MemEngine::new();
        let reg = registry();
        let did = plc_did("plc_s_e_boundary");
        let n = fixed_now();
        let resolved_at = n - PLC_EXPIRE_AT;
        install_repo_info(&eng, &reg, &did, resolved_at);

        let awoken = Repo::wake::<_, (), ()>(&eng, &reg, did.clone(), n)
            .expect("wake ok")
            .0;
        assert!(matches!(awoken, Awoken::Refreshing { .. }));
    }

    #[test]
    fn wake_on_year_old_web_identity_returns_resolved() {
        // did:web identities never reach Expired in the Validity table —
        // they're either Valid (within WEB_VALID_FOR) or Stale (forever after).
        // So wake should never produce Refreshing for did:web, no matter how
        // old the identity is.
        let eng = MemEngine::new();
        let reg = registry();
        let did = web_did("example.com");
        let n = fixed_now();
        let resolved_at = n - Duration::from_secs(365 * 86_400);
        install_repo_info(&eng, &reg, &did, resolved_at);

        let awoken = Repo::wake::<_, (), ()>(&eng, &reg, did.clone(), n)
            .expect("wake ok")
            .0;
        match awoken {
            Awoken::Resolved(repo) => {
                // age well past WEB_VALID_FOR → Stale → needs_refresh
                let _ = WEB_VALID_FOR;
                assert!(
                    repo.needs_identity_refresh(n),
                    "year-old did:web is stale (should_refresh=true)"
                );
                assert!(
                    !repo.requires_identity_refresh(n),
                    "did:web is never Expired, so requires_refresh is always false"
                );
            }
            _ => panic!("did:web must never produce Refreshing"),
        }
    }

    #[test]
    fn wake_with_resolved_at_in_the_future_treats_age_as_zero() {
        // Clock skew: a peer's resolved_at can be ahead of our `now`. The
        // `duration_since(resolved_at)` returns Err, and we `unwrap_or(ZERO)`,
        // so the identity reads as fresh-as-just-now → Resolved (Valid).
        let eng = MemEngine::new();
        let reg = registry();
        let did = plc_did("future_resolved");
        let n = fixed_now();
        let resolved_at = n + Duration::from_secs(60); // 1m ahead
        install_repo_info(&eng, &reg, &did, resolved_at);

        let awoken = Repo::wake::<_, (), ()>(&eng, &reg, did.clone(), n)
            .expect("wake ok")
            .0;
        match awoken {
            Awoken::Resolved(repo) => {
                assert!(!repo.needs_identity_refresh(n));
                assert!(!repo.requires_identity_refresh(n));
            }
            _ => panic!("future-resolved should look as fresh as just-resolved"),
        }
    }

    // --- app-owned slots (load_app_slots) ---

    #[derive(Debug, Clone, PartialEq)]
    struct CommitSlot(u32);
    impl RepoSlot for CommitSlot {
        fn encode(&self) -> Vec<u8> {
            self.0.to_be_bytes().to_vec()
        }
        fn decode(bytes: &[u8]) -> Result<Self, crate::SlotDecodeError> {
            let arr = bytes
                .try_into()
                .map_err(|_| crate::SlotDecodeError("commit slot: want 4 bytes".into()))?;
            Ok(CommitSlot(u32::from_be_bytes(arr)))
        }
    }

    #[derive(Debug, Clone, PartialEq)]
    struct InfoSlotVal(String);
    impl RepoSlot for InfoSlotVal {
        fn encode(&self) -> Vec<u8> {
            self.0.clone().into_bytes()
        }
        fn decode(bytes: &[u8]) -> Result<Self, crate::SlotDecodeError> {
            String::from_utf8(bytes.to_vec())
                .map(InfoSlotVal)
                .map_err(|e| crate::SlotDecodeError(e.to_string()))
        }
    }

    #[test]
    fn load_app_slots_round_trips_stored_slots() {
        let eng = MemEngine::new();
        let did = plc_did("slots");
        let mut b = eng.batch();
        // commit slot lives with the sync state; info slot with the repo info.
        PrevSlot::store(&did, &CommitSlot(42), &mut b);
        InfoSlot::store(&did, &InfoSlotVal("hello".into()), &mut b);
        b.commit().unwrap();

        let (commit, info) =
            Repo::load_app_slots::<_, CommitSlot, InfoSlotVal>(&did, &eng).expect("load ok");
        assert_eq!(commit, Some(CommitSlot(42)));
        assert_eq!(info, Some(InfoSlotVal("hello".into())));
    }

    #[test]
    fn load_app_slots_are_absent_when_unstored() {
        let eng = MemEngine::new();
        let (commit, info) =
            Repo::load_app_slots::<_, CommitSlot, InfoSlotVal>(&plc_did("empty"), &eng)
                .expect("load ok");
        assert_eq!(commit, None);
        assert_eq!(info, None);
    }

    #[test]
    fn unit_slots_store_nothing_and_load_none() {
        // the () slot is never present: store is a no-op, load is always None.
        let eng = MemEngine::new();
        let did = plc_did("unit");
        let mut b = eng.batch();
        PrevSlot::store(&did, &(), &mut b);
        InfoSlot::store(&did, &(), &mut b);
        b.commit().unwrap();
        let (commit, info) = Repo::load_app_slots::<_, (), ()>(&did, &eng).expect("load ok");
        assert_eq!(commit, None);
        assert_eq!(info, None);
    }

    // --- repo-count-by-sync-status (see info_idx_count) ---

    fn repo_identity(hosts: &HostRegistry, resolved_at: SystemTime) -> RepoIdentity {
        identity_on(hosts, "pds.example.com", resolved_at)
    }

    fn identity_on(hosts: &HostRegistry, host: &str, resolved_at: SystemTime) -> RepoIdentity {
        RepoIdentity {
            pds_host: hosts.get(host).expect("interned"),
            signing_key: SigningKey::raw(vec![0u8; 32]),
            supposed_handle: Some("alice.example".to_string()),
            resolved_at,
        }
    }

    fn count(eng: &MemEngine, suffix: &[u8]) -> i64 {
        eng.get_counter(&[PREFIX_REPO_INFO_IDX_COUNT, suffix].concat())
            .unwrap()
    }

    /// create a freshly-resolved repo (its "birth") and commit it, returning the
    /// in-memory `Repo` for driving further transitions.
    fn birth(eng: &MemEngine, hosts: &HostRegistry, label: &str) -> Repo {
        let n = now();
        let mut b = eng.batch();
        let repo = Repo::create_resolved(
            plc_did(label),
            repo_identity(hosts, n),
            AccountStatus::Active,
            n,
            &mut b,
        );
        b.commit().unwrap();
        repo
    }

    #[test]
    fn birth_counts_a_repo_as_desynchronized() {
        let eng = MemEngine::new();
        birth(&eng, &registry(), "a");
        assert_eq!(count(&eng, b"desynchronized|first_seen"), 1);
        assert_eq!(
            count(&eng, b"synchronized"),
            0,
            "the transient must not go negative"
        );
        assert_eq!(count(&eng, b"gone"), 0);
        assert_eq!(count(&eng, b"deactivated"), 0);
    }

    #[test]
    fn resync_moves_desync_to_synchronized() {
        let eng = MemEngine::new();
        let mut repo = birth(&eng, &registry(), "a");
        let mut b = eng.batch();
        repo.set_sync_status(SyncStatus::Synchronized, &mut b);
        b.commit().unwrap();
        assert_eq!(count(&eng, b"desynchronized|first_seen"), 0);
        assert_eq!(count(&eng, b"synchronized"), 1);
    }

    #[test]
    fn reschedule_leaves_counts_unchanged() {
        // Desync -> Desync stays in one bucket: a no-op. this is exactly what the
        // inverted-condition bug got wrong (it re-incremented desynchronized).
        let eng = MemEngine::new();
        let mut repo = birth(&eng, &registry(), "a");
        let mut b = eng.batch();
        repo.delay_resync_until(now() + Duration::from_secs(60), &mut b);
        b.commit().unwrap();
        assert_eq!(count(&eng, b"desynchronized|first_seen"), 1);
        assert_eq!(count(&eng, b"synchronized"), 0);
    }

    #[test]
    fn real_desync_decrements_the_previous_bucket() {
        let eng = MemEngine::new();
        let mut repo = birth(&eng, &registry(), "a");
        // to synchronized...
        let mut b = eng.batch();
        repo.set_sync_status(SyncStatus::Synchronized, &mut b);
        b.commit().unwrap();
        // ...then a genuine (non-FirstSeen) desync back down.
        let mut b = eng.batch();
        repo.set_sync_status(
            SyncStatus::Desynchronized(Desynchronized::new(
                DesyncReason::FirehoseAccountDesynchronized,
                now(),
                1,
                None,
            )),
            &mut b,
        );
        b.commit().unwrap();
        assert_eq!(
            count(&eng, b"synchronized"),
            0,
            "the real previous bucket is decremented"
        );
        assert_eq!(count(&eng, b"desynchronized|firehose_account_desync"), 1);
    }

    #[test]
    fn terminate_moves_a_repo_to_gone() {
        let eng = MemEngine::new();
        let mut repo = birth(&eng, &registry(), "a");
        let mut b = eng.batch();
        repo.terminate("deleted".to_string(), now(), &mut b);
        b.commit().unwrap();
        assert_eq!(count(&eng, b"desynchronized|first_seen"), 0);
        assert_eq!(count(&eng, b"gone"), 1);
    }

    #[test]
    fn account_status_change_does_not_move_sync_counts() {
        // counts are bucketed by sync_status only; account status is orthogonal.
        let eng = MemEngine::new();
        let mut repo = birth(&eng, &registry(), "a");
        let mut b = eng.batch();
        repo.set_upstream_status(
            AccountStatusEvent {
                subject: plc_did("adsf"),
                at: SystemTime::now(),
                upstream: None,
                status: AccountStatus::Takendown,
            },
            &mut b,
        );
        b.commit().unwrap();
        assert_eq!(count(&eng, b"desynchronized|first_seen"), 1);
    }

    #[test]
    fn counts_sum_across_repos() {
        let eng = MemEngine::new();
        let reg = registry();
        birth(&eng, &reg, "a");
        birth(&eng, &reg, "b");
        let mut c = birth(&eng, &reg, "c");
        let mut b = eng.batch();
        c.set_sync_status(SyncStatus::Synchronized, &mut b);
        b.commit().unwrap();
        assert_eq!(count(&eng, b"desynchronized|first_seen"), 2);
        assert_eq!(count(&eng, b"synchronized"), 1);
    }

    // --- sync scope (OutOfScope / reconcile_scope) ---

    const BSKY_PDS: &str = "test.host.bsky.network";
    const OTHER_BSKY_PDS: &str = "other.host.bsky.network";
    const INDIE_PDS: &str = "pds.example.com";

    /// a registry where only bsky pds hosts are in scope
    fn scoped_registry() -> Arc<HostRegistry> {
        HostRegistry::new(
            Default::default(),
            Default::default(),
            "",
            tokio_util::sync::CancellationToken::new(),
            crate::SyncScope::BskyPdsOnly,
        )
    }

    /// create_resolved on the given pds host (scope comes from the registry)
    fn birth_on(eng: &MemEngine, hosts: &HostRegistry, label: &str, host: &str) -> Repo {
        let n = now();
        let mut b = eng.batch();
        let repo = Repo::create_resolved(
            plc_did(label),
            identity_on(hosts, host, n),
            AccountStatus::Active,
            n,
            &mut b,
        );
        b.commit().unwrap();
        repo
    }

    fn queued_for(eng: &MemEngine, hosts: &HostRegistry, host: &str) -> bool {
        let host = hosts.get(host).expect("interned");
        QueuedResync::peek(eng, Some(host)).expect("peek").is_some()
    }

    #[test]
    fn scoped_birth_out_of_scope_parks_without_resync_queue() {
        let eng = MemEngine::new();
        let reg = scoped_registry();
        let repo = birth_on(&eng, &reg, "a", INDIE_PDS);

        assert!(matches!(
            repo.info().sync_status,
            SyncStatus::OutOfScope { .. }
        ));
        assert_eq!(count(&eng, b"outOfScope"), 1);
        assert_eq!(count(&eng, b"desynchronized|first_seen"), 0);
        assert_eq!(
            count(&eng, b"synchronized"),
            0,
            "the creation placeholder must not be decremented",
        );
        assert!(!queued_for(&eng, &reg, INDIE_PDS));
    }

    #[test]
    fn scoped_birth_in_scope_desyncs_first_seen_and_queues() {
        let eng = MemEngine::new();
        let reg = scoped_registry();
        let repo = birth_on(&eng, &reg, "a", BSKY_PDS);

        match &repo.info().sync_status {
            SyncStatus::Desynchronized(d) => {
                assert!(matches!(d.reason, DesyncReason::FirstSeen))
            }
            other => panic!("expected first-seen desync, got {other:?}"),
        }
        assert_eq!(count(&eng, b"desynchronized|first_seen"), 1);
        assert_eq!(count(&eng, b"outOfScope"), 0);
        assert!(queued_for(&eng, &reg, BSKY_PDS));
    }

    #[test]
    fn desynchronize_ignored_while_out_of_scope() {
        let eng = MemEngine::new();
        let reg = scoped_registry();
        let mut repo = birth_on(&eng, &reg, "a", INDIE_PDS);

        let mut b = eng.batch();
        repo.desynchronize(
            DesyncReason::FirehoseAccountDesynchronized,
            None,
            now(),
            &mut b,
        );
        b.commit().unwrap();

        assert!(
            matches!(repo.info().sync_status, SyncStatus::OutOfScope { .. }),
            "out-of-scope is sticky against desynchronize",
        );
        assert_eq!(count(&eng, b"outOfScope"), 1);
        assert_eq!(count(&eng, b"desynchronized|firehose_account_desync"), 0);
        assert!(!queued_for(&eng, &reg, INDIE_PDS));
    }

    #[test]
    fn coming_into_scope_queues_a_came_into_scope_resync() {
        let eng = MemEngine::new();
        let reg = scoped_registry();
        let mut repo = birth_on(&eng, &reg, "a", INDIE_PDS);

        // identity refresh discovers a migration onto a bsky pds
        let n = now();
        let mut b = eng.batch();
        repo.set_identity(identity_on(&reg, BSKY_PDS, n), n, &mut b);
        b.commit().unwrap();

        match &repo.info().sync_status {
            SyncStatus::Desynchronized(d) => {
                assert!(matches!(d.reason, DesyncReason::CameIntoScope))
            }
            other => panic!("expected came-into-scope desync, got {other:?}"),
        }
        assert_eq!(count(&eng, b"outOfScope"), 0);
        assert_eq!(count(&eng, b"desynchronized|came_into_scope"), 1);
        assert!(queued_for(&eng, &reg, BSKY_PDS));
    }

    #[test]
    fn leaving_scope_parks_and_dequeues() {
        let eng = MemEngine::new();
        let reg = scoped_registry();
        // in-scope birth: desynchronized(first_seen) + queued under the bsky host
        let mut repo = birth_on(&eng, &reg, "a", BSKY_PDS);
        assert!(queued_for(&eng, &reg, BSKY_PDS));

        // identity refresh discovers a migration off to an indie pds
        let n = now();
        let mut b = eng.batch();
        repo.set_identity(identity_on(&reg, INDIE_PDS, n), n, &mut b);
        b.commit().unwrap();

        assert!(matches!(
            repo.info().sync_status,
            SyncStatus::OutOfScope { .. }
        ));
        assert_eq!(count(&eng, b"desynchronized|first_seen"), 0);
        assert_eq!(count(&eng, b"outOfScope"), 1);
        assert!(!queued_for(&eng, &reg, BSKY_PDS), "old host entry removed");
        assert!(!queued_for(&eng, &reg, INDIE_PDS));
    }

    #[test]
    fn leaving_scope_from_synchronized_decrements_synchronized() {
        // the transition the old creation-inference hack would have miscounted:
        // a *genuine* synchronized -> out-of-scope move must decrement.
        let eng = MemEngine::new();
        let reg = scoped_registry();
        let mut repo = birth_on(&eng, &reg, "a", BSKY_PDS);
        let mut b = eng.batch();
        repo.set_sync_status(SyncStatus::Synchronized, &mut b);
        b.commit().unwrap();
        assert_eq!(count(&eng, b"synchronized"), 1);

        let n = now();
        let mut b = eng.batch();
        repo.set_identity(identity_on(&reg, INDIE_PDS, n), n, &mut b);
        b.commit().unwrap();

        assert!(matches!(
            repo.info().sync_status,
            SyncStatus::OutOfScope { .. }
        ));
        assert_eq!(count(&eng, b"synchronized"), 0);
        assert_eq!(count(&eng, b"outOfScope"), 1);
    }

    #[test]
    fn in_scope_pds_change_keeps_status() {
        let eng = MemEngine::new();
        let reg = scoped_registry();
        let mut repo = birth_on(&eng, &reg, "a", BSKY_PDS);
        let mut b = eng.batch();
        repo.set_sync_status(SyncStatus::Synchronized, &mut b);
        b.commit().unwrap();

        // moving between two in-scope hosts is not a scope transition
        let n = now();
        let mut b = eng.batch();
        repo.set_identity(identity_on(&reg, OTHER_BSKY_PDS, n), n, &mut b);
        b.commit().unwrap();

        assert!(matches!(repo.info().sync_status, SyncStatus::Synchronized));
        assert_eq!(count(&eng, b"synchronized"), 1);
        assert_eq!(count(&eng, b"outOfScope"), 0);
    }

    #[test]
    fn gone_ignores_scope_flips() {
        let eng = MemEngine::new();
        let reg = scoped_registry();
        let mut repo = birth_on(&eng, &reg, "a", BSKY_PDS);
        let mut b = eng.batch();
        repo.terminate("deleted".to_string(), now(), &mut b);
        b.commit().unwrap();
        assert_eq!(count(&eng, b"gone"), 1);

        // scope flips in either direction leave a gone repo alone
        let n = now();
        let mut b = eng.batch();
        repo.set_identity(identity_on(&reg, INDIE_PDS, n), n, &mut b);
        b.commit().unwrap();
        assert!(matches!(repo.info().sync_status, SyncStatus::Gone { .. }));

        let mut b = eng.batch();
        repo.set_identity(identity_on(&reg, BSKY_PDS, n), n, &mut b);
        b.commit().unwrap();
        assert!(matches!(repo.info().sync_status, SyncStatus::Gone { .. }));

        assert_eq!(count(&eng, b"gone"), 1);
        assert_eq!(count(&eng, b"outOfScope"), 0);
        assert!(!queued_for(&eng, &reg, BSKY_PDS));
        assert!(!queued_for(&eng, &reg, INDIE_PDS));
    }
}
