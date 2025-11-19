#[cfg(test)]
mod backfiller_tests {
    use crate::backfiller::{BackfillerManager, convert_record_to_ipld};
    use crate::storage::Storage;
    use crate::types::BackfillJob;
    use serde_json::json;
    use tempfile::TempDir;

    fn setup_test_storage() -> (Storage, TempDir) {
        let temp_dir = TempDir::with_prefix("backfiller_test_").unwrap();
        let db_path = temp_dir.path().join("test_db");
        crate::storage::DB_PATH.with(|p| *p.borrow_mut() = Some(db_path));
        let storage = Storage::new().unwrap();
        (storage, temp_dir)
    }

    #[test]
    fn test_convert_record_preserves_objects() {
        let input = json!({"text": "hello", "nested": {"key": "value"}});
        let output = convert_record_to_ipld(&input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_convert_record_converts_cid_bytes() {
        let cid_bytes = vec![1u8, 113, 18, 32];
        let mut full_bytes = cid_bytes;
        full_bytes.extend_from_slice(&[0u8; 28]);

        let input = json!({"data": full_bytes});
        let output = convert_record_to_ipld(&input);

        assert!(output.get("data").is_some());
    }

    #[test]
    fn test_convert_record_preserves_regular_arrays() {
        let input = json!({"tags": ["one", "two", "three"]});
        let output = convert_record_to_ipld(&input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_convert_record_handles_nested_structures() {
        let input = json!({
            "text": "post",
            "facets": [
                {"features": [{"$type": "mention"}]}
            ]
        });
        let output = convert_record_to_ipld(&input);
        assert_eq!(output["text"], "post");
    }

    #[tokio::test]
    async fn test_process_job_with_real_repo() {
        let (storage, _dir) = setup_test_storage();

        let job = BackfillJob {
            did: "did:plc:w4xbfzo7kqfes5zb7r6qv3rw".to_owned(),
            retry_count: 0,
        };

        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .unwrap();

        let result = BackfillerManager::process_job(&storage, &http_client, &job).await;

        match result {
            Ok(()) => {
                let queue_len = storage.index_queue_len().unwrap();
                assert!(
                    queue_len > 7000,
                    "expected more than 20000 records to be enqueued for indexing, found {queue_len}"
                );
            }
            Err(e) => {
                panic!("backfill job failed: {e}");
            }
        }
    }
}
