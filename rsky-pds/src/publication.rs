//! Delivery of committed publication intents to the sequencer.
//!
//! A write commits its intent with its blocks (see `ActorStoreTransactor`),
//! and the publisher turns each pending intent into exactly one sequencer
//! row: it records the sequencer's head before inserting, so after a crash
//! it can recognise a row it already inserted instead of inserting it again,
//! and it acknowledges the row in the store only after the sequencer has
//! made it durable.

use crate::actor_store::blob::BlobReader;
use crate::actor_store::db::ActorDb;
use crate::actor_store::{pending_intents_in, ActorStore, StoredIntent};
use crate::lifecycle::LifecycleStore;
use crate::models::models::RepoSeq;
use crate::sequencer::events::{CommitEvt, SyncEvt};
use crate::SharedSequencer;
use anyhow::Result;
use rsky_common::cbor_to_struct;
use rusqlite::params;

/// The moves of a delivery, in order; tests stop after one of them to stand
/// in for a crash there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishStep {
    /// The sequencer head was recorded on the intent.
    Floored,
    /// The row was inserted into the sequencer.
    Sequenced,
    /// The row was acknowledged in the store.
    Acked,
}

/// Whether a sequencer row is the delivery of `intent`.
fn row_matches(row: &RepoSeq, intent: &StoredIntent) -> bool {
    if row.event_type != intent.event_type {
        return false;
    }
    match row.event_type.as_str() {
        "append" => cbor_to_struct::<CommitEvt>(row.event.clone())
            .map(|evt| evt.rev == intent.rev && evt.commit.to_string() == intent.cid)
            .unwrap_or(false),
        "sync" => cbor_to_struct::<SyncEvt>(row.event.clone())
            .map(|evt| evt.rev == intent.rev)
            .unwrap_or(false),
        _ => row.event == intent.event,
    }
}

async fn record_floor(db: &ActorDb, intent_id: i64, floor: i64) -> Result<()> {
    db.run(move |conn| {
        conn.execute(
            "UPDATE publish_intent SET \"seqFloor\" = ?1 WHERE id = ?2 AND \"seqFloor\" IS NULL",
            params![floor, intent_id],
        )?;
        Ok(())
    })
    .await
}

async fn acknowledge(db: &ActorDb, intent_id: i64, seq: i64) -> Result<()> {
    let now = rsky_common::now();
    db.tx(move |tx| {
        tx.execute(
            "UPDATE publish_intent SET state = 'delivered', seq = ?1 WHERE id = ?2",
            params![seq, intent_id],
        )?;
        tx.execute(
            "INSERT INTO publish_ack (\"intentId\", seq, \"ackedAt\") VALUES (?1, ?2, ?3) \
             ON CONFLICT (\"intentId\") DO NOTHING",
            params![intent_id, seq, now],
        )?;
        Ok(())
    })
    .await
}

/// Delivers every pending intent of `did`, oldest first, and returns the
/// sequence numbers they received. An intent whose row already exists from
/// an earlier attempt is acknowledged against that row.
pub async fn publish_pending(
    actor_store: &ActorStore,
    sequencer: &SharedSequencer,
    did: &str,
    stop_after: Option<PublishStep>,
) -> Result<Vec<i64>> {
    let _guard = actor_store.publish_lock(did).lock_owned().await;
    actor_store.admission.admit_worker(did)?;
    if !actor_store.exists(did).await? {
        actor_store.lifecycle.clear_pending_work(did).await?;
        return Ok(vec![]);
    }
    let db = actor_store.write_db(did).await?;
    let intents = db.run(|conn| pending_intents_in(conn)).await?;
    let mut seqs = Vec::with_capacity(intents.len());
    for intent in intents {
        let floor = match intent.seq_floor {
            Some(floor) => floor,
            None => {
                let head = sequencer.sequencer.read().await.curr().await?.unwrap_or(0);
                record_floor(&db, intent.id, head).await?;
                head
            }
        };
        if stop_after == Some(PublishStep::Floored) {
            return Ok(seqs);
        }
        let already = sequencer
            .sequencer
            .read()
            .await
            .rows_for_did_after(did, floor)
            .await?
            .into_iter()
            .find(|row| row_matches(row, &intent))
            .and_then(|row| row.seq);
        let seq = match already {
            Some(seq) => {
                tracing::warn!(did, intent = intent.id, seq, "intent was already sequenced");
                seq
            }
            None => {
                let mut lock = sequencer.sequencer.write().await;
                lock.sequence_evt(RepoSeq::new(
                    did.to_owned(),
                    intent.event_type.clone(),
                    intent.event.clone(),
                    rsky_common::now(),
                ))
                .await?
            }
        };
        if stop_after == Some(PublishStep::Sequenced) {
            return Ok(seqs);
        }
        acknowledge(&db, intent.id, seq).await?;
        if intent.event_type == "append" {
            let creation = cbor_to_struct::<CommitEvt>(intent.event.clone())
                .map(|evt| evt.since.is_none())
                .unwrap_or(false);
            actor_store
                .lifecycle
                .record_publication(did, &intent.rev, seq, creation)
                .await?;
        }
        seqs.push(seq);
    }
    let blob = BlobReader::new(
        crate::actor_store::blobstore::unavailable(),
        db.clone(),
        actor_store.background_queue.clone(),
        actor_store.coexistence,
    );
    settle_pending_work(&actor_store.lifecycle, &db, &blob, did).await?;
    Ok(seqs)
}

/// Clears the actor's pending mark when no intent is undelivered and no
/// blob work is outstanding.
pub async fn settle_pending_work(
    lifecycle: &LifecycleStore,
    db: &ActorDb,
    blob: &BlobReader,
    did: &str,
) -> Result<()> {
    let pending = db.run(|conn| pending_intents_in(conn)).await?;
    if pending.is_empty() && blob.nonterminal_blob_work().await? == 0 {
        lifecycle.clear_pending_work(did).await?;
    }
    Ok(())
}

/// Finishes the publication and blob work every marked actor was left with.
pub async fn resume_pending_work(
    actor_store: &ActorStore,
    sequencer: &SharedSequencer,
    blobstore_for: impl Fn(&str) -> std::sync::Arc<dyn crate::actor_store::blobstore::BlobStore>,
) -> Result<Vec<String>> {
    let mut resumed = Vec::new();
    for did in actor_store.lifecycle.pending_work().await? {
        tracing::warn!(%did, "resuming publication left by a previous process");
        if let Err(refused) = actor_store.admission.admit_worker(&did) {
            tracing::warn!(%refused, "publication left for the actor's writer");
            continue;
        }
        publish_pending(actor_store, sequencer, &did, None).await?;
        if actor_store.exists(&did).await? {
            actor_store.queue_blob_work(&did, blobstore_for(&did));
        }
        resumed.push(did);
    }
    Ok(resumed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor_store::blobstore::MemoryBlobStore;
    use crate::actor_store::ActorStore;
    use crate::background::BackgroundQueue;
    use crate::config::ActorStoreConfig;
    use crate::crawlers::Crawlers;
    use crate::sequencer::{RequestSeqRangeOpts, Sequencer};
    use rsky_repo::types::{PreparedCreateOrUpdate, PreparedWrite, WriteOpAction};
    use secp256k1::{Keypair, Secp256k1, SecretKey};
    use std::sync::Arc;

    const DID: &str = "did:plc:publisher";

    struct World {
        _dir: tempfile::TempDir,
        actor_store: ActorStore,
        sequencer: SharedSequencer,
        blobstore: Arc<MemoryBlobStore>,
    }

    async fn world() -> World {
        let dir = tempfile::tempdir().unwrap();
        let lifecycle = LifecycleStore::open(dir.path().join("rsky/lifecycle.sqlite"))
            .await
            .unwrap();
        let sequencer_db =
            crate::sequencer::db::get_migrated_db(dir.path().join("sequencer.sqlite"))
                .await
                .unwrap();
        let sequencer = SharedSequencer {
            sequencer: tokio::sync::RwLock::new(Sequencer::new(
                sequencer_db,
                Crawlers::new("pds.test".to_owned(), vec![]),
                None,
            )),
        };
        let actor_store = ActorStore::new(
            &ActorStoreConfig {
                directory: dir.path().join("actors").to_str().unwrap().to_owned(),
                cache_size: 10,
            },
            BackgroundQueue::default(),
            lifecycle,
        );
        let keypair = Keypair::from_secret_key(
            &Secp256k1::new(),
            &SecretKey::from_slice(&[9u8; 32]).unwrap(),
        );
        actor_store.create(DID, &keypair).await.unwrap();
        World {
            _dir: dir,
            actor_store,
            sequencer,
            blobstore: Arc::new(MemoryBlobStore::default()),
        }
    }

    fn post(rkey: &str, text: &str) -> PreparedWrite {
        let record: rsky_repo::types::RepoRecord = serde_json::from_value(serde_json::json!({
            "$type": "app.bsky.feed.post",
            "text": text,
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

    async fn init_repo(world: &World) {
        let txn = world
            .actor_store
            .transact(DID.to_owned(), world.blobstore.clone())
            .await
            .unwrap();
        txn.create_repo(vec![], true).await.unwrap();
    }

    async fn write_post(world: &World, rkey: &str) {
        let mut txn = world
            .actor_store
            .transact(DID.to_owned(), world.blobstore.clone())
            .await
            .unwrap();
        txn.process_writes(vec![post(rkey, "hello")], None)
            .await
            .unwrap();
    }

    async fn event_types(world: &World) -> Vec<String> {
        let lock = world.sequencer.sequencer.read().await;
        lock.rows_for_did_after(DID, 0)
            .await
            .unwrap()
            .into_iter()
            .map(|row| row.event_type)
            .collect()
    }

    async fn intents(world: &World) -> Vec<StoredIntent> {
        world
            .actor_store
            .read(DID.to_owned(), world.blobstore.clone())
            .await
            .unwrap()
            .all_intents()
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_write_commits_its_intents_and_publishes_them_once() {
        let world = world().await;
        init_repo(&world).await;
        let pending = intents(&world).await;
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].event_type, "append");
        assert_eq!(pending[1].event_type, "sync");
        assert!(pending.iter().all(|intent| intent.state == "pending"));
        assert_eq!(
            world.actor_store.lifecycle.pending_work().await.unwrap(),
            [DID]
        );
        assert!(event_types(&world).await.is_empty());

        let seqs = publish_pending(&world.actor_store, &world.sequencer, DID, None)
            .await
            .unwrap();
        assert_eq!(seqs, [1, 2]);
        assert_eq!(event_types(&world).await, ["append", "sync"]);
        let delivered = intents(&world).await;
        assert!(delivered.iter().all(|intent| intent.state == "delivered"));
        assert_eq!(delivered[0].seq, Some(1));
        assert_eq!(delivered[0].seq_floor, Some(0));
        assert!(world
            .actor_store
            .lifecycle
            .pending_work()
            .await
            .unwrap()
            .is_empty());

        let mark = world
            .actor_store
            .lifecycle
            .frontier_watermark(DID)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(mark.creation_seq, Some(1));
        assert_eq!(mark.max_rev.as_deref(), Some(delivered[0].rev.as_str()));

        // nothing left to publish
        let again = publish_pending(&world.actor_store, &world.sequencer, DID, None)
            .await
            .unwrap();
        assert!(again.is_empty());
        assert_eq!(event_types(&world).await.len(), 2);

        // the stream sees the rows the publisher inserted
        let lock = world.sequencer.sequencer.read().await;
        let evts = lock
            .request_seq_range(RequestSeqRangeOpts {
                earliest_seq: None,
                latest_seq: None,
                earliest_time: None,
                limit: None,
            })
            .await
            .unwrap();
        assert_eq!(evts.len(), 2);
    }

    #[tokio::test]
    async fn a_crash_after_the_floor_delivers_exactly_once() {
        let world = world().await;
        init_repo(&world).await;
        publish_pending(&world.actor_store, &world.sequencer, DID, None)
            .await
            .unwrap();
        write_post(&world, "3lfixtureaa2a").await;

        let stopped = publish_pending(
            &world.actor_store,
            &world.sequencer,
            DID,
            Some(PublishStep::Floored),
        )
        .await
        .unwrap();
        assert!(stopped.is_empty());
        let pending = intents(&world).await;
        assert_eq!(pending[2].state, "pending");
        assert_eq!(pending[2].seq_floor, Some(2));
        assert_eq!(event_types(&world).await.len(), 2);

        let seqs = publish_pending(&world.actor_store, &world.sequencer, DID, None)
            .await
            .unwrap();
        assert_eq!(seqs, [3]);
        assert_eq!(event_types(&world).await, ["append", "sync", "append"]);
    }

    #[tokio::test]
    async fn a_crash_after_sequencing_recognises_the_row() {
        let world = world().await;
        init_repo(&world).await;
        publish_pending(&world.actor_store, &world.sequencer, DID, None)
            .await
            .unwrap();
        write_post(&world, "3lfixtureaa2b").await;

        publish_pending(
            &world.actor_store,
            &world.sequencer,
            DID,
            Some(PublishStep::Sequenced),
        )
        .await
        .unwrap();
        assert_eq!(event_types(&world).await.len(), 3);
        assert_eq!(intents(&world).await[2].state, "pending");

        let seqs = publish_pending(&world.actor_store, &world.sequencer, DID, None)
            .await
            .unwrap();
        assert_eq!(seqs, [3]);
        assert_eq!(event_types(&world).await.len(), 3);
        let delivered = intents(&world).await;
        assert_eq!(delivered[2].state, "delivered");
        assert_eq!(delivered[2].seq, Some(3));
        // the same for the two-intent creation batch
        assert_eq!(delivered[0].seq, Some(1));
    }

    #[tokio::test]
    async fn resume_finishes_marked_actors_and_forgets_deleted_ones() {
        let world = world().await;
        init_repo(&world).await;
        world
            .actor_store
            .lifecycle
            .mark_pending_work("did:plc:gone")
            .await
            .unwrap();
        let blobstore = world.blobstore.clone();
        let mut resumed =
            resume_pending_work(&world.actor_store, &world.sequencer, |_| blobstore.clone())
                .await
                .unwrap();
        resumed.sort();
        assert_eq!(resumed, ["did:plc:gone", DID]);
        world.actor_store.background_queue.process_all().await;
        assert_eq!(event_types(&world).await, ["append", "sync"]);
        assert!(world
            .actor_store
            .lifecycle
            .pending_work()
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn an_absent_actor_is_left_to_its_writer() {
        let world = world().await;
        init_repo(&world).await;
        let allowlist = world._dir.path().join("write-allowlist.toml");
        std::fs::write(&allowlist, "version = 1\ndefault = \"absent\"\n").unwrap();
        let admission = Arc::new(crate::admission::Admission::from_file(&allowlist).unwrap());
        let restricted = ActorStore::new(
            &ActorStoreConfig {
                directory: world
                    ._dir
                    .path()
                    .join("actors")
                    .to_str()
                    .unwrap()
                    .to_owned(),
                cache_size: 10,
            },
            BackgroundQueue::default(),
            world.actor_store.lifecycle.clone(),
        )
        .with_admission(admission);
        let refused = publish_pending(&restricted, &world.sequencer, DID, None)
            .await
            .unwrap_err();
        assert!(refused
            .downcast_ref::<crate::admission::NotAdmitted>()
            .is_some());
        let blobstore = world.blobstore.clone();
        let resumed = resume_pending_work(&restricted, &world.sequencer, |_| blobstore.clone())
            .await
            .unwrap();
        assert!(resumed.is_empty());
        assert_eq!(restricted.lifecycle.pending_work().await.unwrap(), [DID]);
        assert!(event_types(&world).await.is_empty());
    }

    #[test]
    fn row_matching_by_event_identity() {
        let intent = StoredIntent {
            id: 1,
            rev: "3lfixtureaa2a".to_owned(),
            cid: "bafkreibjfgx2gprinfvicegelk5kosd6y2frmqpqzwqkg7usac74l3t2v4".to_owned(),
            event_type: "sync".to_owned(),
            event: vec![1, 2, 3],
            state: "pending".to_owned(),
            seq_floor: None,
            seq: None,
        };
        let sync = SyncEvt {
            did: DID.to_owned(),
            blocks: vec![],
            rev: intent.rev.clone(),
        };
        let row = RepoSeq::new(
            DID.to_owned(),
            "sync".to_owned(),
            rsky_common::struct_to_cbor(&sync).unwrap(),
            rsky_common::now(),
        );
        assert!(row_matches(&row, &intent));
        let other_rev = RepoSeq::new(
            DID.to_owned(),
            "sync".to_owned(),
            rsky_common::struct_to_cbor(&SyncEvt {
                rev: "3lfixtureaa2b".to_owned(),
                ..sync
            })
            .unwrap(),
            rsky_common::now(),
        );
        assert!(!row_matches(&other_rev, &intent));
        let garbage = RepoSeq::new(
            DID.to_owned(),
            "sync".to_owned(),
            vec![0xff],
            rsky_common::now(),
        );
        assert!(!row_matches(&garbage, &intent));
        let wrong_type = RepoSeq::new(
            DID.to_owned(),
            "append".to_owned(),
            vec![0xff],
            rsky_common::now(),
        );
        assert!(!row_matches(&wrong_type, &intent));
        let opaque = StoredIntent {
            event_type: "identity".to_owned(),
            ..intent
        };
        let same_bytes = RepoSeq::new(
            DID.to_owned(),
            "identity".to_owned(),
            vec![1, 2, 3],
            rsky_common::now(),
        );
        assert!(row_matches(&same_bytes, &opaque));
        let garbage_append = RepoSeq::new(
            DID.to_owned(),
            "append".to_owned(),
            vec![0xff],
            rsky_common::now(),
        );
        let append_intent = StoredIntent {
            event_type: "append".to_owned(),
            ..opaque
        };
        assert!(!row_matches(&garbage_append, &append_intent));
    }
}
