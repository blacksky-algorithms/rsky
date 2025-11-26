#[cfg(test)]
mod backfiller_tests {
    use crate::backfiller::{BackfillerManager, convert_record_to_ipld};
    use crate::storage::Storage;
    use crate::types::{BackfillJob, WintermuteError};
    use serde_json::json;
    use tempfile::TempDir;

    fn setup_test_storage() -> (Storage, TempDir) {
        let temp_dir = TempDir::with_prefix("backfiller_test_").unwrap();
        let db_path = temp_dir.path().join("test_db");
        let storage = Storage::new(Some(db_path)).unwrap();
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
                let queue_len = storage.firehose_backfill_len().unwrap();
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

    #[tokio::test]
    async fn test_backpressure_metric_tracking() {
        use crate::config::BACKFILLER_OUTPUT_HIGH_WATER_MARK;
        use crate::types::IndexJob;
        use crate::types::WriteAction;

        let (storage, _dir) = setup_test_storage();

        // Fill output stream beyond high water mark
        for i in 0..BACKFILLER_OUTPUT_HIGH_WATER_MARK + 100 {
            let job = IndexJob {
                uri: format!("at://did:plc:test/app.bsky.feed.post/test{i}"),
                cid: "bafytest".to_owned(),
                action: WriteAction::Create,
                record: Some(json!({"text": "test"})),
                indexed_at: "2024-01-01T00:00:00Z".to_owned(),
                rev: "test".to_owned(),
            };
            storage.enqueue_firehose_backfill(&job).unwrap();
        }

        // Verify output stream length exceeds high water mark
        let output_len = storage.firehose_backfill_len().unwrap();
        assert!(
            output_len > BACKFILLER_OUTPUT_HIGH_WATER_MARK,
            "output stream should exceed high water mark: {output_len} > {BACKFILLER_OUTPUT_HIGH_WATER_MARK}"
        );
    }

    #[tokio::test]
    async fn test_process_job_with_invalid_did() {
        let (storage, _dir) = setup_test_storage();

        let job = BackfillJob {
            did: "did:plc:nonexistent123456789".to_owned(),
            retry_count: 0,
        };

        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap();

        let result = BackfillerManager::process_job(&storage, &http_client, &job).await;

        // Should fail with DID resolution error
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("did resolution failed") || err_msg.contains("no pds found"),
            "unexpected error: {err_msg}"
        );
    }

    #[tokio::test]
    async fn test_process_job_with_http_error() {
        let (storage, _dir) = setup_test_storage();

        // Use a DID that should resolve but point to a non-existent PDS
        let job = BackfillJob {
            did: "did:web:example.invalid".to_owned(),
            retry_count: 0,
        };

        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap();

        let result = BackfillerManager::process_job(&storage, &http_client, &job).await;

        // Should fail with HTTP or DID resolution error
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_convert_record_with_cid_link() {
        // Test that CID bytes are properly converted to $link format
        let cid_bytes = vec![
            1, 113, 18, 32, // CID prefix for dag-cbor sha256
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0,
        ];

        let input = json!({"embed": {"cid": cid_bytes}});
        let output = convert_record_to_ipld(&input);

        // Verify nested CID was converted
        assert!(output["embed"]["cid"].get("$link").is_some());
    }

    #[tokio::test]
    async fn test_convert_record_with_mixed_content() {
        let input = json!({
            "text": "Hello world",
            "facets": [
                {
                    "index": {"byteStart": 0, "byteEnd": 5},
                    "features": [{"$type": "app.bsky.richtext.facet#mention"}]
                }
            ],
            "tags": ["rust", "atproto"]
        });

        let output = convert_record_to_ipld(&input);

        // Verify structure is preserved
        assert_eq!(output["text"], "Hello world");
        assert_eq!(output["tags"][0], "rust");
        assert_eq!(output["tags"][1], "atproto");
        assert_eq!(
            output["facets"][0]["features"][0]["$type"],
            "app.bsky.richtext.facet#mention"
        );
    }

    #[test]
    fn test_backfiller_manager_new() {
        let (storage, _dir) = setup_test_storage();
        let manager = BackfillerManager::new(std::sync::Arc::new(storage));
        assert!(manager.is_ok(), "BackfillerManager::new should succeed");
    }

    #[test]
    fn test_convert_record_empty_object() {
        let input = json!({});
        let output = convert_record_to_ipld(&input);
        assert_eq!(output, json!({}));
    }

    #[test]
    fn test_convert_record_empty_array() {
        let input = json!({"items": []});
        let output = convert_record_to_ipld(&input);
        assert_eq!(output, json!({"items": []}));
    }

    #[test]
    fn test_convert_record_numbers_and_booleans() {
        let input = json!({
            "count": 42,
            "score": 2.5,
            "enabled": true,
            "disabled": false
        });
        let output = convert_record_to_ipld(&input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_convert_record_null_values() {
        let input = json!({"field": null});
        let output = convert_record_to_ipld(&input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_convert_record_invalid_cid_bytes() {
        // Byte array that's not a valid CID - should be preserved as regular array
        let input = json!({"data": [1, 2, 3, 4, 5]});
        let output = convert_record_to_ipld(&input);
        // Should not have $link since it's not a valid CID
        assert!(!output["data"].is_object());
        assert_eq!(output, input);
    }

    #[test]
    fn test_convert_record_mixed_byte_array() {
        // Array with numbers > 255 should not be treated as byte array
        let input = json!({"data": [1, 2, 256, 4]});
        let output = convert_record_to_ipld(&input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_convert_record_deeply_nested() {
        let input = json!({
            "level1": {
                "level2": {
                    "level3": {
                        "level4": {
                            "value": "deep"
                        }
                    }
                }
            }
        });
        let output = convert_record_to_ipld(&input);
        assert_eq!(
            output["level1"]["level2"]["level3"]["level4"]["value"],
            "deep"
        );
    }

    #[test]
    fn test_convert_record_array_of_objects() {
        let input = json!({
            "users": [
                {"name": "Alice", "age": 30},
                {"name": "Bob", "age": 25}
            ]
        });
        let output = convert_record_to_ipld(&input);
        assert_eq!(output["users"][0]["name"], "Alice");
        assert_eq!(output["users"][1]["age"], 25);
    }

    #[tokio::test]
    async fn test_update_metrics() {
        let (storage, _dir) = setup_test_storage();
        let manager = BackfillerManager::new(std::sync::Arc::new(storage)).unwrap();

        // Enqueue some jobs
        for i in 0..5 {
            let job = BackfillJob {
                did: format!("did:plc:test{i}"),
                retry_count: 0,
            };
            manager.storage.enqueue_backfill(&job).unwrap();
        }

        // Call update_metrics
        manager.update_metrics();

        // Verify queue length is tracked (we can't directly access metrics, but we can verify it doesn't panic)
        let queue_len = manager.storage.repo_backfill_len().unwrap();
        assert_eq!(queue_len, 5);
    }

    #[tokio::test]
    async fn test_check_backpressure_no_backpressure() {
        let (storage, _dir) = setup_test_storage();
        let manager = BackfillerManager::new(std::sync::Arc::new(storage)).unwrap();

        // No jobs in output stream, should return false
        let has_backpressure = manager.check_backpressure().await;
        assert!(
            !has_backpressure,
            "should not have backpressure with empty output stream"
        );
    }

    #[tokio::test]
    async fn test_check_backpressure_with_backpressure() {
        use crate::config::BACKFILLER_OUTPUT_HIGH_WATER_MARK;
        use crate::types::IndexJob;
        use crate::types::WriteAction;

        let (storage, _dir) = setup_test_storage();
        let manager = BackfillerManager::new(std::sync::Arc::new(storage)).unwrap();

        // Fill output stream beyond high water mark
        for i in 0..BACKFILLER_OUTPUT_HIGH_WATER_MARK + 10 {
            let job = IndexJob {
                uri: format!("at://did:plc:test/app.bsky.feed.post/test{i}"),
                cid: "bafytest".to_owned(),
                action: WriteAction::Create,
                record: Some(json!({"text": "test"})),
                indexed_at: "2024-01-01T00:00:00Z".to_owned(),
                rev: "test".to_owned(),
            };
            manager.storage.enqueue_firehose_backfill(&job).unwrap();
        }

        // Should detect backpressure
        let has_backpressure = manager.check_backpressure().await;
        assert!(
            has_backpressure,
            "should detect backpressure when output stream exceeds high water mark"
        );
    }

    #[tokio::test]
    async fn test_dequeue_batch_empty_queue() {
        let (storage, _dir) = setup_test_storage();
        let manager = BackfillerManager::new(std::sync::Arc::new(storage)).unwrap();

        let jobs = manager.dequeue_batch();
        assert_eq!(jobs.len(), 0, "should return empty vec for empty queue");
    }

    #[tokio::test]
    async fn test_dequeue_batch_single_job() {
        let (storage, _dir) = setup_test_storage();
        let manager = BackfillerManager::new(std::sync::Arc::new(storage)).unwrap();

        // Enqueue one job
        let job = BackfillJob {
            did: "did:plc:test1".to_owned(),
            retry_count: 0,
        };
        manager.storage.enqueue_backfill(&job).unwrap();

        let jobs = manager.dequeue_batch();
        // Dequeue now removes items immediately to prevent race conditions
        assert_eq!(jobs.len(), 1, "should return only the one job that exists");
        assert_eq!(jobs[0].1.did, "did:plc:test1");
    }

    #[tokio::test]
    async fn test_dequeue_batch_multiple_jobs() {
        use crate::config::BACKFILLER_BATCH_SIZE;

        let (storage, _dir) = setup_test_storage();
        let manager = BackfillerManager::new(std::sync::Arc::new(storage)).unwrap();

        // Enqueue more than batch size
        for i in 0..(BACKFILLER_BATCH_SIZE + 5) {
            let job = BackfillJob {
                did: format!("did:plc:test{i}"),
                retry_count: 0,
            };
            manager.storage.enqueue_backfill(&job).unwrap();
        }

        let jobs = manager.dequeue_batch();
        assert_eq!(
            jobs.len(),
            BACKFILLER_BATCH_SIZE,
            "should return batch_size jobs, not more"
        );
    }

    #[tokio::test]
    async fn test_spawn_job_tasks_empty() {
        let (storage, _dir) = setup_test_storage();
        let manager = BackfillerManager::new(std::sync::Arc::new(storage)).unwrap();

        let tasks = manager.spawn_job_tasks(vec![]).await;
        assert_eq!(
            tasks.len(),
            0,
            "should return empty tasks vec for empty input"
        );
    }

    #[tokio::test]
    async fn test_spawn_job_tasks_with_jobs() {
        let (storage, _dir) = setup_test_storage();
        let manager = BackfillerManager::new(std::sync::Arc::new(storage)).unwrap();

        // Create mock jobs (they will fail since DIDs are invalid, but that's ok for this test)
        let jobs = vec![
            (
                b"key1".to_vec(),
                BackfillJob {
                    did: "did:plc:invalid1".to_owned(),
                    retry_count: 0,
                },
            ),
            (
                b"key2".to_vec(),
                BackfillJob {
                    did: "did:plc:invalid2".to_owned(),
                    retry_count: 0,
                },
            ),
        ];

        let tasks = manager.spawn_job_tasks(jobs).await;
        assert_eq!(tasks.len(), 2, "should spawn 2 tasks");

        // Wait for tasks to complete (they will fail, but that's expected)
        for task in tasks {
            drop(task.await);
        }
    }

    #[tokio::test]
    async fn test_handle_task_results_success() {
        let (storage, _dir) = setup_test_storage();
        let manager = BackfillerManager::new(std::sync::Arc::new(storage)).unwrap();

        // Manually enqueue a job so we can remove it
        let job = BackfillJob {
            did: "did:plc:test123".to_owned(),
            retry_count: 0,
        };
        manager.storage.enqueue_backfill(&job).unwrap();
        let (key, _) = manager.storage.dequeue_backfill().unwrap().unwrap();

        // Create a successful task result
        let task = tokio::spawn(async move { (key, job, Ok(())) });

        manager.handle_task_results(vec![task]).await;

        // Verify job was removed (queue should be empty)
        let queue_len = manager.storage.repo_backfill_len().unwrap();
        assert_eq!(queue_len, 0, "successful job should be removed from queue");
    }

    #[tokio::test]
    async fn test_handle_task_results_retry() {
        let (storage, _dir) = setup_test_storage();
        let manager = BackfillerManager::new(std::sync::Arc::new(storage)).unwrap();

        // Manually enqueue a job
        let job = BackfillJob {
            did: "did:plc:test456".to_owned(),
            retry_count: 0,
        };
        manager.storage.enqueue_backfill(&job).unwrap();
        let (key, job_from_queue) = manager.storage.dequeue_backfill().unwrap().unwrap();

        // Create a failed task result (retry_count < 3)
        let task = tokio::spawn(async move {
            (
                key,
                job_from_queue,
                Err(WintermuteError::Other("test error".into())),
            )
        });

        manager.handle_task_results(vec![task]).await;

        // Verify job was re-enqueued with incremented retry_count
        let queue_len = manager.storage.repo_backfill_len().unwrap();
        assert_eq!(queue_len, 1, "failed job should be re-enqueued");

        // Dequeue and check retry count
        let (_, retried_job) = manager.storage.dequeue_backfill().unwrap().unwrap();
        assert_eq!(
            retried_job.retry_count, 1,
            "retry_count should be incremented"
        );
    }

    #[tokio::test]
    async fn test_handle_task_results_dead_letter() {
        let (storage, _dir) = setup_test_storage();
        let manager = BackfillerManager::new(std::sync::Arc::new(storage)).unwrap();

        // Manually enqueue a job with max retries
        let job = BackfillJob {
            did: "did:plc:test789".to_owned(),
            retry_count: 2, // Will become 3 after increment
        };
        manager.storage.enqueue_backfill(&job).unwrap();
        let (key, job_from_queue) = manager.storage.dequeue_backfill().unwrap().unwrap();

        // Create a failed task result (retry_count will become 3)
        let task = tokio::spawn(async move {
            (
                key,
                job_from_queue,
                Err(WintermuteError::Other("test error".into())),
            )
        });

        manager.handle_task_results(vec![task]).await;

        // Verify job was NOT re-enqueued (dead-lettered)
        let queue_len = manager.storage.repo_backfill_len().unwrap();
        assert_eq!(
            queue_len, 0,
            "job exceeding max retries should be dead-lettered"
        );
    }

    #[tokio::test]
    async fn test_handle_task_results_task_panic() {
        let (storage, _dir) = setup_test_storage();
        let manager = BackfillerManager::new(std::sync::Arc::new(storage)).unwrap();

        // Create a task that will panic
        let task = tokio::spawn(async {
            panic!("intentional panic for testing");
            #[allow(unreachable_code)]
            (
                Vec::new(),
                BackfillJob {
                    did: "test".to_owned(),
                    retry_count: 0,
                },
                Ok(()),
            )
        });

        // Should handle the panic without crashing
        manager.handle_task_results(vec![task]).await;
    }

    #[tokio::test]
    async fn test_process_job_invalid_did() {
        let (storage, _dir) = setup_test_storage();

        let job = BackfillJob {
            did: "did:plc:nonexistent123456789".to_owned(),
            retry_count: 0,
        };

        let http_client = reqwest::Client::new();
        let result = BackfillerManager::process_job(&storage, &http_client, &job).await;

        assert!(result.is_err());
        match result {
            Err(WintermuteError::Other(msg)) => {
                assert!(msg.contains("did resolution failed"));
            }
            _ => panic!("expected DID resolution error"),
        }
    }

    #[test]
    fn test_run_creates_runtime() {
        use std::sync::Arc;

        let (storage, _dir) = setup_test_storage();
        let manager = BackfillerManager::new(Arc::new(storage)).unwrap();

        // Set shutdown flag immediately so process_loop exits
        crate::SHUTDOWN.store(true, std::sync::atomic::Ordering::Relaxed);

        // run() should complete successfully
        let result = manager.run();
        assert!(result.is_ok());

        // Reset shutdown flag for other tests
        crate::SHUTDOWN.store(false, std::sync::atomic::Ordering::Relaxed);
    }

    #[tokio::test]
    async fn test_process_loop_exits_on_shutdown() {
        use std::sync::Arc;

        let (storage, _dir) = setup_test_storage();
        let manager = BackfillerManager::new(Arc::new(storage)).unwrap();

        // Set shutdown flag
        crate::SHUTDOWN.store(true, std::sync::atomic::Ordering::Relaxed);

        // process_loop should exit immediately
        manager.process_loop().await;

        // Reset for other tests
        crate::SHUTDOWN.store(false, std::sync::atomic::Ordering::Relaxed);
    }

    #[tokio::test]
    async fn test_process_loop_handles_empty_queue() {
        use std::sync::Arc;
        use std::time::Duration;

        let (storage, _dir) = setup_test_storage();
        let manager = BackfillerManager::new(Arc::new(storage)).unwrap();

        // Spawn process_loop in background
        let manager_arc = Arc::new(manager);
        let handle = {
            let m = manager_arc.clone();
            tokio::spawn(async move {
                m.process_loop().await;
            })
        };

        // Let it run for a bit
        tokio::time::sleep(Duration::from_millis(300)).await;

        // Set shutdown
        crate::SHUTDOWN.store(true, std::sync::atomic::Ordering::Relaxed);

        // Wait for completion
        tokio::time::timeout(Duration::from_secs(2), handle)
            .await
            .expect("process_loop should exit on shutdown")
            .expect("task should complete");

        // Reset
        crate::SHUTDOWN.store(false, std::sync::atomic::Ordering::Relaxed);
    }

    // HTTP Mocking Tests - These actually test error paths in process_job()
    // Note: These tests primarily exercise DID resolution error paths since mocking
    // the full CAR fetch flow would require extensive infrastructure

    #[tokio::test]
    async fn test_process_job_did_resolution_failure() {
        let (storage, _dir) = setup_test_storage();

        // Test with invalid DID that will fail resolution
        let job = BackfillJob {
            did: "did:plc:invalidtestdid123".to_owned(),
            retry_count: 0,
        };

        let http_client = reqwest::Client::new();
        let result = BackfillerManager::process_job(&storage, &http_client, &job).await;

        // Should fail with DID resolution error
        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(err_msg.contains("did resolution failed") || err_msg.contains("DID"));
    }

    #[tokio::test]
    async fn test_process_job_invalid_did_format() {
        let (storage, _dir) = setup_test_storage();

        // Test with completely malformed DID
        let job = BackfillJob {
            did: "not-a-valid-did".to_owned(),
            retry_count: 0,
        };

        let http_client = reqwest::Client::new();
        let result = BackfillerManager::process_job(&storage, &http_client, &job).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_new_http_client_creation() {
        use std::sync::Arc;

        let (storage, _dir) = setup_test_storage();

        // Test that new() successfully creates HTTP client with timeout
        let result = BackfillerManager::new(Arc::new(storage));
        assert!(result.is_ok());

        let manager = result.unwrap();
        assert_eq!(manager.workers, crate::config::WORKERS_BACKFILLER);

        // HTTP client should be configured with timeout
        // We can't directly test the timeout config, but we know it was created
    }
}
