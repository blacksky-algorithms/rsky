#[cfg(test)]
mod ingester_tests {
    use crate::ingester::IngesterManager;
    use crate::storage::Storage;
    use crate::types::{CommitData, FirehoseEvent};
    use std::sync::Arc;
    use tempfile::TempDir;

    fn setup_test_storage() -> (Storage, TempDir) {
        let temp_dir = TempDir::with_prefix("ingester_test_").unwrap();
        let db_path = temp_dir.path().join("test_db");
        let storage = Storage::new(Some(db_path)).unwrap();
        (storage, temp_dir)
    }

    // Helper to create properly formatted test messages with header + body
    fn create_commit_message(
        seq: i64,
        repo: &str,
        time: &str,
        rev: &str,
        blocks: Vec<u8>,
        ops: Vec<(&str, &str, Option<&str>)>, // (action, path, cid)
    ) -> Vec<u8> {
        use chrono::{DateTime, Utc};
        use lexicon_cid::Cid;
        use rsky_lexicon::com::atproto::sync::{
            SubscribeReposCommit, SubscribeReposCommitOperation,
        };

        #[derive(serde::Serialize)]
        struct Header {
            t: String,
            op: u8,
        }

        let header = Header {
            t: "#commit".to_owned(),
            op: 1,
        };

        let commit = SubscribeReposCommit {
            seq,
            time: time.parse::<DateTime<Utc>>().unwrap(),
            rebase: false,
            too_big: false,
            repo: repo.to_owned(),
            commit: Cid::try_from("bafyreihzwnyumvubacqyflkxpsejegc6sxwkcaxv3iwm3lrn3x45gxkioa")
                .unwrap(),
            prev: None,
            rev: rev.to_owned(),
            since: None,
            blocks,
            ops: ops
                .into_iter()
                .map(|(action, path, cid)| SubscribeReposCommitOperation {
                    action: action.to_owned(),
                    path: path.to_owned(),
                    cid: cid.and_then(|c| Cid::try_from(c).ok()),
                })
                .collect(),
            blobs: vec![],
        };

        let mut result = Vec::new();
        ciborium::ser::into_writer(&header, &mut result).unwrap();
        serde_ipld_dagcbor::to_writer(&mut result, &commit).unwrap();
        result
    }

    #[test]
    fn test_parse_message_commit() {
        let msg_bytes = create_commit_message(
            12345,
            "did:plc:test123",
            "2024-01-01T00:00:00Z",
            "test-rev",
            vec![1, 2, 3, 4],
            vec![],
        );

        let result = IngesterManager::parse_message(&msg_bytes).unwrap();

        assert!(result.is_some());
        let event = result.unwrap();
        assert_eq!(event.seq, 12345);
        assert_eq!(event.did, "did:plc:test123");
        assert_eq!(event.time, "2024-01-01T00:00:00+00:00");
        assert_eq!(event.kind, "commit");
        assert!(event.commit.is_some());

        let commit = event.commit.unwrap();
        assert_eq!(commit.rev, "test-rev");
        assert_eq!(commit.blocks, vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_parse_message_with_operations() {
        let msg_bytes = create_commit_message(
            54321,
            "did:plc:user456",
            "2024-01-15T10:30:00Z",
            "rev-abc123",
            vec![],
            vec![
                (
                    "create",
                    "app.bsky.feed.post/abc123",
                    Some("bafyreihzwnyumvubacqyflkxpsejegc6sxwkcaxv3iwm3lrn3x45gxkioa"),
                ),
                ("update", "app.bsky.actor.profile/self", None),
                ("delete", "app.bsky.feed.like/xyz789", None),
            ],
        );

        let result = IngesterManager::parse_message(&msg_bytes).unwrap();
        assert!(result.is_some());

        let event = result.unwrap();
        assert_eq!(event.seq, 54321);
        assert!(event.commit.is_some());

        let commit = event.commit.unwrap();
        assert_eq!(commit.ops.len(), 3);

        // Verify create operation
        assert_eq!(commit.ops[0].action, "create");
        assert_eq!(commit.ops[0].path, "app.bsky.feed.post/abc123");
        assert!(commit.ops[0].cid.is_some());

        // Verify update operation
        assert_eq!(commit.ops[1].action, "update");
        assert_eq!(commit.ops[1].path, "app.bsky.actor.profile/self");

        // Verify delete operation
        assert_eq!(commit.ops[2].action, "delete");
        assert_eq!(commit.ops[2].path, "app.bsky.feed.like/xyz789");
    }

    #[test]
    fn test_parse_message_non_commit() {
        #[derive(serde::Serialize)]
        struct Header {
            t: String,
            op: u8,
        }

        let header = Header {
            t: "#info".to_owned(),
            op: 1,
        };

        let mut msg_bytes = Vec::new();
        ciborium::ser::into_writer(&header, &mut msg_bytes).unwrap();

        let result = IngesterManager::parse_message(&msg_bytes).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_message_empty_blocks() {
        let msg_bytes = create_commit_message(
            12345,
            "did:plc:test123",
            "2024-01-01T00:00:00Z",
            "test-rev",
            vec![], // Empty blocks
            vec![],
        );

        let result = IngesterManager::parse_message(&msg_bytes).unwrap();
        assert!(result.is_some());

        let event = result.unwrap();
        let commit = event.commit.unwrap();
        assert!(commit.blocks.is_empty());
    }

    #[test]
    fn test_parse_message_invalid_cbor() {
        let invalid_bytes = vec![0xFF, 0xFE, 0xFD, 0xFC];
        let result = IngesterManager::parse_message(&invalid_bytes);
        assert!(result.is_err());
    }

    #[test]
    fn test_firehose_event_storage_roundtrip() {
        let (storage, _dir) = setup_test_storage();

        let event = FirehoseEvent {
            seq: 99999,
            did: "did:plc:roundtrip".to_owned(),
            time: "2024-01-01T12:00:00Z".to_owned(),
            kind: "commit".to_owned(),
            commit: Some(CommitData {
                rev: "test-rev-123".to_owned(),
                ops: vec![],
                blocks: vec![10, 20, 30],
            }),
        };

        storage.write_firehose_event(event.seq, &event).unwrap();

        let retrieved = storage.read_firehose_event(99999).unwrap();
        assert!(retrieved.is_some());

        let retrieved_event = retrieved.unwrap();
        assert_eq!(retrieved_event.seq, event.seq);
        assert_eq!(retrieved_event.did, event.did);
        assert_eq!(retrieved_event.time, event.time);
        assert_eq!(retrieved_event.kind, event.kind);

        let retrieved_commit = retrieved_event.commit.unwrap();
        let original_commit = event.commit.unwrap();
        assert_eq!(retrieved_commit.rev, original_commit.rev);
        assert_eq!(retrieved_commit.blocks, original_commit.blocks);
    }

    #[test]
    fn test_cursor_management() {
        let (storage, _dir) = setup_test_storage();

        let cursor_key = "firehose:test-relay";

        assert!(storage.get_cursor(cursor_key).unwrap().is_none());

        storage.set_cursor(cursor_key, 1000).unwrap();
        assert_eq!(storage.get_cursor(cursor_key).unwrap(), Some(1000));

        storage.set_cursor(cursor_key, 2500).unwrap();
        assert_eq!(storage.get_cursor(cursor_key).unwrap(), Some(2500));

        storage.set_cursor(cursor_key, 0).unwrap();
        assert_eq!(storage.get_cursor(cursor_key).unwrap(), Some(0));
    }

    #[test]
    fn test_multiple_cursors() {
        let (storage, _dir) = setup_test_storage();

        storage.set_cursor("firehose:relay1", 100).unwrap();
        storage.set_cursor("firehose:relay2", 200).unwrap();
        storage.set_cursor("backfill_enum:relay1", 300).unwrap();

        assert_eq!(storage.get_cursor("firehose:relay1").unwrap(), Some(100));
        assert_eq!(storage.get_cursor("firehose:relay2").unwrap(), Some(200));
        assert_eq!(
            storage.get_cursor("backfill_enum:relay1").unwrap(),
            Some(300)
        );
    }

    #[test]
    fn test_ingester_manager_creation() {
        let (storage, _dir) = setup_test_storage();

        let relay_hosts = vec!["https://relay.example.com".to_owned()];
        let labeler_hosts = vec!["https://labeler.example.com".to_owned()];

        let manager = IngesterManager::new(relay_hosts, labeler_hosts, Arc::new(storage));

        assert!(manager.is_ok());
    }

    #[test]
    fn test_parse_message_with_large_seq() {
        let msg_bytes = create_commit_message(
            9_999_999_999,
            "did:plc:large-seq",
            "2024-12-31T23:59:59Z",
            "large-rev",
            vec![],
            vec![],
        );

        let result = IngesterManager::parse_message(&msg_bytes).unwrap();
        assert!(result.is_some());

        let event = result.unwrap();
        assert_eq!(event.seq, 9_999_999_999);
    }

    #[test]
    fn test_parse_message_with_large_blocks() {
        let large_blocks = vec![0u8; 1024 * 100]; // 100KB

        let msg_bytes = create_commit_message(
            123,
            "did:plc:large-blocks",
            "2024-01-01T00:00:00Z",
            "test-rev",
            large_blocks.clone(),
            vec![],
        );

        let result = IngesterManager::parse_message(&msg_bytes).unwrap();
        assert!(result.is_some());

        let event = result.unwrap();
        let commit = event.commit.unwrap();
        assert_eq!(commit.blocks.len(), large_blocks.len());
    }

    #[test]
    fn test_firehose_event_persistence() {
        let (storage, _dir) = setup_test_storage();

        let events = vec![
            FirehoseEvent {
                seq: 1,
                did: "did:plc:user1".to_owned(),
                time: "2024-01-01T00:00:00Z".to_owned(),
                kind: "commit".to_owned(),
                commit: Some(CommitData {
                    rev: "rev1".to_owned(),
                    ops: vec![],
                    blocks: vec![],
                }),
            },
            FirehoseEvent {
                seq: 2,
                did: "did:plc:user2".to_owned(),
                time: "2024-01-01T00:00:01Z".to_owned(),
                kind: "commit".to_owned(),
                commit: Some(CommitData {
                    rev: "rev2".to_owned(),
                    ops: vec![],
                    blocks: vec![],
                }),
            },
            FirehoseEvent {
                seq: 3,
                did: "did:plc:user3".to_owned(),
                time: "2024-01-01T00:00:02Z".to_owned(),
                kind: "commit".to_owned(),
                commit: Some(CommitData {
                    rev: "rev3".to_owned(),
                    ops: vec![],
                    blocks: vec![],
                }),
            },
        ];

        for event in &events {
            storage.write_firehose_event(event.seq, event).unwrap();
        }

        for event in &events {
            let retrieved = storage.read_firehose_event(event.seq).unwrap();
            assert!(retrieved.is_some());
            let retrieved_event = retrieved.unwrap();
            assert_eq!(retrieved_event.seq, event.seq);
            assert_eq!(retrieved_event.did, event.did);
        }
    }

    #[test]
    fn test_cursor_for_different_relays() {
        let (storage, _dir) = setup_test_storage();

        let relay1 = "https://relay1.bsky.network";
        let relay2 = "https://relay2.bsky.network";

        let cursor1_key = format!("firehose:{relay1}");
        let cursor2_key = format!("firehose:{relay2}");

        storage.set_cursor(&cursor1_key, 5000).unwrap();
        storage.set_cursor(&cursor2_key, 3000).unwrap();

        assert_eq!(storage.get_cursor(&cursor1_key).unwrap(), Some(5000));
        assert_eq!(storage.get_cursor(&cursor2_key).unwrap(), Some(3000));

        storage.set_cursor(&cursor1_key, 5100).unwrap();

        assert_eq!(storage.get_cursor(&cursor1_key).unwrap(), Some(5100));
        assert_eq!(storage.get_cursor(&cursor2_key).unwrap(), Some(3000));
    }

    // =============================================================================
    // LABELS TESTS
    // =============================================================================

    // Helper to create properly formatted label messages with header + body
    fn create_label_message(
        seq: i64,
        labels: Vec<(&str, &str, &str, &str)>, // (src, uri, val, cts)
    ) -> Vec<u8> {
        #[derive(serde::Serialize)]
        struct Header {
            t: String,
            op: u8,
        }

        #[derive(serde::Serialize)]
        struct RawLabel {
            src: String,
            uri: String,
            val: String,
            cts: String,
        }

        #[derive(serde::Serialize)]
        struct SubscribeLabels {
            seq: i64,
            labels: Vec<RawLabel>,
        }

        let header = Header {
            t: "#labels".to_owned(),
            op: 1,
        };

        let body = SubscribeLabels {
            seq,
            labels: labels
                .into_iter()
                .map(|(src, uri, val, cts)| RawLabel {
                    src: src.to_owned(),
                    uri: uri.to_owned(),
                    val: val.to_owned(),
                    cts: cts.to_owned(),
                })
                .collect(),
        };

        let mut result = Vec::new();
        ciborium::ser::into_writer(&header, &mut result).unwrap();
        serde_ipld_dagcbor::to_writer(&mut result, &body).unwrap();
        result
    }

    #[test]
    fn test_parse_label_message_valid() {
        let msg_bytes = create_label_message(
            12345,
            vec![(
                "did:plc:labeler123",
                "at://did:plc:user456/app.bsky.feed.post/abc123",
                "spam",
                "2025-01-20T10:30:00Z",
            )],
        );

        let result = crate::ingester::labels::parse_label_message(&msg_bytes).unwrap();

        assert!(result.is_some());
        let label_event = result.unwrap();
        assert_eq!(label_event.seq, 12345);
        assert_eq!(label_event.labels.len(), 1);

        let label = &label_event.labels[0];
        assert_eq!(label.src, "did:plc:labeler123");
        assert_eq!(label.uri, "at://did:plc:user456/app.bsky.feed.post/abc123");
        assert_eq!(label.val, "spam");
        assert_eq!(label.cts, "2025-01-20T10:30:00Z");
    }

    #[test]
    fn test_parse_label_message_multiple_labels() {
        let msg_bytes = create_label_message(
            67890,
            vec![
                (
                    "did:plc:labeler",
                    "at://did:plc:user/app.bsky.feed.post/1",
                    "spam",
                    "2025-01-20T10:00:00Z",
                ),
                (
                    "did:plc:labeler",
                    "at://did:plc:user/app.bsky.feed.post/2",
                    "nsfw",
                    "2025-01-20T10:01:00Z",
                ),
            ],
        );

        let result = crate::ingester::labels::parse_label_message(&msg_bytes).unwrap();

        assert!(result.is_some());
        let label_event = result.unwrap();
        assert_eq!(label_event.seq, 67890);
        assert_eq!(label_event.labels.len(), 2);

        assert_eq!(label_event.labels[0].val, "spam");
        assert_eq!(label_event.labels[1].val, "nsfw");
    }

    #[test]
    fn test_parse_label_message_non_labels() {
        // Create a message with wrong type (using ciborium directly)
        #[derive(serde::Serialize)]
        struct Header {
            t: String,
            op: u8,
        }

        let header = Header {
            t: "#info".to_owned(),
            op: 1,
        };

        let mut msg_bytes = Vec::new();
        ciborium::ser::into_writer(&header, &mut msg_bytes).unwrap();

        let result = crate::ingester::labels::parse_label_message(&msg_bytes).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_label_message_empty_labels_array() {
        let msg_bytes = create_label_message(12345, vec![]);

        let result = crate::ingester::labels::parse_label_message(&msg_bytes).unwrap();
        assert!(result.is_some());
        let label_event = result.unwrap();
        assert_eq!(label_event.labels.len(), 0);
    }

    #[test]
    fn test_label_storage_roundtrip() {
        let (storage, _dir) = setup_test_storage();

        let label_event = crate::types::LabelEvent {
            seq: 54321,
            labels: vec![
                crate::types::Label {
                    src: "did:plc:labeler".to_owned(),
                    uri: "at://did:plc:user/app.bsky.feed.post/abc".to_owned(),
                    val: "spam".to_owned(),
                    cts: "2025-01-20T10:00:00Z".to_owned(),
                },
                crate::types::Label {
                    src: "did:plc:labeler".to_owned(),
                    uri: "at://did:plc:user/app.bsky.feed.post/def".to_owned(),
                    val: "nsfw".to_owned(),
                    cts: "2025-01-20T10:01:00Z".to_owned(),
                },
            ],
        };

        // Enqueue the label event
        storage.enqueue_label_live(&label_event).unwrap();

        // Check queue length increased
        assert!(storage.label_live_len().unwrap() > 0);

        // Dequeue the label event
        let dequeued = storage.dequeue_label_live().unwrap();
        assert!(dequeued.is_some());

        let (key, retrieved_event) = dequeued.unwrap();
        assert_eq!(retrieved_event.seq, label_event.seq);
        assert_eq!(retrieved_event.labels.len(), label_event.labels.len());
        assert_eq!(retrieved_event.labels[0].src, label_event.labels[0].src);
        assert_eq!(retrieved_event.labels[0].val, label_event.labels[0].val);

        // Remove from queue
        storage.remove_label_live(&key).unwrap();

        // Queue should be empty now
        let empty = storage.dequeue_label_live().unwrap();
        assert!(empty.is_none());
    }

    #[test]
    fn test_label_cursor_management() {
        let (storage, _dir) = setup_test_storage();

        let cursor_key = "labels:https://mod.bsky.app";

        // Initially no cursor
        assert!(storage.get_cursor(cursor_key).unwrap().is_none());

        // Set cursor
        storage.set_cursor(cursor_key, 10000).unwrap();
        assert_eq!(storage.get_cursor(cursor_key).unwrap(), Some(10000));

        // Update cursor
        storage.set_cursor(cursor_key, 20000).unwrap();
        assert_eq!(storage.get_cursor(cursor_key).unwrap(), Some(20000));
    }

    #[test]
    fn test_multiple_label_cursors() {
        let (storage, _dir) = setup_test_storage();

        storage
            .set_cursor("labels:https://mod.bsky.app", 1000)
            .unwrap();
        storage
            .set_cursor("labels:https://custom-labeler.example", 2000)
            .unwrap();

        assert_eq!(
            storage.get_cursor("labels:https://mod.bsky.app").unwrap(),
            Some(1000)
        );
        assert_eq!(
            storage
                .get_cursor("labels:https://custom-labeler.example")
                .unwrap(),
            Some(2000)
        );
    }

    #[tokio::test]
    #[ignore]
    async fn test_live_firehose_operation_parsing() {
        use futures::stream::StreamExt;
        use tokio::time::{Duration, timeout};
        use tokio_tungstenite::connect_async;
        use tokio_tungstenite::tungstenite::Message;

        // Initialize logging for debugging
        drop(tracing_subscriber::fmt().with_env_filter("info").try_init());

        // Connect to live firehose stream
        let url = "wss://bsky.network/xrpc/com.atproto.sync.subscribeRepos";
        tracing::info!("connecting to live firehose stream: {url}");

        let (ws_stream, _) = connect_async(url)
            .await
            .expect("failed to connect to bsky.network firehose");

        let (_write, mut read) = ws_stream.split();

        tracing::info!("listening to firehose for 5 seconds...");

        let mut total_messages = 0;
        let mut total_creates = 0;
        let mut total_updates = 0;
        let mut total_deletes = 0;
        let mut total_operations = 0;

        // Collect messages for 5 seconds
        let collection_result = timeout(Duration::from_secs(5), async {
            while let Some(msg_result) = read.next().await {
                if let Ok(Message::Binary(data)) = msg_result {
                    total_messages += 1;

                    // Parse the message
                    if let Ok(Some(event)) = IngesterManager::parse_message(&data) {
                        if let Some(commit) = event.commit {
                            // Count operation types
                            for op in commit.ops {
                                total_operations += 1;
                                match op.action.as_str() {
                                    "create" => total_creates += 1,
                                    "update" => total_updates += 1,
                                    "delete" => total_deletes += 1,
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
        })
        .await;

        // Timeout is expected after 5 seconds
        drop(collection_result);

        // Print operation statistics for debugging
        tracing::info!(
            "disconnected from stream. received {total_messages} messages with {total_operations} total operations"
        );
        tracing::info!(
            "operation breakdown: {total_creates} creates, {total_updates} updates, {total_deletes} deletes"
        );

        // Validate that we received data
        assert!(
            total_messages > 0,
            "expected to receive at least one message from live firehose"
        );
        assert!(
            total_operations > 0,
            "expected to parse at least one operation from live firehose"
        );

        // User stated: "In any given 5 second window there will at least be create events"
        assert!(
            total_creates > 0,
            "expected at least one create operation in 5 second window, but got 0"
        );

        tracing::info!(
            "integration test complete - successfully parsed {total_operations} operations from live firehose"
        );
    }

    #[tokio::test]
    async fn test_event_to_index_jobs_conversion() {
        use crate::types::{CommitData, FirehoseEvent, RepoOp};

        let (storage, _dir) = setup_test_storage();

        // Create a firehose event with multiple operations
        let event = FirehoseEvent {
            seq: 12345,
            did: "did:plc:test123".to_owned(),
            time: "2024-01-01T00:00:00Z".to_owned(),
            kind: "commit".to_owned(),
            commit: Some(CommitData {
                rev: "rev-abc".to_owned(),
                ops: vec![
                    RepoOp {
                        action: "create".to_owned(),
                        path: "app.bsky.feed.post/abc123".to_owned(),
                        cid: Some(
                            "bafyreihzwnyumvubacqyflkxpsejegc6sxwkcaxv3iwm3lrn3x45gxkioa"
                                .to_owned(),
                        ),
                    },
                    RepoOp {
                        action: "update".to_owned(),
                        path: "app.bsky.actor.profile/self".to_owned(),
                        cid: Some(
                            "bafyreihzwnyumvubacqyflkxpsejegc6sxwkcaxv3iwm3lrn3x45gxkioa"
                                .to_owned(),
                        ),
                    },
                    RepoOp {
                        action: "delete".to_owned(),
                        path: "app.bsky.feed.like/xyz789".to_owned(),
                        cid: None,
                    },
                ],
                blocks: vec![],
            }),
        };

        // Queue should be empty initially
        assert_eq!(storage.firehose_live_len().unwrap(), 0);

        // Enqueue the event for indexing
        IngesterManager::enqueue_event_for_indexing(&storage, &event)
            .await
            .unwrap();

        // Should have 3 jobs (one for each operation)
        assert_eq!(storage.firehose_live_len().unwrap(), 3);

        // Dequeue and verify jobs (order not guaranteed)
        let mut jobs = Vec::new();
        while let Ok(Some((key, job))) = storage.dequeue_firehose_live() {
            jobs.push(job);
            storage.remove_firehose_live(&key).unwrap();
        }

        assert_eq!(jobs.len(), 3);

        // Verify we have one of each action type
        assert_eq!(
            jobs.iter()
                .filter(|j| matches!(j.action, crate::types::WriteAction::Create))
                .count(),
            1
        );
        assert_eq!(
            jobs.iter()
                .filter(|j| matches!(j.action, crate::types::WriteAction::Update))
                .count(),
            1
        );
        assert_eq!(
            jobs.iter()
                .filter(|j| matches!(j.action, crate::types::WriteAction::Delete))
                .count(),
            1
        );

        // Verify URIs are correct
        assert!(
            jobs.iter()
                .any(|j| j.uri == "at://did:plc:test123/app.bsky.feed.post/abc123")
        );
        assert!(
            jobs.iter()
                .any(|j| j.uri == "at://did:plc:test123/app.bsky.actor.profile/self")
        );
        assert!(
            jobs.iter()
                .any(|j| j.uri == "at://did:plc:test123/app.bsky.feed.like/xyz789")
        );

        // Queue should be empty again
        assert_eq!(storage.firehose_live_len().unwrap(), 0);
    }

    #[tokio::test]
    async fn test_event_with_no_operations() {
        use crate::types::{CommitData, FirehoseEvent};

        let (storage, _dir) = setup_test_storage();

        let event = FirehoseEvent {
            seq: 12345,
            did: "did:plc:test123".to_owned(),
            time: "2024-01-01T00:00:00Z".to_owned(),
            kind: "commit".to_owned(),
            commit: Some(CommitData {
                rev: "rev-abc".to_owned(),
                ops: vec![], // No operations
                blocks: vec![],
            }),
        };

        // Should succeed but not enqueue anything
        IngesterManager::enqueue_event_for_indexing(&storage, &event)
            .await
            .unwrap();
        assert_eq!(storage.firehose_live_len().unwrap(), 0);
    }

    #[tokio::test]
    async fn test_event_with_no_commit() {
        use crate::types::FirehoseEvent;

        let (storage, _dir) = setup_test_storage();

        let event = FirehoseEvent {
            seq: 12345,
            did: "did:plc:test123".to_owned(),
            time: "2024-01-01T00:00:00Z".to_owned(),
            kind: "identity".to_owned(),
            commit: None, // No commit data
        };

        // Should succeed but not enqueue anything
        IngesterManager::enqueue_event_for_indexing(&storage, &event)
            .await
            .unwrap();
        assert_eq!(storage.firehose_live_len().unwrap(), 0);
    }
}
