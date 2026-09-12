//! What an actor still owes this process, and the maintenance drain that
//! finishes it under an exclusive lock.

use crate::actor_store::blob::BlobReader;
use crate::actor_store::blobstore::{unavailable, BlobStore};
use crate::actor_store::{pending_intents_in, ActorStore};
use crate::locks::LockDir;
use crate::publication::{publish_pending, settle_pending_work};
use crate::repair::RepairStore;
use crate::SharedSequencer;
use anyhow::Result;
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;

/// The per-actor counters behind the drain and hand-back decisions.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DrainStatus {
    pub did: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow_id: Option<String>,
    pub inflight_mutations: usize,
    pub publish_intent_pending: usize,
    pub blob_work_nonterminal: usize,
    pub lifecycle_pending: usize,
    pub repair_pending: usize,
    /// No client write in flight and nothing left to publish: the
    /// precondition for a repair or quarantine step.
    pub client_quiescent: bool,
    /// Client-quiescent with no blob, lifecycle, or repair work outstanding:
    /// the precondition for handing the actor to another writer.
    pub fully_drained: bool,
}

fn table_exists(conn: &rusqlite::Connection, name: &str) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [name],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// `(pending intents, non-terminal blob work)` of a store; a store another
/// implementation created has neither journal and owes nothing.
async fn journal_counts(actor_store: &ActorStore, did: &str) -> Result<(usize, usize)> {
    if !actor_store.exists(did).await? {
        return Ok((0, 0));
    }
    let reader = actor_store.read(did.to_owned(), unavailable()).await?;
    let db = reader.record.db.clone();
    let journaled = db
        .run(|conn| Ok(table_exists(conn, "publish_intent")? && table_exists(conn, "blob_work")?))
        .await?;
    if !journaled {
        return Ok((0, 0));
    }
    let intents = db.run(|conn| pending_intents_in(conn)).await?.len();
    let blob_work = reader.blob.nonterminal_blob_work().await?;
    Ok((intents, blob_work))
}

pub async fn drain_status(
    actor_store: &ActorStore,
    repairs: &RepairStore,
    did: &str,
) -> Result<DrainStatus> {
    let state = actor_store.admission.state_of(did);
    let (publish_intent_pending, blob_work_nonterminal) = journal_counts(actor_store, did).await?;
    let lifecycle_pending = actor_store
        .lifecycle
        .tombstone_of(did)
        .await?
        .filter(|tombstone| tombstone.logically_deleted_at.is_none())
        .map(|_| 1)
        .unwrap_or(0);
    let repair_pending =
        repairs.pending_for(did).await?.len() + repairs.open_quarantines_for(did).await?.len();
    let inflight_mutations = actor_store.inflight_mutations(did);
    let client_quiescent = inflight_mutations == 0 && publish_intent_pending == 0;
    let fully_drained = client_quiescent
        && blob_work_nonterminal == 0
        && lifecycle_pending == 0
        && repair_pending == 0;
    Ok(DrainStatus {
        did: did.to_owned(),
        workflow_id: state.workflow_id().map(str::to_owned),
        state: state.name().to_owned(),
        inflight_mutations,
        publish_intent_pending,
        blob_work_nonterminal,
        lifecycle_pending,
        repair_pending,
        client_quiescent,
        fully_drained,
    })
}

/// Waits for the actor's in-flight writes to finish, then delivers its
/// intents and runs its blob work while holding the actor's lock
/// exclusively, so nothing new starts underneath.
pub async fn drain_did(
    actor_store: &ActorStore,
    sequencer: &SharedSequencer,
    repairs: &RepairStore,
    lock_dir: &LockDir,
    blobstore: Arc<dyn BlobStore>,
    did: &str,
    timeout: Duration,
) -> Result<DrainStatus> {
    let _exclusive = lock_dir.exclusive_within(did, timeout).await?;
    if actor_store.admission.admit_worker(did).is_ok() {
        publish_pending(actor_store, sequencer, did, None).await?;
        if actor_store.exists(did).await? {
            let db = actor_store.write_db(did).await?;
            let blob = BlobReader::new(
                blobstore,
                db.clone(),
                actor_store.background_queue.clone(),
                actor_store.coexistence,
            );
            blob.run_blob_work().await?;
            settle_pending_work(&actor_store.lifecycle, &db, &blob, did).await?;
        }
    }
    drain_status(actor_store, repairs, did).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor_store::blobstore::MemoryBlobStore;
    use crate::admission::Admission;
    use crate::background::BackgroundQueue;
    use crate::config::ActorStoreConfig;
    use crate::crawlers::Crawlers;
    use crate::lifecycle::LifecycleStore;
    use crate::sequencer::Sequencer;
    use rsky_repo::types::{PreparedCreateOrUpdate, PreparedWrite, WriteOpAction};
    use secp256k1::{Keypair, Secp256k1, SecretKey};
    use tokio::sync::RwLock;

    const DID: &str = "did:plc:drain";

    struct World {
        dir: tempfile::TempDir,
        actor_store: ActorStore,
        sequencer: SharedSequencer,
        repairs: RepairStore,
        lock_dir: LockDir,
        blobstore: Arc<MemoryBlobStore>,
    }

    async fn world(allowlist: &str) -> World {
        let dir = tempfile::tempdir().unwrap();
        let lifecycle = LifecycleStore::open(dir.path().join("rsky/lifecycle.sqlite"))
            .await
            .unwrap();
        let path = dir.path().join("write-allowlist.toml");
        std::fs::write(&path, allowlist).unwrap();
        let lock_dir = LockDir::new(dir.path().join("locks")).unwrap();
        let actor_store = ActorStore::new(
            &ActorStoreConfig {
                directory: dir.path().join("actors").to_str().unwrap().to_owned(),
                cache_size: 10,
            },
            BackgroundQueue::default(),
            lifecycle,
        )
        .with_admission(Arc::new(Admission::from_file(&path).unwrap()))
        .with_lock_dir(lock_dir.clone());
        let sequencer = SharedSequencer {
            sequencer: RwLock::new(Sequencer::new(
                crate::sequencer::db::get_migrated_db(dir.path().join("sequencer.sqlite"))
                    .await
                    .unwrap(),
                Crawlers::new("pds.test".to_owned(), vec![]),
                None,
            )),
        };
        let repairs = RepairStore::open(dir.path().join("rsky/repair.sqlite"))
            .await
            .unwrap();
        World {
            dir,
            actor_store,
            sequencer,
            repairs,
            lock_dir,
            blobstore: Arc::new(MemoryBlobStore::default()),
        }
    }

    fn keypair() -> Keypair {
        Keypair::from_secret_key(
            &Secp256k1::new(),
            &SecretKey::from_slice(&[3u8; 32]).unwrap(),
        )
    }

    fn post(rkey: &str) -> PreparedWrite {
        let record: rsky_repo::types::RepoRecord = serde_json::from_value(serde_json::json!({
            "$type": "app.bsky.feed.post",
            "text": "drain",
            "createdAt": "2023-01-01T00:00:00.000Z",
        }))
        .unwrap();
        let cid = rsky_common::ipld::cid_for_cbor(&record).unwrap();
        PreparedWrite::Create(PreparedCreateOrUpdate {
            action: WriteOpAction::Create,
            uri: format!("at://{DID}/app.bsky.feed.post/{rkey}"),
            cid,
            swap_cid: None,
            record,
            blobs: vec![],
        })
    }

    #[tokio::test]
    async fn status_reports_what_the_actor_owes() {
        let world = world("version = 1\ndefault = \"active\"\n").await;
        let status = drain_status(&world.actor_store, &world.repairs, DID)
            .await
            .unwrap();
        assert_eq!(status.state, "active");
        assert!(status.fully_drained);
        assert!(status.client_quiescent);
        assert_eq!(status.publish_intent_pending, 0);

        world.actor_store.create(DID, &keypair()).await.unwrap();
        let mut txn = world
            .actor_store
            .transact(DID.to_owned(), world.blobstore.clone())
            .await
            .unwrap();
        txn.create_repo(vec![], true).await.unwrap();
        txn.process_writes(vec![post("3lfixtureaa2a")], None)
            .await
            .unwrap();
        let status = drain_status(&world.actor_store, &world.repairs, DID)
            .await
            .unwrap();
        assert_eq!(status.inflight_mutations, 1);
        assert_eq!(status.publish_intent_pending, 3);
        assert!(!status.client_quiescent);
        assert!(!status.fully_drained);
        drop(txn);

        // the actor's deletion in progress keeps it from being drained
        world.actor_store.lifecycle.tombstone(DID).await.unwrap();
        let status = drain_status(&world.actor_store, &world.repairs, DID)
            .await
            .unwrap();
        assert_eq!(status.lifecycle_pending, 1);
        assert_eq!(status.inflight_mutations, 0);
        world
            .actor_store
            .lifecycle
            .mark_logically_deleted(DID)
            .await
            .unwrap();

        let drained = drain_did(
            &world.actor_store,
            &world.sequencer,
            &world.repairs,
            &world.lock_dir,
            world.blobstore.clone(),
            DID,
            Duration::from_secs(1),
        )
        .await
        .unwrap();
        assert!(drained.fully_drained, "{drained:?}");
        assert_eq!(drained.publish_intent_pending, 0);
        let rows = world
            .sequencer
            .sequencer
            .read()
            .await
            .rows_for_did_after(DID, 0)
            .await
            .unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(
            serde_json::to_value(&drained).unwrap()["fullyDrained"],
            true
        );
        assert!(serde_json::to_value(&drained)
            .unwrap()
            .get("workflowId")
            .is_none());
    }

    #[tokio::test]
    async fn a_drain_waits_for_writes_and_respects_the_allowlist() {
        let world = world(
            "version = 1\ndefault = \"absent\"\n[entries]\n\"did:plc:drain\" = { state = \"maintenance\", workflow_id = \"w1\" }\n",
        )
        .await;
        world
            .repairs
            .create(
                "w1",
                DID,
                &crate::repair::RepairKind::EmptyCommit {
                    boundary: "3zzzzzzzzzzzz".to_owned(),
                },
            )
            .await
            .unwrap();
        let status = drain_status(&world.actor_store, &world.repairs, DID)
            .await
            .unwrap();
        assert_eq!(status.state, "maintenance");
        assert_eq!(status.workflow_id.as_deref(), Some("w1"));
        assert_eq!(status.repair_pending, 1);
        assert!(!status.fully_drained);
        assert_eq!(serde_json::to_value(&status).unwrap()["workflowId"], "w1");

        // a shared holder keeps the exclusive drain out until it lets go
        let held = world.lock_dir.shared(DID).await.unwrap();
        let err = drain_did(
            &world.actor_store,
            &world.sequencer,
            &world.repairs,
            &world.lock_dir,
            world.blobstore.clone(),
            DID,
            Duration::from_millis(80),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("in flight"));
        drop(held);

        // an absent actor is reported but never touched
        let absent = drain_did(
            &world.actor_store,
            &world.sequencer,
            &world.repairs,
            &world.lock_dir,
            world.blobstore.clone(),
            "did:plc:nobody",
            Duration::from_millis(80),
        )
        .await
        .unwrap();
        assert_eq!(absent.state, "absent");
        assert!(absent.fully_drained);

        // a store another implementation created carries no journals
        let location = world.actor_store.get_location(DID).unwrap();
        std::fs::create_dir_all(&location.directory).unwrap();
        std::fs::write(&location.key_location, keypair().secret_bytes()).unwrap();
        crate::actor_store::db::get_db(&location.db_location)
            .unwrap()
            .run(|conn| {
                conn.execute_batch(crate::actor_store::db::ACTOR_DB_MIGRATIONS[0].sql)?;
                Ok(())
            })
            .await
            .unwrap();
        let status = drain_status(&world.actor_store, &world.repairs, DID)
            .await
            .unwrap();
        assert_eq!(status.publish_intent_pending, 0);
        assert_eq!(status.repair_pending, 1);
        assert!(!status.fully_drained);
        let _ = world.dir.path();
    }
}
