//! route a `ReconcileScope` message to every [`SyncStatus::OutOfScope`] repo
//!
//! one-shot at startup

use std::sync::Arc;
use std::time::Duration;

use tokio::task::spawn_blocking;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::cancel::CancelExt;
use crate::repo_actor::{RepoMessage, RepoRegistry};
use crate::storage::repo::RepoInfo;
use crate::{
    Did, LoadError, PrefixedEngine, RepoSendError, Resolve, StorageEngine, StorageError,
    SyncConsumer,
};

/// how long to wait before retrying a send when a repo's mailbox is full.
const SEND_BACKOFF: Duration = Duration::from_secs(1);

/// log sweep progress every this many sent messages
const PROGRESS_EVERY: u64 = 50_000;

#[derive(Debug, thiserror::Error)]
pub enum RescopeSweepError<E: StorageError> {
    #[error("scanning repo info: {0}")]
    Scan(#[source] LoadError<E>),

    #[error("repo registry eviction stuck: cannot route reconcile-scope")]
    EvictionStuck,

    #[error("rescope sweep scan task panicked")]
    ScanPanic(#[source] tokio::task::JoinError),
}

/// scan every stored repo and send `ReconcileScope` to every out-of-scope one
pub async fn run<S, A, R>(
    storage: PrefixedEngine<S>,
    repo_registry: Arc<RepoRegistry<S, A, R>>,
    cancel: CancellationToken,
) -> Result<(), RescopeSweepError<S::Error>>
where
    S: StorageEngine,
    A: SyncConsumer<Engine = S>,
    R: Resolve,
{
    // producer: blocking scan of the whole keyspace, streaming out-of-scope DIDs
    // over a bounded channel so memory stays flat however many there are.
    // `blocking_send` naturally backpressures the scan when the consumer stalls.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Did>(1024);
    let scan_storage = storage.clone();
    let scan_cancel = cancel.clone();
    let producer = spawn_blocking(move || -> Result<u64, LoadError<S::Error>> {
        let mut found = 0u64;
        for item in RepoInfo::scan_out_of_scope(&scan_storage) {
            if scan_cancel.is_cancelled() {
                break;
            }
            let did = item?;
            found += 1;
            if tx.blocking_send(did).is_err() {
                break; // consumer gone
            }
        }
        Ok(found)
    });

    // consumer: route each DID to its actor for an actor-owned reconcile.
    let mut sent = 0u64;
    while let Some(did) = rx.recv().await {
        loop {
            match repo_registry.try_send(&did, RepoMessage::ReconcileScope) {
                Ok(()) => {
                    sent += 1;
                    if sent.is_multiple_of(PROGRESS_EVERY) {
                        info!(sent, "rescope sweep progress");
                    }
                    break;
                }
                Err(RepoSendError::Draining(_)) => {
                    info!(sent, "rescope sweep stopping: repo registry draining");
                    return Ok(());
                }
                Err(RepoSendError::Backpressure(_)) => {
                    // mailbox full: back off and retry the same repo
                    if !cancel.sleep(SEND_BACKOFF).await {
                        return Ok(()); // cancelled
                    }
                }
                Err(RepoSendError::EvictionStuck(_)) => {
                    warn!(%did, "rescope sweep aborting: repo registry eviction stuck");
                    return Err(RescopeSweepError::EvictionStuck);
                }
            }
        }
    }

    let found = producer
        .await
        .map_err(RescopeSweepError::ScanPanic)?
        .map_err(RescopeSweepError::Scan)?;
    info!(found, sent, "rescope sweep complete");
    Ok(())
}
