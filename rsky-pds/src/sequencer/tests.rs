use super::db::get_migrated_db;
use super::outbox::{Outbox, OutboxError};
use super::*;
use crate::account_manager::helpers::account::AccountStatus;
use crate::actor_store::repo::types::SyncEvtData;
use crate::sequencer::events::sync_evt_data_from_commit;
use futures::{pin_mut, StreamExt};
use ipld_core::ipld::Ipld;
use lexicon_cid::Cid;
use rsky_repo::block_map::BlockMap;
use rsky_repo::cid_set::CidSet;
use rsky_repo::types::{CommitAction, CommitData, CommitOp};
use rusqlite::params;
use std::str::FromStr;
use std::time::Duration as StdDuration;

const TEST_CID: &str = "bafkreibjfgx2gprinfvicegelk5kosd6y2frmqpqzwqkg7usac74l3t2v4";

async fn test_sequencer() -> (tempfile::TempDir, Sequencer) {
    let dir = tempfile::tempdir().unwrap();
    let db = get_migrated_db(dir.path().join("sequencer.sqlite"))
        .await
        .unwrap();
    let sequencer = Sequencer::new(
        db,
        crate::crawlers::Crawlers::new("pds.test".to_owned(), vec![]),
        None,
    );
    (dir, sequencer)
}

fn commit_data(cid: Cid) -> CommitDataWithOps {
    let mut relevant_blocks = BlockMap::new();
    relevant_blocks.set(cid, vec![1, 2, 3]);
    CommitDataWithOps {
        commit_data: CommitData {
            cid,
            rev: "3jzfcijpj2z2a".to_owned(),
            since: None,
            prev: None,
            new_blocks: BlockMap::new(),
            relevant_blocks,
            removed_cids: CidSet::new(None),
        },
        ops: vec![CommitOp {
            action: CommitAction::Create,
            path: "app.bsky.feed.post/3jzfcijpj2z2a".to_owned(),
            cid: Some(cid),
            prev: None,
        }],
        prev_data: None,
    }
}

#[tokio::test]
async fn sequences_and_reads_events() {
    let (_dir, mut sequencer) = test_sequencer().await;
    assert_eq!(sequencer.curr().await.unwrap(), None);

    let cid = Cid::from_str(TEST_CID).unwrap();
    let did = "did:plc:seq".to_owned();

    let seq1 = sequencer
        .sequence_commit(did.clone(), commit_data(cid))
        .await
        .unwrap();
    let seq2 = sequencer
        .sequence_sync_evt(
            did.clone(),
            SyncEvtData {
                cid,
                rev: "3jzfcijpj2z2a".to_owned(),
                blocks: {
                    let mut blocks = BlockMap::new();
                    blocks.set(cid, vec![1, 2, 3]);
                    blocks
                },
            },
        )
        .await
        .unwrap();
    let seq3 = sequencer
        .sequence_identity_evt(did.clone(), Some("seq.test".to_owned()))
        .await
        .unwrap();
    let seq4 = sequencer
        .sequence_account_evt(did.clone(), AccountStatus::Takendown)
        .await
        .unwrap();
    let seq5 = sequencer
        .sequence_handle_update(did.clone(), "seq2.test".to_owned())
        .await
        .unwrap();
    assert_eq!(vec![seq1, seq2, seq3, seq4, seq5], vec![1, 2, 3, 4, 5]);

    assert_eq!(sequencer.curr().await.unwrap(), Some(5));
    let next = sequencer.next_seq(1).await.unwrap().unwrap();
    assert_eq!(next.seq, Some(2));
    assert!(sequencer.next_seq(5).await.unwrap().is_none());

    let earliest = sequencer
        .earliest_after_time("2020-01-01T00:00:00.000Z".to_owned())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(earliest.seq, Some(1));
    assert!(sequencer
        .earliest_after_time("2100-01-01T00:00:00.000Z".to_owned())
        .await
        .unwrap()
        .is_none());

    // handle events are not surfaced by request_seq_range; the four
    // typed events come back in order
    let evts = sequencer
        .request_seq_range(RequestSeqRangeOpts {
            earliest_seq: None,
            latest_seq: None,
            earliest_time: None,
            limit: None,
        })
        .await
        .unwrap();
    assert_eq!(evts.len(), 4);
    assert!(matches!(evts[0], SeqEvt::TypedCommitEvt(_)));
    assert!(matches!(evts[1], SeqEvt::TypedSyncEvt(_)));
    assert!(matches!(evts[2], SeqEvt::TypedIdentityEvt(_)));
    assert!(matches!(evts[3], SeqEvt::TypedAccountEvt(_)));
    assert_eq!(
        evts.iter().map(|evt| evt.seq()).collect::<Vec<i64>>(),
        vec![1, 2, 3, 4]
    );

    // filters
    let evts = sequencer
        .request_seq_range(RequestSeqRangeOpts {
            earliest_seq: Some(2),
            latest_seq: Some(4),
            earliest_time: Some("2020-01-01T00:00:00.000Z".to_owned()),
            limit: Some(1),
        })
        .await
        .unwrap();
    assert_eq!(evts.len(), 1);
    assert_eq!(evts[0].seq(), 3);

    // invalidated events are skipped
    sequencer
        .db
        .run(|conn| {
            conn.execute("UPDATE repo_seq SET invalidated = 1 WHERE seq = 1", [])?;
            Ok(())
        })
        .await
        .unwrap();
    let evts = sequencer
        .request_seq_range(RequestSeqRangeOpts {
            earliest_seq: None,
            latest_seq: None,
            earliest_time: None,
            limit: None,
        })
        .await
        .unwrap();
    assert_eq!(evts[0].seq(), 2);

    // a rebase event decodes as a commit; unknown event types are skipped
    sequencer
        .db
        .run(|conn| {
            conn.execute(
                "UPDATE repo_seq SET \"eventType\" = 'rebase' WHERE seq = 1",
                [],
            )?;
            conn.execute("UPDATE repo_seq SET invalidated = 0 WHERE seq = 1", [])?;
            conn.execute(
                "UPDATE repo_seq SET \"eventType\" = 'unknown' WHERE seq = 4",
                [],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    let evts = sequencer
        .request_seq_range(RequestSeqRangeOpts {
            earliest_seq: None,
            latest_seq: None,
            earliest_time: None,
            limit: None,
        })
        .await
        .unwrap();
    assert!(matches!(evts[0], SeqEvt::TypedCommitEvt(_)));
    assert!(!evts.iter().any(|evt| evt.seq() == 4));
}

#[tokio::test]
async fn deletes_events_for_user() {
    let (_dir, mut sequencer) = test_sequencer().await;
    let keep_seq = sequencer
        .sequence_identity_evt("did:plc:del".to_owned(), None)
        .await
        .unwrap();
    sequencer
        .sequence_identity_evt("did:plc:del".to_owned(), None)
        .await
        .unwrap();
    let other_seq = sequencer
        .sequence_identity_evt("did:plc:other".to_owned(), None)
        .await
        .unwrap();

    sequencer
        .delete_all_for_user("did:plc:del", Some(vec![keep_seq]))
        .await
        .unwrap();
    let remaining: Vec<i64> = sequencer
        .request_seq_range(RequestSeqRangeOpts {
            earliest_seq: None,
            latest_seq: None,
            earliest_time: None,
            limit: None,
        })
        .await
        .unwrap()
        .iter()
        .map(|evt| evt.seq())
        .collect();
    assert_eq!(remaining, vec![keep_seq, other_seq]);

    sequencer
        .delete_all_for_user("did:plc:del", None)
        .await
        .unwrap();
    let remaining: Vec<i64> = sequencer
        .request_seq_range(RequestSeqRangeOpts {
            earliest_seq: None,
            latest_seq: None,
            earliest_time: None,
            limit: None,
        })
        .await
        .unwrap()
        .iter()
        .map(|evt| evt.seq())
        .collect();
    assert_eq!(remaining, vec![other_seq]);
}

#[tokio::test]
async fn start_broadcasts_sequenced_events_until_destroyed() {
    let (_dir, mut sequencer) = test_sequencer().await;
    let mut live = sequencer.subscribe();
    let mut background = sequencer.clone();
    let handle = tokio::spawn(async move { background.start().await });
    // let the poll loop take its initial cursor before sequencing
    tokio::time::sleep(StdDuration::from_millis(500)).await;

    sequencer
        .sequence_identity_evt("did:plc:start-loop".to_owned(), None)
        .await
        .unwrap();
    let batch = tokio::time::timeout(StdDuration::from_secs(5), live.recv())
        .await
        .expect("the poll loop broadcasts the batch")
        .unwrap();
    assert_eq!(batch.len(), 1);
    assert!(matches!(batch[0], SeqEvt::TypedIdentityEvt(_)));
    assert_eq!(sequencer.last_seen(), batch[0].seq());

    assert!(!sequencer.is_destroyed());
    sequencer.destroy().await;
    assert!(sequencer.is_destroyed());
    let res = tokio::time::timeout(StdDuration::from_secs(5), handle)
        .await
        .expect("sequencer poll loop did not stop after destroy")
        .unwrap();
    assert!(res.is_ok());
}

/// Inserts `count` identity rows for `did` directly, as one transaction.
async fn seed_rows(sequencer: &Sequencer, did: &str, count: usize) {
    let did = did.to_owned();
    let event = rsky_common::struct_to_cbor(&crate::sequencer::events::IdentityEvt {
        did: did.clone(),
        handle: None,
    })
    .unwrap();
    sequencer
        .db
        .tx(move |tx| {
            let mut stmt = tx.prepare(
                "INSERT INTO repo_seq (did, event, \"eventType\", \"sequencedAt\") \
                 VALUES (?1, ?2, 'identity', ?3)",
            )?;
            for _ in 0..count {
                stmt.execute(params![did, event, rsky_common::now()])?;
            }
            Ok(())
        })
        .await
        .unwrap();
}

/// A subscriber with a cursor receives every row above it exactly once,
/// across page boundaries and across the switch from backfill to live
/// delivery, and never an invalidated row.
#[tokio::test]
async fn outbox_backfills_exactly_and_hands_over_to_live_delivery() {
    let (_dir, sequencer) = test_sequencer().await;
    seed_rows(&sequencer, "did:plc:backfill", 3000).await;
    assert!(sequencer.invalidate(1500).await.unwrap());
    assert!(!sequencer.invalidate(1500).await.unwrap());
    let outbox = Outbox::new(sequencer.clone());

    // the whole history from the start, minus the invalidated row
    let stream = outbox.events(Some(0)).await;
    pin_mut!(stream);
    let mut seqs = Vec::new();
    while seqs.len() < 2999 {
        let evt = stream.next().await.unwrap().unwrap();
        seqs.push(evt.seq());
    }
    assert_eq!(seqs.len(), 2999);
    assert!(seqs.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(seqs.first(), Some(&1));
    assert_eq!(seqs.last(), Some(&3000));
    assert!(!seqs.contains(&1500));

    // live rows arrive through the poll loop with no gap or repeat
    let mut background = sequencer.clone();
    let poller = tokio::spawn(async move { background.start().await });
    tokio::time::sleep(StdDuration::from_millis(300)).await;
    let mut writer = sequencer.clone();
    for _ in 0..3 {
        writer
            .sequence_identity_evt("did:plc:live".to_owned(), None)
            .await
            .unwrap();
    }
    for expected in 3001..=3003 {
        let evt = tokio::time::timeout(StdDuration::from_secs(5), stream.next())
            .await
            .expect("live event")
            .unwrap()
            .unwrap();
        assert_eq!(evt.seq(), expected);
    }

    // a cursor in the middle starts right after it, and a subscriber with
    // no cursor sees only what follows
    let middle = outbox.events(Some(2998)).await;
    pin_mut!(middle);
    let first = middle.next().await.unwrap().unwrap();
    assert_eq!(first.seq(), 2999);
    let tail = outbox.events(None).await;
    pin_mut!(tail);
    writer
        .sequence_identity_evt("did:plc:live".to_owned(), None)
        .await
        .unwrap();
    let only = tokio::time::timeout(StdDuration::from_secs(5), tail.next())
        .await
        .expect("live event")
        .unwrap()
        .unwrap();
    assert_eq!(only.seq(), 3004);

    writer.destroy().await;
    poller.await.unwrap().unwrap();
}

/// A subscriber that falls further behind the broadcast than its capacity
/// is told so instead of being served a gap.
#[tokio::test]
async fn outbox_reports_a_consumer_that_fell_too_far_behind() {
    let dir = tempfile::tempdir().unwrap();
    let db = get_migrated_db(dir.path().join("sequencer.sqlite"))
        .await
        .unwrap();
    let sequencer = Sequencer::with_broadcast_capacity(
        db,
        crate::crawlers::Crawlers::new("pds.test".to_owned(), vec![]),
        None,
        2,
    );
    let outbox = Outbox::new(sequencer.clone());
    let stream = outbox.events(None).await;
    pin_mut!(stream);
    // three batches land while the subscriber has not polled once
    for _ in 0..3 {
        let _ = sequencer.events.send(vec![]);
    }
    let err = stream.next().await.unwrap().unwrap_err();
    assert_eq!(
        err.downcast_ref::<OutboxError>(),
        Some(&OutboxError::ConsumerTooSlow(1))
    );
    assert!(err.to_string().contains("ConsumerTooSlow"));
    // the poll loop going away ends the subscription with its own error
    assert_eq!(
        OutboxError::from(tokio::sync::broadcast::error::RecvError::Closed),
        OutboxError::SequencerStopped
    );
    assert!(OutboxError::SequencerStopped
        .to_string()
        .contains("stopped"));
}

#[tokio::test]
async fn start_surfaces_startup_db_errors() {
    let (_dir, mut sequencer) = test_sequencer().await;
    sequencer
        .db
        .run(|conn| {
            conn.execute_batch("DROP TABLE repo_seq")?;
            Ok(())
        })
        .await
        .unwrap();
    let err = sequencer.start().await.unwrap_err();
    assert!(err.to_string().contains("no such table"));
}

#[tokio::test]
async fn start_logs_poll_errors_and_keeps_running() {
    let _guard = tracing::subscriber::set_default(tracing_subscriber::FmtSubscriber::new());
    let (_dir, mut sequencer) = test_sequencer().await;
    let mut background = sequencer.clone();
    let handle = tokio::spawn(async move { background.start().await });
    tokio::time::sleep(StdDuration::from_millis(200)).await;

    // break the db mid-loop; the loop logs the error and keeps polling
    sequencer
        .db
        .run(|conn| {
            conn.execute_batch("DROP TABLE repo_seq")?;
            Ok(())
        })
        .await
        .unwrap();
    tokio::time::sleep(StdDuration::from_millis(2200)).await;
    assert!(!handle.is_finished());

    sequencer.destroy().await;
    let res = tokio::time::timeout(StdDuration::from_secs(5), handle)
        .await
        .expect("sequencer poll loop did not stop after destroy")
        .unwrap();
    assert!(res.is_ok());
}

fn as_map(ipld: Ipld) -> std::collections::BTreeMap<String, Ipld> {
    match ipld {
        Ipld::Map(map) => map,
        other => panic!("expected cbor map, got {other:?}"),
    }
}

fn sorted_keys(map: &std::collections::BTreeMap<String, Ipld>) -> Vec<&str> {
    map.keys().map(|key| key.as_str()).collect()
}

#[tokio::test]
async fn commit_event_matches_reference_shape() {
    let cid = Cid::from_str(TEST_CID).unwrap();
    let evt = format_seq_commit("did:plc:golden".to_owned(), commit_data(cid))
        .await
        .unwrap();
    assert_eq!(evt.event_type, "append");
    let map = as_map(serde_ipld_dagcbor::from_slice(&evt.event).unwrap());
    // deprecated `prev` is omitted; `prevData` only appears when present
    assert_eq!(
        sorted_keys(&map),
        vec!["blobs", "blocks", "commit", "ops", "rebase", "repo", "rev", "since", "tooBig"]
    );
    assert!(matches!(map.get("blocks"), Some(Ipld::Bytes(_))));
    assert_eq!(map.get("rebase"), Some(&Ipld::Bool(false)));
    assert_eq!(map.get("tooBig"), Some(&Ipld::Bool(false)));
    assert_eq!(map.get("since"), Some(&Ipld::Null));
    let Some(Ipld::List(ops)) = map.get("ops") else {
        panic!("expected ops list");
    };
    let Ipld::Map(op) = &ops[0] else {
        panic!("expected op map");
    };
    // creates have no `prev`, and `cid` is always present
    assert_eq!(
        op.keys().map(|key| key.as_str()).collect::<Vec<&str>>(),
        vec!["action", "cid", "path"]
    );
    assert_eq!(op.get("action"), Some(&Ipld::String("create".to_owned())));
}

#[tokio::test]
async fn commit_event_includes_prev_data_and_op_prev() {
    let cid = Cid::from_str(TEST_CID).unwrap();
    let mut data = commit_data(cid);
    data.prev_data = Some(cid);
    data.ops = vec![CommitOp {
        action: CommitAction::Delete,
        path: "app.bsky.feed.post/3jzfcijpj2z2a".to_owned(),
        cid: None,
        prev: Some(cid),
    }];
    let evt = format_seq_commit("did:plc:golden".to_owned(), data)
        .await
        .unwrap();
    let map = as_map(serde_ipld_dagcbor::from_slice(&evt.event).unwrap());
    assert_eq!(map.get("prevData"), Some(&Ipld::Link(cid)));
    let Some(Ipld::List(ops)) = map.get("ops") else {
        panic!("expected ops list");
    };
    let Ipld::Map(op) = &ops[0] else {
        panic!("expected op map");
    };
    // deletes carry a null `cid` and the previous record cid in `prev`
    assert_eq!(op.get("cid"), Some(&Ipld::Null));
    assert_eq!(op.get("prev"), Some(&Ipld::Link(cid)));
}

#[tokio::test]
async fn sync_event_matches_reference_shape() {
    let cid = Cid::from_str(TEST_CID).unwrap();
    let mut blocks = BlockMap::new();
    blocks.set(cid, vec![1, 2, 3]);
    let evt = format_seq_sync_evt(
        "did:plc:golden".to_owned(),
        SyncEvtData {
            cid,
            rev: "3jzfcijpj2z2a".to_owned(),
            blocks,
        },
    )
    .await
    .unwrap();
    assert_eq!(evt.event_type, "sync");
    let map = as_map(serde_ipld_dagcbor::from_slice(&evt.event).unwrap());
    assert_eq!(sorted_keys(&map), vec!["blocks", "did", "rev"]);
    // the CAR slice is a CBOR byte string, not an integer array
    assert!(matches!(map.get("blocks"), Some(Ipld::Bytes(_))));
}

#[tokio::test]
async fn identity_event_omits_absent_handle() {
    let evt = format_seq_identity_evt("did:plc:golden".to_owned(), None)
        .await
        .unwrap();
    assert_eq!(evt.event_type, "identity");
    let map = as_map(serde_ipld_dagcbor::from_slice(&evt.event).unwrap());
    assert_eq!(sorted_keys(&map), vec!["did"]);

    let evt = format_seq_identity_evt("did:plc:golden".to_owned(), Some("alice.test".to_owned()))
        .await
        .unwrap();
    let map = as_map(serde_ipld_dagcbor::from_slice(&evt.event).unwrap());
    assert_eq!(sorted_keys(&map), vec!["did", "handle"]);
    assert_eq!(
        map.get("handle"),
        Some(&Ipld::String("alice.test".to_owned()))
    );
}

#[tokio::test]
async fn account_event_matches_reference_shape() {
    let evt = format_seq_account_evt("did:plc:golden".to_owned(), AccountStatus::Active)
        .await
        .unwrap();
    assert_eq!(evt.event_type, "account");
    let map = as_map(serde_ipld_dagcbor::from_slice(&evt.event).unwrap());
    assert_eq!(sorted_keys(&map), vec!["active", "did"]);
    assert_eq!(map.get("active"), Some(&Ipld::Bool(true)));

    for (status, expected) in [
        (AccountStatus::Takendown, "takendown"),
        (AccountStatus::Suspended, "suspended"),
        (AccountStatus::Deleted, "deleted"),
        (AccountStatus::Deactivated, "deactivated"),
    ] {
        let evt = format_seq_account_evt("did:plc:golden".to_owned(), status)
            .await
            .unwrap();
        let map = as_map(serde_ipld_dagcbor::from_slice(&evt.event).unwrap());
        assert_eq!(sorted_keys(&map), vec!["active", "did", "status"]);
        assert_eq!(map.get("active"), Some(&Ipld::Bool(false)));
        assert_eq!(map.get("status"), Some(&Ipld::String(expected.to_owned())));
    }
}

#[tokio::test]
async fn sync_evt_data_from_commit_requires_commit_block() {
    let cid = Cid::from_str(TEST_CID).unwrap();
    let data = commit_data(cid);
    let sync_data = sync_evt_data_from_commit(data).await.unwrap();
    assert_eq!(sync_data.cid, cid);
    assert_eq!(sync_data.rev, "3jzfcijpj2z2a");

    let mut missing = commit_data(cid);
    missing.commit_data.relevant_blocks = BlockMap::new();
    let err = sync_evt_data_from_commit(missing).await.unwrap_err();
    assert!(err.to_string().contains("commit block was not found"));
}

#[tokio::test]
async fn rows_for_did_after_returns_stored_rows_in_order() {
    let (_dir, mut sequencer) = test_sequencer().await;
    let cid = Cid::from_str(TEST_CID).unwrap();
    sequencer
        .sequence_commit("did:plc:one".to_owned(), commit_data(cid))
        .await
        .unwrap();
    sequencer
        .sequence_account_evt("did:plc:two".to_owned(), AccountStatus::Active)
        .await
        .unwrap();
    sequencer
        .sequence_account_evt("did:plc:one".to_owned(), AccountStatus::Deactivated)
        .await
        .unwrap();
    let rows = sequencer
        .rows_for_did_after("did:plc:one", 0)
        .await
        .unwrap();
    assert_eq!(
        rows.iter()
            .map(|row| row.event_type.as_str())
            .collect::<Vec<_>>(),
        ["append", "account"]
    );
    assert_eq!(rows[0].seq, Some(1));
    let later = sequencer
        .rows_for_did_after("did:plc:one", 1)
        .await
        .unwrap();
    assert_eq!(later.len(), 1);
    assert_eq!(later[0].seq, Some(3));
    assert!(sequencer
        .rows_for_did_after("did:plc:one", 3)
        .await
        .unwrap()
        .is_empty());
}
