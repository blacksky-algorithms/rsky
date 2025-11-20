#[cfg(test)]
mod ingester_tests {
    use crate::ingester::IngesterManager;
    use crate::storage::Storage;
    use crate::types::{CommitData, FirehoseEvent};
    use ipld_core::ipld::Ipld;
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn setup_test_storage() -> (Storage, TempDir) {
        let temp_dir = TempDir::with_prefix("ingester_test_").unwrap();
        let db_path = temp_dir.path().join("test_db");
        let storage = Storage::new(Some(db_path)).unwrap();
        (storage, temp_dir)
    }

    #[test]
    fn test_parse_message_commit() {
        let mut commit_map = BTreeMap::new();
        commit_map.insert("seq".to_owned(), Ipld::Integer(12345));
        commit_map.insert(
            "repo".to_owned(),
            Ipld::String("did:plc:test123".to_owned()),
        );
        commit_map.insert(
            "time".to_owned(),
            Ipld::String("2024-01-01T00:00:00Z".to_owned()),
        );
        commit_map.insert("rev".to_owned(), Ipld::String("test-rev".to_owned()));
        commit_map.insert("blocks".to_owned(), Ipld::Bytes(vec![1, 2, 3, 4]));

        let mut msg_map = BTreeMap::new();
        msg_map.insert("t".to_owned(), Ipld::String("#commit".to_owned()));
        msg_map.insert("op".to_owned(), Ipld::Map(commit_map));

        let msg_ipld = Ipld::Map(msg_map);
        let msg_bytes = serde_ipld_dagcbor::to_vec(&msg_ipld).unwrap();

        let result = IngesterManager::parse_message(&msg_bytes).unwrap();

        assert!(result.is_some());
        let event = result.unwrap();
        assert_eq!(event.seq, 12345);
        assert_eq!(event.did, "did:plc:test123");
        assert_eq!(event.time, "2024-01-01T00:00:00Z");
        assert_eq!(event.kind, "commit");
        assert!(event.commit.is_some());

        let commit = event.commit.unwrap();
        assert_eq!(commit.rev, "test-rev");
        assert_eq!(commit.blocks, vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_parse_message_non_commit() {
        let mut msg_map = BTreeMap::new();
        msg_map.insert("t".to_owned(), Ipld::String("#info".to_owned()));

        let msg_ipld = Ipld::Map(msg_map);
        let msg_bytes = serde_ipld_dagcbor::to_vec(&msg_ipld).unwrap();

        let result = IngesterManager::parse_message(&msg_bytes).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_message_missing_seq() {
        let mut commit_map = BTreeMap::new();
        commit_map.insert(
            "repo".to_owned(),
            Ipld::String("did:plc:test123".to_owned()),
        );
        commit_map.insert(
            "time".to_owned(),
            Ipld::String("2024-01-01T00:00:00Z".to_owned()),
        );

        let mut msg_map = BTreeMap::new();
        msg_map.insert("t".to_owned(), Ipld::String("#commit".to_owned()));
        msg_map.insert("op".to_owned(), Ipld::Map(commit_map));

        let msg_ipld = Ipld::Map(msg_map);
        let msg_bytes = serde_ipld_dagcbor::to_vec(&msg_ipld).unwrap();

        let result = IngesterManager::parse_message(&msg_bytes);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing seq"));
    }

    #[test]
    fn test_parse_message_missing_repo() {
        let mut commit_map = BTreeMap::new();
        commit_map.insert("seq".to_owned(), Ipld::Integer(12345));
        commit_map.insert(
            "time".to_owned(),
            Ipld::String("2024-01-01T00:00:00Z".to_owned()),
        );

        let mut msg_map = BTreeMap::new();
        msg_map.insert("t".to_owned(), Ipld::String("#commit".to_owned()));
        msg_map.insert("op".to_owned(), Ipld::Map(commit_map));

        let msg_ipld = Ipld::Map(msg_map);
        let msg_bytes = serde_ipld_dagcbor::to_vec(&msg_ipld).unwrap();

        let result = IngesterManager::parse_message(&msg_bytes);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing repo"));
    }

    #[test]
    fn test_parse_message_missing_time() {
        let mut commit_map = BTreeMap::new();
        commit_map.insert("seq".to_owned(), Ipld::Integer(12345));
        commit_map.insert(
            "repo".to_owned(),
            Ipld::String("did:plc:test123".to_owned()),
        );

        let mut msg_map = BTreeMap::new();
        msg_map.insert("t".to_owned(), Ipld::String("#commit".to_owned()));
        msg_map.insert("op".to_owned(), Ipld::Map(commit_map));

        let msg_ipld = Ipld::Map(msg_map);
        let msg_bytes = serde_ipld_dagcbor::to_vec(&msg_ipld).unwrap();

        let result = IngesterManager::parse_message(&msg_bytes);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing time"));
    }

    #[test]
    fn test_parse_message_missing_rev() {
        let mut commit_map = BTreeMap::new();
        commit_map.insert("seq".to_owned(), Ipld::Integer(12345));
        commit_map.insert(
            "repo".to_owned(),
            Ipld::String("did:plc:test123".to_owned()),
        );
        commit_map.insert(
            "time".to_owned(),
            Ipld::String("2024-01-01T00:00:00Z".to_owned()),
        );

        let mut msg_map = BTreeMap::new();
        msg_map.insert("t".to_owned(), Ipld::String("#commit".to_owned()));
        msg_map.insert("op".to_owned(), Ipld::Map(commit_map));

        let msg_ipld = Ipld::Map(msg_map);
        let msg_bytes = serde_ipld_dagcbor::to_vec(&msg_ipld).unwrap();

        let result = IngesterManager::parse_message(&msg_bytes);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing rev"));
    }

    #[test]
    fn test_parse_message_empty_blocks() {
        let mut commit_map = BTreeMap::new();
        commit_map.insert("seq".to_owned(), Ipld::Integer(12345));
        commit_map.insert(
            "repo".to_owned(),
            Ipld::String("did:plc:test123".to_owned()),
        );
        commit_map.insert(
            "time".to_owned(),
            Ipld::String("2024-01-01T00:00:00Z".to_owned()),
        );
        commit_map.insert("rev".to_owned(), Ipld::String("test-rev".to_owned()));

        let mut msg_map = BTreeMap::new();
        msg_map.insert("t".to_owned(), Ipld::String("#commit".to_owned()));
        msg_map.insert("op".to_owned(), Ipld::Map(commit_map));

        let msg_ipld = Ipld::Map(msg_map);
        let msg_bytes = serde_ipld_dagcbor::to_vec(&msg_ipld).unwrap();

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
        let mut commit_map = BTreeMap::new();
        commit_map.insert("seq".to_owned(), Ipld::Integer(9_999_999_999));
        commit_map.insert(
            "repo".to_owned(),
            Ipld::String("did:plc:large-seq".to_owned()),
        );
        commit_map.insert(
            "time".to_owned(),
            Ipld::String("2024-12-31T23:59:59Z".to_owned()),
        );
        commit_map.insert("rev".to_owned(), Ipld::String("large-rev".to_owned()));

        let mut msg_map = BTreeMap::new();
        msg_map.insert("t".to_owned(), Ipld::String("#commit".to_owned()));
        msg_map.insert("op".to_owned(), Ipld::Map(commit_map));

        let msg_ipld = Ipld::Map(msg_map);
        let msg_bytes = serde_ipld_dagcbor::to_vec(&msg_ipld).unwrap();

        let result = IngesterManager::parse_message(&msg_bytes).unwrap();
        assert!(result.is_some());

        let event = result.unwrap();
        assert_eq!(event.seq, 9_999_999_999);
    }

    #[test]
    fn test_parse_message_with_large_blocks() {
        let large_blocks = vec![0u8; 1024 * 100]; // 100KB

        let mut commit_map = BTreeMap::new();
        commit_map.insert("seq".to_owned(), Ipld::Integer(123));
        commit_map.insert(
            "repo".to_owned(),
            Ipld::String("did:plc:large-blocks".to_owned()),
        );
        commit_map.insert(
            "time".to_owned(),
            Ipld::String("2024-01-01T00:00:00Z".to_owned()),
        );
        commit_map.insert("rev".to_owned(), Ipld::String("test-rev".to_owned()));
        commit_map.insert("blocks".to_owned(), Ipld::Bytes(large_blocks.clone()));

        let mut msg_map = BTreeMap::new();
        msg_map.insert("t".to_owned(), Ipld::String("#commit".to_owned()));
        msg_map.insert("op".to_owned(), Ipld::Map(commit_map));

        let msg_ipld = Ipld::Map(msg_map);
        let msg_bytes = serde_ipld_dagcbor::to_vec(&msg_ipld).unwrap();

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

    #[test]
    fn test_parse_label_message_valid() {
        // Create a valid label message
        let label_map = {
            let mut m = BTreeMap::new();
            m.insert(
                "src".to_owned(),
                Ipld::String("did:plc:labeler123".to_owned()),
            );
            m.insert(
                "uri".to_owned(),
                Ipld::String("at://did:plc:user456/app.bsky.feed.post/abc123".to_owned()),
            );
            m.insert("val".to_owned(), Ipld::String("spam".to_owned()));
            m.insert(
                "cts".to_owned(),
                Ipld::String("2025-01-20T10:30:00Z".to_owned()),
            );
            m
        };

        let labels_list = Ipld::List(vec![Ipld::Map(label_map)]);

        let mut op_map = BTreeMap::new();
        op_map.insert("seq".to_owned(), Ipld::Integer(12345));
        op_map.insert("labels".to_owned(), labels_list);

        let mut msg_map = BTreeMap::new();
        msg_map.insert("t".to_owned(), Ipld::String("#labels".to_owned()));
        msg_map.insert("op".to_owned(), Ipld::Map(op_map));

        let msg_ipld = Ipld::Map(msg_map);
        let msg_bytes = serde_ipld_dagcbor::to_vec(&msg_ipld).unwrap();

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
        // Create multiple labels in one message
        let label1 = {
            let mut m = BTreeMap::new();
            m.insert("src".to_owned(), Ipld::String("did:plc:labeler".to_owned()));
            m.insert(
                "uri".to_owned(),
                Ipld::String("at://did:plc:user/app.bsky.feed.post/1".to_owned()),
            );
            m.insert("val".to_owned(), Ipld::String("spam".to_owned()));
            m.insert(
                "cts".to_owned(),
                Ipld::String("2025-01-20T10:00:00Z".to_owned()),
            );
            m
        };

        let label2 = {
            let mut m = BTreeMap::new();
            m.insert("src".to_owned(), Ipld::String("did:plc:labeler".to_owned()));
            m.insert(
                "uri".to_owned(),
                Ipld::String("at://did:plc:user/app.bsky.feed.post/2".to_owned()),
            );
            m.insert("val".to_owned(), Ipld::String("nsfw".to_owned()));
            m.insert(
                "cts".to_owned(),
                Ipld::String("2025-01-20T10:01:00Z".to_owned()),
            );
            m
        };

        let labels_list = Ipld::List(vec![Ipld::Map(label1), Ipld::Map(label2)]);

        let mut op_map = BTreeMap::new();
        op_map.insert("seq".to_owned(), Ipld::Integer(67890));
        op_map.insert("labels".to_owned(), labels_list);

        let mut msg_map = BTreeMap::new();
        msg_map.insert("t".to_owned(), Ipld::String("#labels".to_owned()));
        msg_map.insert("op".to_owned(), Ipld::Map(op_map));

        let msg_ipld = Ipld::Map(msg_map);
        let msg_bytes = serde_ipld_dagcbor::to_vec(&msg_ipld).unwrap();

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
        // Create a message with wrong type
        let mut msg_map = BTreeMap::new();
        msg_map.insert("t".to_owned(), Ipld::String("#info".to_owned()));

        let msg_ipld = Ipld::Map(msg_map);
        let msg_bytes = serde_ipld_dagcbor::to_vec(&msg_ipld).unwrap();

        let result = crate::ingester::labels::parse_label_message(&msg_bytes).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_label_message_missing_seq() {
        let label_map = {
            let mut m = BTreeMap::new();
            m.insert("src".to_owned(), Ipld::String("did:plc:labeler".to_owned()));
            m.insert("uri".to_owned(), Ipld::String("at://test".to_owned()));
            m.insert("val".to_owned(), Ipld::String("spam".to_owned()));
            m.insert(
                "cts".to_owned(),
                Ipld::String("2025-01-20T10:00:00Z".to_owned()),
            );
            m
        };

        let labels_list = Ipld::List(vec![Ipld::Map(label_map)]);

        let mut op_map = BTreeMap::new();
        // No seq field
        op_map.insert("labels".to_owned(), labels_list);

        let mut msg_map = BTreeMap::new();
        msg_map.insert("t".to_owned(), Ipld::String("#labels".to_owned()));
        msg_map.insert("op".to_owned(), Ipld::Map(op_map));

        let msg_ipld = Ipld::Map(msg_map);
        let msg_bytes = serde_ipld_dagcbor::to_vec(&msg_ipld).unwrap();

        let result = crate::ingester::labels::parse_label_message(&msg_bytes);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing seq"));
    }

    #[test]
    fn test_parse_label_message_missing_labels_field() {
        let mut op_map = BTreeMap::new();
        op_map.insert("seq".to_owned(), Ipld::Integer(12345));
        // No labels field

        let mut msg_map = BTreeMap::new();
        msg_map.insert("t".to_owned(), Ipld::String("#labels".to_owned()));
        msg_map.insert("op".to_owned(), Ipld::Map(op_map));

        let msg_ipld = Ipld::Map(msg_map);
        let msg_bytes = serde_ipld_dagcbor::to_vec(&msg_ipld).unwrap();

        let result = crate::ingester::labels::parse_label_message(&msg_bytes);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing labels"));
    }

    #[test]
    fn test_parse_label_message_missing_src() {
        let label_map = {
            let mut m = BTreeMap::new();
            // No src field
            m.insert("uri".to_owned(), Ipld::String("at://test".to_owned()));
            m.insert("val".to_owned(), Ipld::String("spam".to_owned()));
            m.insert(
                "cts".to_owned(),
                Ipld::String("2025-01-20T10:00:00Z".to_owned()),
            );
            m
        };

        let labels_list = Ipld::List(vec![Ipld::Map(label_map)]);

        let mut op_map = BTreeMap::new();
        op_map.insert("seq".to_owned(), Ipld::Integer(12345));
        op_map.insert("labels".to_owned(), labels_list);

        let mut msg_map = BTreeMap::new();
        msg_map.insert("t".to_owned(), Ipld::String("#labels".to_owned()));
        msg_map.insert("op".to_owned(), Ipld::Map(op_map));

        let msg_ipld = Ipld::Map(msg_map);
        let msg_bytes = serde_ipld_dagcbor::to_vec(&msg_ipld).unwrap();

        let result = crate::ingester::labels::parse_label_message(&msg_bytes);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing src"));
    }

    #[test]
    fn test_parse_label_message_missing_uri() {
        let label_map = {
            let mut m = BTreeMap::new();
            m.insert("src".to_owned(), Ipld::String("did:plc:labeler".to_owned()));
            // No uri field
            m.insert("val".to_owned(), Ipld::String("spam".to_owned()));
            m.insert(
                "cts".to_owned(),
                Ipld::String("2025-01-20T10:00:00Z".to_owned()),
            );
            m
        };

        let labels_list = Ipld::List(vec![Ipld::Map(label_map)]);

        let mut op_map = BTreeMap::new();
        op_map.insert("seq".to_owned(), Ipld::Integer(12345));
        op_map.insert("labels".to_owned(), labels_list);

        let mut msg_map = BTreeMap::new();
        msg_map.insert("t".to_owned(), Ipld::String("#labels".to_owned()));
        msg_map.insert("op".to_owned(), Ipld::Map(op_map));

        let msg_ipld = Ipld::Map(msg_map);
        let msg_bytes = serde_ipld_dagcbor::to_vec(&msg_ipld).unwrap();

        let result = crate::ingester::labels::parse_label_message(&msg_bytes);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing uri"));
    }

    #[test]
    fn test_parse_label_message_missing_val() {
        let label_map = {
            let mut m = BTreeMap::new();
            m.insert("src".to_owned(), Ipld::String("did:plc:labeler".to_owned()));
            m.insert("uri".to_owned(), Ipld::String("at://test".to_owned()));
            // No val field
            m.insert(
                "cts".to_owned(),
                Ipld::String("2025-01-20T10:00:00Z".to_owned()),
            );
            m
        };

        let labels_list = Ipld::List(vec![Ipld::Map(label_map)]);

        let mut op_map = BTreeMap::new();
        op_map.insert("seq".to_owned(), Ipld::Integer(12345));
        op_map.insert("labels".to_owned(), labels_list);

        let mut msg_map = BTreeMap::new();
        msg_map.insert("t".to_owned(), Ipld::String("#labels".to_owned()));
        msg_map.insert("op".to_owned(), Ipld::Map(op_map));

        let msg_ipld = Ipld::Map(msg_map);
        let msg_bytes = serde_ipld_dagcbor::to_vec(&msg_ipld).unwrap();

        let result = crate::ingester::labels::parse_label_message(&msg_bytes);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing val"));
    }

    #[test]
    fn test_parse_label_message_missing_cts() {
        let label_map = {
            let mut m = BTreeMap::new();
            m.insert("src".to_owned(), Ipld::String("did:plc:labeler".to_owned()));
            m.insert("uri".to_owned(), Ipld::String("at://test".to_owned()));
            m.insert("val".to_owned(), Ipld::String("spam".to_owned()));
            // No cts field
            m
        };

        let labels_list = Ipld::List(vec![Ipld::Map(label_map)]);

        let mut op_map = BTreeMap::new();
        op_map.insert("seq".to_owned(), Ipld::Integer(12345));
        op_map.insert("labels".to_owned(), labels_list);

        let mut msg_map = BTreeMap::new();
        msg_map.insert("t".to_owned(), Ipld::String("#labels".to_owned()));
        msg_map.insert("op".to_owned(), Ipld::Map(op_map));

        let msg_ipld = Ipld::Map(msg_map);
        let msg_bytes = serde_ipld_dagcbor::to_vec(&msg_ipld).unwrap();

        let result = crate::ingester::labels::parse_label_message(&msg_bytes);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing cts"));
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

    #[test]
    fn test_parse_label_message_empty_labels_array() {
        let labels_list = Ipld::List(vec![]); // Empty array

        let mut op_map = BTreeMap::new();
        op_map.insert("seq".to_owned(), Ipld::Integer(12345));
        op_map.insert("labels".to_owned(), labels_list);

        let mut msg_map = BTreeMap::new();
        msg_map.insert("t".to_owned(), Ipld::String("#labels".to_owned()));
        msg_map.insert("op".to_owned(), Ipld::Map(op_map));

        let msg_ipld = Ipld::Map(msg_map);
        let msg_bytes = serde_ipld_dagcbor::to_vec(&msg_ipld).unwrap();

        let result = crate::ingester::labels::parse_label_message(&msg_bytes).unwrap();
        assert!(result.is_some());
        let label_event = result.unwrap();
        assert_eq!(label_event.labels.len(), 0);
    }
}
