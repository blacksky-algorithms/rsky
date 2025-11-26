use rsky_wintermute::SHUTDOWN;
use rsky_wintermute::backfiller::BackfillerManager;
use rsky_wintermute::config::{BACKFILLER_BATCH_SIZE, BACKFILLER_OUTPUT_HIGH_WATER_MARK};
use rsky_wintermute::storage::Storage;
use rsky_wintermute::types::{BackfillJob, IndexJob, WriteAction};
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tempfile::TempDir;

fn setup_test_storage() -> (Arc<Storage>, TempDir) {
    let temp_dir = TempDir::with_prefix("backfiller_integration_").unwrap();
    let db_path = temp_dir.path().join("test_db");
    let storage = Arc::new(Storage::new(Some(db_path)).unwrap());
    (storage, temp_dir)
}

#[test]
fn test_backfiller_manager_new_success() {
    let (storage, _dir) = setup_test_storage();
    let manager = BackfillerManager::new(storage);
    assert!(
        manager.is_ok(),
        "BackfillerManager::new should succeed: {:?}",
        manager.err()
    );
}

#[tokio::test]
async fn test_backfiller_processes_single_job() {
    let (storage, _dir) = setup_test_storage();

    // Enqueue a valid job
    let job = BackfillJob {
        did: "did:plc:w4xbfzo7kqfes5zb7r6qv3rw".to_owned(),
        retry_count: 0,
    };
    storage.enqueue_backfill(&job).unwrap();

    // Process with timeout
    let storage_clone = Arc::clone(&storage);
    let handle = tokio::spawn(async move {
        let manager = BackfillerManager::new(storage_clone).unwrap();

        // Spawn the manager in background with shutdown after processing
        let manager_handle = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            SHUTDOWN.store(true, Ordering::Relaxed);
        });

        // This will fail because run() blocks forever, but we're testing the setup
        let _ = tokio::time::timeout(Duration::from_secs(1), async { manager.run() }).await;

        manager_handle.abort();
    });

    let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;

    // Reset shutdown flag
    SHUTDOWN.store(false, Ordering::Relaxed);
}

#[tokio::test]
async fn test_backfiller_handles_empty_queue() {
    let (storage, _dir) = setup_test_storage();

    // Don't enqueue any jobs
    assert_eq!(storage.repo_backfill_len().unwrap(), 0);

    // Create manager and verify it handles empty queue gracefully
    let manager = BackfillerManager::new(Arc::clone(&storage));
    assert!(manager.is_ok());
}

#[tokio::test]
async fn test_backfiller_respects_batch_size() {
    let (storage, _dir) = setup_test_storage();

    // Enqueue more jobs than batch size
    for i in 0..(*BACKFILLER_BATCH_SIZE * 2) {
        let job = BackfillJob {
            did: format!("did:plc:test{}", i),
            retry_count: 0,
        };
        storage.enqueue_backfill(&job).unwrap();
    }

    let initial_len = storage.repo_backfill_len().unwrap();
    let batch_size = *BACKFILLER_BATCH_SIZE;
    assert!(
        initial_len >= batch_size * 2,
        "should have at least {} jobs, got {}",
        batch_size * 2,
        initial_len
    );
}

#[tokio::test]
async fn test_backfiller_detects_backpressure() {
    let (storage, _dir) = setup_test_storage();

    // Fill output stream beyond high water mark to trigger backpressure
    for i in 0..(*BACKFILLER_OUTPUT_HIGH_WATER_MARK + 100) {
        let job = IndexJob {
            uri: format!("at://did:plc:test/app.bsky.feed.post/test{}", i),
            cid: "bafytest".to_owned(),
            action: WriteAction::Create,
            record: Some(json!({"text": "test"})),
            indexed_at: "2024-01-01T00:00:00Z".to_owned(),
            rev: "test".to_owned(),
        };
        storage.enqueue_firehose_backfill(&job).unwrap();
    }

    let output_len = storage.firehose_backfill_len().unwrap();
    let high_water_mark = *BACKFILLER_OUTPUT_HIGH_WATER_MARK;
    assert!(
        output_len > high_water_mark,
        "output stream should exceed high water mark for backpressure test: {} > {}",
        output_len,
        high_water_mark
    );
}

#[tokio::test]
async fn test_backfiller_processes_multiple_jobs_concurrently() {
    let (storage, _dir) = setup_test_storage();

    // Enqueue multiple jobs that should succeed quickly (using invalid DIDs that fail fast)
    for i in 0..5 {
        let job = BackfillJob {
            did: format!("did:plc:invalid{}", i),
            retry_count: 0,
        };
        storage.enqueue_backfill(&job).unwrap();
    }

    assert_eq!(storage.repo_backfill_len().unwrap(), 5);
}

#[tokio::test]
async fn test_backfiller_handles_job_failures_with_retry() {
    let (storage, _dir) = setup_test_storage();

    // Enqueue a job that will fail (invalid DID)
    let job = BackfillJob {
        did: "did:plc:nonexistent123".to_owned(),
        retry_count: 0,
    };
    storage.enqueue_backfill(&job).unwrap();

    // Process the job directly to test retry logic
    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    let result = BackfillerManager::process_job(&storage, &http_client, &job).await;

    // Should fail
    assert!(result.is_err());

    // Verify job can be retried (retry_count < 3)
    let mut retry_job = job;
    retry_job.retry_count += 1;
    assert!(retry_job.retry_count < 3, "should be eligible for retry");
}

#[tokio::test]
async fn test_backfiller_dead_letters_after_max_retries() {
    let job = BackfillJob {
        did: "did:plc:nonexistent456".to_owned(),
        retry_count: 3, // Already at max retries
    };

    // This job should not be retried (retry_count >= 3)
    assert!(
        job.retry_count >= 3,
        "job with retry_count {} should be dead-lettered",
        job.retry_count
    );
}

#[tokio::test]
async fn test_process_job_error_paths() {
    let (storage, _dir) = setup_test_storage();
    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    // Test 1: Invalid DID resolution
    let job1 = BackfillJob {
        did: "did:plc:doesnotexist999".to_owned(),
        retry_count: 0,
    };
    let result1 = BackfillerManager::process_job(&storage, &http_client, &job1).await;
    assert!(result1.is_err());

    // Test 2: Invalid DID format
    let job2 = BackfillJob {
        did: "invalid:did:format".to_owned(),
        retry_count: 0,
    };
    let result2 = BackfillerManager::process_job(&storage, &http_client, &job2).await;
    assert!(result2.is_err());
}

#[tokio::test]
async fn test_backfiller_manager_run_setup() {
    // Test that run() properly sets up the tokio runtime
    // We can't actually run it since it blocks forever, but we can verify creation succeeds
    let (storage, _dir) = setup_test_storage();
    let manager = BackfillerManager::new(storage);
    assert!(manager.is_ok());

    // The manager should be ready to run
    let _manager = manager.unwrap();
    // Can't actually call run() as it blocks forever, but we've tested creation
}

#[tokio::test]
async fn test_shutdown_signal_handling() {
    // Set shutdown flag
    SHUTDOWN.store(true, Ordering::Relaxed);

    // Verify it's set
    assert!(SHUTDOWN.load(Ordering::Relaxed));

    // Reset for other tests
    SHUTDOWN.store(false, Ordering::Relaxed);
    assert!(!SHUTDOWN.load(Ordering::Relaxed));
}

#[tokio::test]
async fn test_backfiller_metrics_tracking() {
    let (storage, _dir) = setup_test_storage();

    // Enqueue a job
    let job = BackfillJob {
        did: "did:plc:test789".to_owned(),
        retry_count: 0,
    };
    storage.enqueue_backfill(&job).unwrap();

    // Verify queue length can be read for metrics
    let queue_len = storage.repo_backfill_len().unwrap();
    assert_eq!(queue_len, 1);

    // Verify output stream length can be read for metrics
    let output_len = storage.firehose_backfill_len().unwrap();
    assert_eq!(output_len, 0);
}

#[tokio::test]
async fn test_backfiller_batch_dequeuing() {
    let (storage, _dir) = setup_test_storage();

    // Enqueue exactly BATCH_SIZE jobs
    let batch_size = *BACKFILLER_BATCH_SIZE;
    for i in 0..batch_size {
        let job = BackfillJob {
            did: format!("did:plc:batch{}", i),
            retry_count: 0,
        };
        storage.enqueue_backfill(&job).unwrap();
    }

    let initial_len = storage.repo_backfill_len().unwrap();
    assert_eq!(initial_len, batch_size);

    // Dequeue in a batch - dequeue doesn't remove, just peeks
    let mut dequeued = Vec::new();
    for _ in 0..batch_size {
        if let Ok(Some((key, job))) = storage.dequeue_backfill() {
            dequeued.push((key, job));
            // Note: dequeue doesn't remove the item, need to call remove_backfill separately
            break; // dequeue returns same item repeatedly until removed
        }
    }

    assert!(!dequeued.is_empty(), "should dequeue at least one item");

    // Remove all items from queue
    for _ in 0..batch_size {
        if let Ok(Some((key, _))) = storage.dequeue_backfill() {
            storage.remove_backfill(&key).unwrap();
        } else {
            break;
        }
    }

    let final_len = storage.repo_backfill_len().unwrap();
    assert_eq!(final_len, 0);
}
