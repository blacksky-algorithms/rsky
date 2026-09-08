//! initial identity resolution for newly-seen DIDs
//!
//! when waking the repo produces `Awoken::Pending`, we end up here, before
//! the actor can go to its normal task loop. if we fail, it stays pending and
//! will try again on its next wake.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tokio::sync::Semaphore;
use tokio::task::spawn_blocking;
use tokio_util::sync::CancellationToken;

use crate::identity::Resolve;
use crate::storage::engine::{PrefixedEngine, StorageBatch, StorageEngine};
use crate::storage::repo::{AccountStatus, PendingIdentity, Repo};
use crate::{Host, SensitiveToken, SyncConsumer, UpstreamKind};

use super::{ProcessError, TaskProcessor};

pub(super) struct InitialResolve<S: StorageEngine, A: SyncConsumer<Engine = S>, R: Resolve> {
    pub(super) storage: PrefixedEngine<S>,
    pub(super) resolver: Arc<R>,
    pub(super) pending: PendingIdentity,
    pub(super) upstream: Arc<Host>,
    pub(super) upstream_kind: UpstreamKind,
    pub(super) upstream_get_repo_token: Option<SensitiveToken>,
    pub(super) consumer_app: Arc<A>,
    pub(super) cancel: CancellationToken,
    pub(super) big_repo: Arc<Semaphore>,
    pub(super) reactive_permit_wait: Duration,
    pub(super) spill_dir: Arc<Path>,
    pub(super) now: SystemTime,
}

pub(super) enum InitialResolveOutcome<S: StorageEngine, A: SyncConsumer<Engine = S>, R: Resolve> {
    /// not time to retry yet, nothing happened
    Snooze,
    /// tried and failed and should try again
    Retry,
    /// aw yea we are up!
    GetUp(TaskProcessor<S, A, R>),
}

impl<S: StorageEngine, A: SyncConsumer<Engine = S>, R: Resolve> InitialResolve<S, A, R> {
    pub(super) async fn run(
        self,
    ) -> Result<InitialResolveOutcome<S, A, R>, ProcessError<S::Error>> {
        let Self {
            storage,
            resolver,
            mut pending,
            upstream,
            upstream_kind,
            upstream_get_repo_token,
            consumer_app,
            cancel,
            big_repo,
            reactive_permit_wait,
            spill_dir,
            now,
        } = self;

        if !pending.is_ready(now) {
            return Ok(InitialResolveOutcome::Snooze);
        }

        let did = pending.did.clone();

        match resolver.resolve(&did, now).await {
            Ok(identity) => {
                let storage_inner = storage.clone();
                let did = did.clone();

                let repo = spawn_blocking(move || {
                    let mut batch = storage_inner.batch();
                    let effective_account_status = match pending.moderation() {
                        Some((status, source)) if source.is_authoritative_for(&identity) => {
                            status.clone()
                        }
                        _ => AccountStatus::Active,
                    };
                    let repo = Repo::create_resolved(
                        did.clone(),
                        identity,
                        effective_account_status,
                        now,
                        &mut batch,
                    );
                    pending.delete(&mut batch);
                    batch.commit()?;
                    Ok(repo)
                })
                .await
                .expect("storage not to panic")
                .map_err(ProcessError::Storage)?;

                Ok(InitialResolveOutcome::GetUp(TaskProcessor {
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
                    commit_slot: None,
                    info_slot: None,
                }))
            }
            Err(err) => {
                // TODO: handle different kinds of identity error??
                let storage = storage.clone();
                spawn_blocking(move || {
                    let mut batch = storage.batch();
                    pending.store_failed(&format!("failed to resolve: {err}"), now, &mut batch);
                    batch.commit()?;
                    Ok(())
                })
                .await
                .expect("storage not to panic")
                .map_err(ProcessError::Storage)?;
                Ok(InitialResolveOutcome::Retry)
            }
        }
    }
}
