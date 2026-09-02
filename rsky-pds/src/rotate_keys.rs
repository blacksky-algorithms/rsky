// based on https://github.com/bluesky-social/atproto/blob/main/packages/pds/src/scripts/rotate-keys.ts
// moves existing accounts off the shared repo signing key onto their own

use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::plc;
use crate::sequencer::Sequencer;
use anyhow::{anyhow, Result};
use rsky_crypto::utils::encode_did_key;
use secp256k1::{Keypair, Secp256k1, SecretKey};
use tokio::sync::RwLock;

/// Everything a rotation run needs. Borrowed rather than owned so the job can
/// run against a live process's stores as well as a standalone binary's.
pub struct RotateKeysContext<'a> {
    pub actor_store: &'a ActorStore,
    pub account_manager: &'a AccountManager,
    pub blobstore_factory: &'a BlobstoreFactory,
    pub sequencer: &'a RwLock<Sequencer>,
    pub plc_client: &'a plc::Client,
    pub plc_rotation_key: &'a SecretKey,
    /// The legacy shared repo signing key. An account whose key file still
    /// holds it has never been rotated.
    pub shared_signing_key: &'a Keypair,
}

pub struct RotateKeysOpts {
    /// Rotate only these DIDs; `None` sweeps every account on this PDS.
    pub dids: Option<Vec<String>>,
    /// Report what would change without writing a key, publishing a PLC
    /// operation or emitting an event.
    pub dry_run: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RotateKeysReport {
    pub scanned: usize,
    pub rotated: usize,
    pub skipped: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// Only did:plc identities can be re-keyed by this server.
    NotPlcDid,
    /// The DID document already names the account's own key.
    AlreadyRotated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotationOutcome {
    Skipped(SkipReason),
    Rotated,
}

pub async fn list_account_dids(account_manager: &AccountManager) -> Result<Vec<String>> {
    account_manager
        .db
        .run(|conn| {
            let mut stmt = conn.prepare("SELECT did FROM actor ORDER BY did")?;
            let rows = stmt
                .query_map([], |row| row.get(0))?
                .collect::<Result<Vec<String>, rusqlite::Error>>()?;
            Ok(rows)
        })
        .await
}

/// Re-keys accounts one at a time. Deliberately not batched: each DID's PLC
/// operation is followed straight away by the commit that re-signs its repo,
/// so the window where the published key and the repo head disagree stays as
/// short as possible.
pub async fn rotate_keys(
    ctx: &RotateKeysContext<'_>,
    opts: RotateKeysOpts,
) -> Result<RotateKeysReport> {
    let dids = match opts.dids {
        Some(dids) => dids,
        None => list_account_dids(ctx.account_manager).await?,
    };
    let mut report = RotateKeysReport::default();
    for did in dids {
        report.scanned += 1;
        match rotate_one(ctx, &did, opts.dry_run).await {
            Ok(RotationOutcome::Skipped(reason)) => {
                report.skipped += 1;
                tracing::debug!(%did, ?reason, "skipping signing key rotation");
            }
            Ok(RotationOutcome::Rotated) => {
                report.rotated += 1;
                tracing::info!(%did, dry_run = opts.dry_run, "rotated signing key");
            }
            Err(error) => {
                report.failed += 1;
                tracing::error!(%did, %error, "failed to rotate signing key");
            }
        }
    }
    Ok(report)
}

pub async fn rotate_one(
    ctx: &RotateKeysContext<'_>,
    did: &str,
    dry_run: bool,
) -> Result<RotationOutcome> {
    if !did.starts_with("did:plc:") {
        return Ok(RotationOutcome::Skipped(SkipReason::NotPlcDid));
    }
    let published = ctx
        .plc_client
        .get_document_data(&did.to_string())
        .await?
        .verification_methods
        .get("atproto")
        .cloned()
        .ok_or_else(|| anyhow!("no atproto verification method published for {did}"))?;
    let current = ctx.actor_store.keypair(did).await?;
    let current_did_key = encode_did_key(&current.public_key());
    let shared_did_key = encode_did_key(&ctx.shared_signing_key.public_key());

    // Resolve and compare: an account whose published key is already its own
    // is finished. This is what makes a run resumable over thousands of DIDs.
    if current_did_key != shared_did_key && published == current_did_key {
        return Ok(RotationOutcome::Skipped(SkipReason::AlreadyRotated));
    }
    if dry_run {
        return Ok(RotationOutcome::Rotated);
    }

    // A key file that already moved off the shared key came from an
    // interrupted run; republish it rather than burning another PLC op.
    let keypair = if current_did_key == shared_did_key {
        let keypair = Keypair::new(&Secp256k1::new(), &mut rand::thread_rng());
        ctx.actor_store.set_keypair(did, &keypair).await?;
        keypair
    } else {
        current
    };
    let signing_key = encode_did_key(&keypair.public_key());
    ctx.plc_client
        .update_atproto_key(&did.to_string(), ctx.plc_rotation_key, &signing_key)
        .await?;

    // An empty commit re-signs the repo head with the new key.
    let (commit, sync_data) = {
        let mut actor_txn = ctx
            .actor_store
            .transact(
                did.to_string(),
                ctx.blobstore_factory.blobstore(did.to_string()),
            )
            .await?;
        let commit = actor_txn.process_writes(vec![], None).await?;
        let sync_data = actor_txn.get_sync_event_data().await?;
        (commit, sync_data)
    };
    ctx.account_manager
        .update_repo_root(
            did.to_string(),
            commit.commit_data.cid,
            commit.commit_data.rev.clone(),
        )
        .await?;

    let mut sequencer = ctx.sequencer.write().await;
    sequencer
        .sequence_identity_evt(did.to_string(), None)
        .await?;
    sequencer
        .sequence_sync_evt(did.to_string(), sync_data)
        .await?;
    Ok(RotationOutcome::Rotated)
}

#[cfg(test)]
mod tests;
