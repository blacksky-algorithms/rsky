use super::db::get_migrated_db;
use super::*;
use crate::account_manager::helpers::account::AccountStatus;
use crate::actor_store::repo::types::SyncEvtData;
use lexicon_cid::Cid;
use rsky_repo::block_map::BlockMap;
use rsky_repo::cid_set::CidSet;
use rsky_repo::types::{CommitAction, CommitData, CommitOp};
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
async fn start_emits_sequenced_events_until_destroyed() {
    let (_dir, mut sequencer) = test_sequencer().await;
    let received: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
        std::sync::Arc::new(std::sync::Mutex::new(vec![]));
    {
        let received = received.clone();
        EVENT_EMITTER
            .write()
            .await
            .on("events", move |evts: Vec<String>| {
                received.lock().unwrap().extend(evts);
            });
    }

    let mut background = sequencer.clone();
    let handle = tokio::spawn(async move { background.start().await });
    // let the poll loop take its initial cursor before sequencing
    tokio::time::sleep(StdDuration::from_millis(500)).await;

    sequencer
        .sequence_identity_evt("did:plc:start-loop".to_owned(), None)
        .await
        .unwrap();

    let mut seen = false;
    for _ in 0..100 {
        if received
            .lock()
            .unwrap()
            .iter()
            .any(|evt| evt.contains("did:plc:start-loop"))
        {
            seen = true;
            break;
        }
        tokio::time::sleep(StdDuration::from_millis(50)).await;
    }
    assert!(seen, "sequenced event was not emitted by the poll loop");

    assert!(!sequencer.is_destroyed());
    sequencer.destroy().await;
    assert!(sequencer.is_destroyed());
    let res = tokio::time::timeout(StdDuration::from_secs(5), handle)
        .await
        .expect("sequencer poll loop did not stop after destroy")
        .unwrap();
    assert!(res.is_ok());
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
