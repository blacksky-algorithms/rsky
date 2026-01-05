//! Indexer integration tests
//!
//! These tests verify end-to-end indexing from backfill through postgres writes.
//! Each test uses an isolated fjall database via `TempDir` and shares a `PostgreSQL` database.
//!
//! Run with: `DATABASE_URL=... cargo test --lib indexer::tests`

#[cfg(test)]
mod indexer_tests {
    use crate::backfiller::BackfillerManager;
    use crate::indexer::IndexerManager;
    use crate::storage::Storage;
    use crate::types::{BackfillJob, LabelEvent, WriteAction};
    use deadpool_postgres::{Config, ManagerConfig, Pool, RecyclingMethod, Runtime};
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio_postgres::NoTls;

    fn setup_test_storage() -> (Storage, TempDir) {
        let temp_dir = TempDir::with_prefix("indexer_test_").unwrap();
        let db_path = temp_dir.path().join("test_db");
        let storage = Storage::new(Some(db_path)).unwrap();
        (storage, temp_dir)
    }

    fn setup_test_pool() -> Pool {
        let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgresql://postgres:postgres@localhost:5432/bsky_test".to_owned()
        });

        let mut pg_config = Config::new();
        pg_config.url = Some(database_url);
        pg_config.manager = Some(ManagerConfig {
            recycling_method: RecyclingMethod::Fast,
        });

        pg_config.create_pool(Some(Runtime::Tokio1), NoTls).unwrap()
    }

    async fn cleanup_test_data(pool: &Pool, did: &str) {
        let client = pool.get().await.unwrap();

        let tables = [
            "post",
            "like",
            "follow",
            "repost",
            "block",
            "profile",
            "feed_generator",
            "list",
            "list_item",
            "list_block",
            "starter_pack",
            "labeler",
            "thread_gate",
            "post_gate",
            "chat_declaration",
            "notif_declaration",
            "status",
            "verification",
            "notification",
        ];

        for table in &tables {
            let query =
                format!("DELETE FROM {table} WHERE creator = $1 OR did = $1 OR author = $1");
            drop(client.execute(&query, &[&did]).await);
        }

        drop(
            client
                .execute("DELETE FROM record WHERE did = $1", &[&did])
                .await,
        );
        drop(
            client
                .execute("DELETE FROM profile_agg WHERE did = $1", &[&did])
                .await,
        );
        drop(
            client
                .execute(
                    "DELETE FROM post_agg WHERE uri IN (SELECT uri FROM post WHERE creator = $1)",
                    &[&did],
                )
                .await,
        );
    }

    #[test]
    fn test_write_action_serialization() {
        let create = WriteAction::Create;
        let json = serde_json::to_string(&create).unwrap();
        assert!(json.contains("Create"));

        let update = WriteAction::Update;
        let json = serde_json::to_string(&update).unwrap();
        assert!(json.contains("Update"));

        let delete = WriteAction::Delete;
        let json = serde_json::to_string(&delete).unwrap();
        assert!(json.contains("Delete"));
    }

    #[tokio::test]
    async fn test_index_job_processing() {
        let (storage, _dir) = setup_test_storage();
        let pool = setup_test_pool();
        let test_did = "did:plc:w4xbfzo7kqfes5zb7r6qv3rw";

        cleanup_test_data(&pool, test_did).await;

        let job = BackfillJob {
            did: test_did.to_owned(),
            retry_count: 0,
        };

        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .unwrap();

        tracing::info!("processing backfill job for {test_did}");
        let result = BackfillerManager::process_job(&storage, &http_client, &job).await;

        assert!(result.is_ok(), "backfill job failed: {:?}", result.err());

        let queue_len = storage.firehose_backfill_len().unwrap();
        tracing::info!("backfill complete, {queue_len} records enqueued for indexing");
        assert!(
            queue_len > 5000,
            "expected more than 5000 records to be enqueued, found {queue_len}"
        );

        let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgresql://postgres:postgres@localhost:5432/bsky_test".to_owned()
        });
        let indexer = IndexerManager::new(Arc::new(storage), &database_url).unwrap();

        let mut processed = 0;
        let batch_size = 100;
        let mut consecutive_empty = 0;

        while consecutive_empty < 3 {
            let mut batch_processed = 0;

            for _ in 0..batch_size {
                match indexer.storage.dequeue_firehose_backfill() {
                    Ok(Some((key, index_job))) => {
                        let result =
                            IndexerManager::process_job(&indexer.pool_backfill, &index_job).await;

                        match result {
                            Ok(()) => {
                                drop(indexer.storage.remove_firehose_backfill(&key));
                                batch_processed += 1;
                            }
                            Err(e) => {
                                tracing::error!("index job failed for {}: {e:?}", index_job.uri);
                            }
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        tracing::error!("dequeue failed: {e}");
                        break;
                    }
                }
            }

            processed += batch_processed;

            if batch_processed == 0 {
                consecutive_empty += 1;
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            } else {
                consecutive_empty = 0;
            }

            if processed > 0 && processed % 1000 == 0 {
                tracing::info!("processed {processed} index jobs");
            }
        }

        tracing::info!("indexing complete, {processed} records indexed");

        #[allow(clippy::cast_precision_loss)]
        let success_rate = (f64::from(processed) / queue_len as f64) * 100.0;
        tracing::info!("indexing success rate: {success_rate:.2}%");

        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss
        )]
        let min_expected = (queue_len as f64 * 0.99) as usize;

        assert!(
            success_rate >= 99.0,
            "expected at least 99% of records to be indexed ({min_expected} records), but only {success_rate:.2}% were successful ({processed} / {queue_len})"
        );

        let client = pool.get().await.unwrap();

        let record_count: i64 = client
            .query_one("SELECT COUNT(*) FROM record WHERE did = $1", &[&test_did])
            .await
            .unwrap()
            .get(0);
        tracing::info!("records in generic table: {record_count}");

        let notification_count: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM notification WHERE author = $1",
                &[&test_did],
            )
            .await
            .unwrap()
            .get(0);
        tracing::info!("notifications created: {notification_count}");

        assert_eq!(
            record_count,
            i64::try_from(queue_len).expect("queue_len should fit in i64"),
            "expected all {queue_len} records in generic record table, found {record_count}"
        );

        let post_count: i64 = client
            .query_one("SELECT COUNT(*) FROM post WHERE creator = $1", &[&test_did])
            .await
            .unwrap()
            .get(0);
        tracing::info!("posts indexed: {post_count}");
        assert!(post_count > 0, "expected posts to be indexed");

        let like_count: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM \"like\" WHERE creator = $1",
                &[&test_did],
            )
            .await
            .unwrap()
            .get(0);
        tracing::info!("likes indexed: {like_count}");

        let follow_count: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM follow WHERE creator = $1",
                &[&test_did],
            )
            .await
            .unwrap()
            .get(0);
        tracing::info!("follows indexed: {follow_count}");

        let repost_count: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM repost WHERE creator = $1",
                &[&test_did],
            )
            .await
            .unwrap()
            .get(0);
        tracing::info!("reposts indexed: {repost_count}");

        let block_count: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM actor_block WHERE creator = $1",
                &[&test_did],
            )
            .await
            .unwrap()
            .get(0);
        tracing::info!("blocks indexed: {block_count}");

        let list_count: i64 = client
            .query_one("SELECT COUNT(*) FROM list WHERE creator = $1", &[&test_did])
            .await
            .unwrap()
            .get(0);
        tracing::info!("lists indexed: {list_count}");

        let list_item_count: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM list_item WHERE creator = $1",
                &[&test_did],
            )
            .await
            .unwrap()
            .get(0);
        tracing::info!("list items indexed: {list_item_count}");

        let feed_gen_count: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM feed_generator WHERE creator = $1",
                &[&test_did],
            )
            .await
            .unwrap()
            .get(0);
        tracing::info!("feed generators indexed: {feed_gen_count}");

        let profile_count: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM profile WHERE creator = $1",
                &[&test_did],
            )
            .await
            .unwrap()
            .get(0);
        tracing::info!("profiles indexed: {profile_count}");

        assert_eq!(profile_count, 1, "expected exactly 1 profile record");

        let total_records = post_count + like_count + follow_count + repost_count + profile_count;
        assert!(
            total_records > 5000,
            "expected total records > 5000 in core tables (post, like, follow, repost, profile), found {total_records}. Note: other records go into list_item, list_block, starter_pack, etc."
        );

        let profile_agg: Option<(i64, i64, i64)> = client
            .query_opt(
                "SELECT \"followersCount\", \"followsCount\", \"postsCount\" FROM profile_agg WHERE did = $1",
                &[&test_did],
            )
            .await
            .unwrap()
            .map(|row| (row.get(0), row.get(1), row.get(2)));

        if let Some((followers, follows, posts)) = profile_agg {
            tracing::info!(
                "profile_agg for {test_did}: followers={followers}, follows={follows}, posts={posts}"
            );
            assert_eq!(
                posts, post_count,
                "profile_agg postsCount should match post count"
            );
        } else {
            tracing::warn!("profile_agg not found for {test_did}");
        }

        let post_agg_count: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM post_agg WHERE uri LIKE $1",
                &[&format!("at://{test_did}/%")],
            )
            .await
            .unwrap()
            .get(0);
        tracing::info!("post_agg entries for {test_did}: {post_agg_count}");

        cleanup_test_data(&pool, test_did).await;
    }

    #[tokio::test]
    async fn test_notification_creation() {
        let (storage, _dir) = setup_test_storage();
        let pool = setup_test_pool();
        // Use a different DID to avoid interfering with test_index_job_processing
        let test_did = "did:plc:ewvi7nxzyoun6zhxrhs64oiz";

        cleanup_test_data(&pool, test_did).await;

        let job = BackfillJob {
            did: test_did.to_owned(),
            retry_count: 0,
        };

        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .unwrap();

        let result = BackfillerManager::process_job(&storage, &http_client, &job).await;
        assert!(result.is_ok());

        let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgresql://postgres:postgres@localhost:5432/bsky_test".to_owned()
        });
        let indexer = IndexerManager::new(Arc::new(storage), &database_url).unwrap();

        let batch_size = 100;

        loop {
            let mut batch_processed = 0;

            for _ in 0..batch_size {
                match indexer.storage.dequeue_firehose_backfill() {
                    Ok(Some((key, index_job))) => {
                        let result =
                            IndexerManager::process_job(&indexer.pool_backfill, &index_job).await;

                        if result.is_ok() {
                            drop(indexer.storage.remove_firehose_backfill(&key));
                            batch_processed += 1;
                        }
                    }
                    Ok(None) | Err(_) => break,
                }
            }

            if batch_processed == 0 {
                break;
            }
        }

        let client = pool.get().await.unwrap();

        let notification_count: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM notification WHERE author = $1",
                &[&test_did],
            )
            .await
            .unwrap()
            .get(0);

        tracing::info!("notifications created: {notification_count}");

        cleanup_test_data(&pool, test_did).await;
    }

    #[tokio::test]
    async fn test_valid_uri_processing() {
        let pool = setup_test_pool();

        let valid_job = crate::types::IndexJob {
            uri: "at://did:plc:test/app.bsky.feed.post/valid123".to_owned(),
            cid: "bafytest".to_owned(),
            action: WriteAction::Create,
            record: Some(serde_json::json!({"text": "test", "createdAt": "2024-01-01T00:00:00Z"})),
            indexed_at: chrono::Utc::now().to_rfc3339(),
            rev: "test".to_owned(),
        };

        let result = IndexerManager::process_job(&pool, &valid_job).await;
        assert!(
            result.is_ok(),
            "expected valid URI to succeed: {:?}",
            result.err()
        );

        let client = pool.get().await.unwrap();
        let count: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM post WHERE creator = 'did:plc:test'",
                &[],
            )
            .await
            .unwrap()
            .get(0);
        assert_eq!(count, 1, "expected post to be inserted");

        drop(
            client
                .execute("DELETE FROM post WHERE creator = 'did:plc:test'", &[])
                .await,
        );
        drop(
            client
                .execute("DELETE FROM record WHERE did = 'did:plc:test'", &[])
                .await,
        );
    }

    // =============================================================================
    // LABELS INDEXING TESTS
    // =============================================================================

    async fn cleanup_test_labels(pool: &Pool, src: &str) {
        let client = pool.get().await.unwrap();
        drop(
            client
                .execute("DELETE FROM label WHERE src = $1", &[&src])
                .await,
        );
    }

    #[tokio::test]
    async fn test_label_indexing_single_label() {
        let pool = setup_test_pool();
        let test_src = "did:plc:test_labeler_single";
        let test_uri = "at://did:plc:user123/app.bsky.feed.post/abc123";

        cleanup_test_labels(&pool, test_src).await;

        let label_event = crate::types::LabelEvent {
            seq: 1000,
            labels: vec![crate::types::Label {
                src: test_src.to_owned(),
                uri: test_uri.to_owned(),
                val: "spam".to_owned(),
                cts: "2025-01-20T10:00:00Z".to_owned(),
            }],
        };

        let result = IndexerManager::process_label_event(&pool, &label_event).await;
        assert!(result.is_ok(), "label indexing should succeed: {result:?}");

        let client = pool.get().await.unwrap();
        let count: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM label WHERE src = $1 AND uri = $2 AND cid = '' AND val = $3",
                &[&test_src, &test_uri, &"spam"],
            )
            .await
            .unwrap()
            .get(0);
        assert_eq!(count, 1, "expected label to be inserted");

        // Verify the label data
        let row = client
            .query_one(
                "SELECT src, uri, val, cts FROM label WHERE src = $1 AND cid = ''",
                &[&test_src],
            )
            .await
            .unwrap();

        let src: String = row.get(0);
        let uri: String = row.get(1);
        let val: String = row.get(2);
        let cts: String = row.get(3);

        assert_eq!(src, test_src);
        assert_eq!(uri, test_uri);
        assert_eq!(val, "spam");
        assert_eq!(cts, "2025-01-20T10:00:00Z");

        cleanup_test_labels(&pool, test_src).await;
    }

    #[tokio::test]
    async fn test_label_indexing_multiple_labels() {
        let pool = setup_test_pool();
        let test_src = "did:plc:test_labeler_multi";

        cleanup_test_labels(&pool, test_src).await;

        let label_event = crate::types::LabelEvent {
            seq: 2000,
            labels: vec![
                crate::types::Label {
                    src: test_src.to_owned(),
                    uri: "at://did:plc:user1/app.bsky.feed.post/post1".to_owned(),
                    val: "spam".to_owned(),
                    cts: "2025-01-20T10:00:00Z".to_owned(),
                },
                crate::types::Label {
                    src: test_src.to_owned(),
                    uri: "at://did:plc:user2/app.bsky.feed.post/post2".to_owned(),
                    val: "nsfw".to_owned(),
                    cts: "2025-01-20T10:01:00Z".to_owned(),
                },
                crate::types::Label {
                    src: test_src.to_owned(),
                    uri: "at://did:plc:user3/app.bsky.feed.post/post3".to_owned(),
                    val: "porn".to_owned(),
                    cts: "2025-01-20T10:02:00Z".to_owned(),
                },
            ],
        };

        let result = IndexerManager::process_label_event(&pool, &label_event).await;
        assert!(result.is_ok());

        let client = pool.get().await.unwrap();
        let count: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM label WHERE src = $1 AND cid = ''",
                &[&test_src],
            )
            .await
            .unwrap()
            .get(0);
        assert_eq!(count, 3, "expected all 3 labels to be inserted");

        // Verify each label
        let spam_count: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM label WHERE src = $1 AND cid = '' AND val = 'spam'",
                &[&test_src],
            )
            .await
            .unwrap()
            .get(0);
        assert_eq!(spam_count, 1);

        let nsfw_count: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM label WHERE src = $1 AND cid = '' AND val = 'nsfw'",
                &[&test_src],
            )
            .await
            .unwrap()
            .get(0);
        assert_eq!(nsfw_count, 1);

        let porn_count: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM label WHERE src = $1 AND cid = '' AND val = 'porn'",
                &[&test_src],
            )
            .await
            .unwrap()
            .get(0);
        assert_eq!(porn_count, 1);

        cleanup_test_labels(&pool, test_src).await;
    }

    #[tokio::test]
    async fn test_label_indexing_upsert_behavior() {
        let pool = setup_test_pool();
        let test_src = "did:plc:test_labeler_upsert";
        let test_uri = "at://did:plc:user/app.bsky.feed.post/test";

        cleanup_test_labels(&pool, test_src).await;

        // First insert
        let label_event1 = crate::types::LabelEvent {
            seq: 3000,
            labels: vec![crate::types::Label {
                src: test_src.to_owned(),
                uri: test_uri.to_owned(),
                val: "spam".to_owned(),
                cts: "2025-01-20T10:00:00Z".to_owned(),
            }],
        };

        let result = IndexerManager::process_label_event(&pool, &label_event1).await;
        assert!(result.is_ok());

        let client = pool.get().await.unwrap();
        let count: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM label WHERE src = $1 AND cid = ''",
                &[&test_src],
            )
            .await
            .unwrap()
            .get(0);
        assert_eq!(count, 1);

        // Second insert (same label, updated timestamp) - should upsert
        let label_event2 = crate::types::LabelEvent {
            seq: 3001,
            labels: vec![crate::types::Label {
                src: test_src.to_owned(),
                uri: test_uri.to_owned(),
                val: "spam".to_owned(),
                cts: "2025-01-20T11:00:00Z".to_owned(), // Different timestamp
            }],
        };

        let result = IndexerManager::process_label_event(&pool, &label_event2).await;
        assert!(result.is_ok());

        // Should still be 1 row (upserted, not inserted)
        let count: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM label WHERE src = $1 AND cid = ''",
                &[&test_src],
            )
            .await
            .unwrap()
            .get(0);
        assert_eq!(count, 1, "expected label to be upserted, not duplicated");

        // Verify the timestamp was updated
        let cts: String = client
            .query_one(
                "SELECT cts FROM label WHERE src = $1 AND uri = $2 AND cid = '' AND val = $3",
                &[&test_src, &test_uri, &"spam"],
            )
            .await
            .unwrap()
            .get(0);
        assert_eq!(cts, "2025-01-20T11:00:00Z", "timestamp should be updated");

        cleanup_test_labels(&pool, test_src).await;
    }

    #[tokio::test]
    async fn test_label_indexing_multiple_labelers_same_uri() {
        let pool = setup_test_pool();
        let test_src1 = "did:plc:labeler1";
        let test_src2 = "did:plc:labeler2";
        let test_uri = "at://did:plc:user/app.bsky.feed.post/shared";

        cleanup_test_labels(&pool, test_src1).await;
        cleanup_test_labels(&pool, test_src2).await;

        // Label from first labeler
        let label_event1 = crate::types::LabelEvent {
            seq: 4000,
            labels: vec![crate::types::Label {
                src: test_src1.to_owned(),
                uri: test_uri.to_owned(),
                val: "spam".to_owned(),
                cts: "2025-01-20T10:00:00Z".to_owned(),
            }],
        };

        let result = IndexerManager::process_label_event(&pool, &label_event1).await;
        assert!(result.is_ok());

        // Label from second labeler (same URI, same val)
        let label_event2 = crate::types::LabelEvent {
            seq: 4001,
            labels: vec![crate::types::Label {
                src: test_src2.to_owned(),
                uri: test_uri.to_owned(),
                val: "spam".to_owned(),
                cts: "2025-01-20T10:01:00Z".to_owned(),
            }],
        };

        let result = IndexerManager::process_label_event(&pool, &label_event2).await;
        assert!(result.is_ok());

        let client = pool.get().await.unwrap();
        let count: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM label WHERE uri = $1 AND cid = ''",
                &[&test_uri],
            )
            .await
            .unwrap()
            .get(0);
        assert_eq!(
            count, 2,
            "expected 2 labels (different labelers can label same URI)"
        );

        cleanup_test_labels(&pool, test_src1).await;
        cleanup_test_labels(&pool, test_src2).await;
    }

    #[tokio::test]
    async fn test_label_indexing_different_vals_same_labeler_uri() {
        let pool = setup_test_pool();
        let test_src = "did:plc:labeler_multival";
        let test_uri = "at://did:plc:user/app.bsky.feed.post/multival";

        cleanup_test_labels(&pool, test_src).await;

        // First label: spam
        let label_event1 = crate::types::LabelEvent {
            seq: 5000,
            labels: vec![crate::types::Label {
                src: test_src.to_owned(),
                uri: test_uri.to_owned(),
                val: "spam".to_owned(),
                cts: "2025-01-20T10:00:00Z".to_owned(),
            }],
        };

        let result = IndexerManager::process_label_event(&pool, &label_event1).await;
        assert!(result.is_ok());

        // Second label: nsfw (different val)
        let label_event2 = crate::types::LabelEvent {
            seq: 5001,
            labels: vec![crate::types::Label {
                src: test_src.to_owned(),
                uri: test_uri.to_owned(),
                val: "nsfw".to_owned(),
                cts: "2025-01-20T10:01:00Z".to_owned(),
            }],
        };

        let result = IndexerManager::process_label_event(&pool, &label_event2).await;
        assert!(result.is_ok());

        let client = pool.get().await.unwrap();
        let count: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM label WHERE src = $1 AND uri = $2 AND cid = ''",
                &[&test_src, &test_uri],
            )
            .await
            .unwrap()
            .get(0);
        assert_eq!(
            count, 2,
            "expected 2 labels (same labeler can apply different vals to same URI)"
        );

        cleanup_test_labels(&pool, test_src).await;
    }

    #[tokio::test]
    async fn test_label_storage_and_indexing_roundtrip() {
        let (storage, _dir) = setup_test_storage();
        let pool = setup_test_pool();
        let test_src = "did:plc:labeler_roundtrip";

        cleanup_test_labels(&pool, test_src).await;

        // Create and enqueue label event
        let label_event = crate::types::LabelEvent {
            seq: 6000,
            labels: vec![crate::types::Label {
                src: test_src.to_owned(),
                uri: "at://did:plc:user/app.bsky.feed.post/roundtrip".to_owned(),
                val: "spam".to_owned(),
                cts: "2025-01-20T10:00:00Z".to_owned(),
            }],
        };

        // Enqueue
        storage.enqueue_label_live(&label_event).unwrap();
        assert!(storage.label_live_len().unwrap() > 0);

        // Dequeue
        let dequeued = storage.dequeue_label_live().unwrap();
        assert!(dequeued.is_some());

        let (key, retrieved_event) = dequeued.unwrap();
        assert_eq!(retrieved_event.seq, label_event.seq);

        // Index the label
        let result = IndexerManager::process_label_event(&pool, &retrieved_event).await;
        assert!(result.is_ok());

        // Remove from queue
        storage.remove_label_live(&key).unwrap();

        // Verify in database
        let client = pool.get().await.unwrap();
        let count: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM label WHERE src = $1 AND cid = ''",
                &[&test_src],
            )
            .await
            .unwrap()
            .get(0);
        assert_eq!(count, 1, "label should be in database");

        cleanup_test_labels(&pool, test_src).await;
    }

    #[tokio::test]
    async fn test_label_indexing_empty_labels_array() {
        let pool = setup_test_pool();

        // Label event with empty labels array - should succeed without error
        let label_event = crate::types::LabelEvent {
            seq: 7000,
            labels: vec![],
        };

        let result = IndexerManager::process_label_event(&pool, &label_event).await;
        assert!(
            result.is_ok(),
            "empty labels array should succeed without error"
        );
    }

    #[tokio::test]
    #[ignore] // Ignored by default, run with: cargo test -- --ignored test_live_label_stream
    async fn test_live_label_stream_integration() {
        use futures::stream::StreamExt;
        use tokio::time::{Duration, timeout};
        use tokio_tungstenite::connect_async;

        // Initialize tracing for test output
        drop(
            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::from_default_env()
                        .add_directive(tracing::Level::INFO.into()),
                )
                .with_test_writer()
                .try_init(),
        );

        let (storage, _dir) = setup_test_storage();
        let pool = setup_test_pool();
        let test_src = "did:plc:live_integration_test";

        cleanup_test_labels(&pool, test_src).await;

        // Connect to live atproto.africa label stream
        let url = "wss://atproto.africa/xrpc/com.atproto.label.subscribeLabels";
        tracing::info!("connecting to live label stream: {url}");

        let (ws_stream, _) = connect_async(url)
            .await
            .expect("failed to connect to atproto.africa");

        let (_, mut read) = ws_stream.split();

        // Collect labels for 5 seconds
        let mut label_events = Vec::new();
        let mut total_labels = 0;

        tracing::info!("listening to label stream for 5 seconds...");
        let mut message_count = 0;
        let collection_result = timeout(Duration::from_secs(5), async {
            while let Some(msg_result) = read.next().await {
                message_count += 1;
                match msg_result {
                    Ok(msg) => {
                        tracing::info!("received message #{}: {:?}", message_count, msg);
                        if let tokio_tungstenite::tungstenite::Message::Binary(data) = msg {
                            tracing::info!("binary message size: {} bytes", data.len());
                            match crate::ingester::labels::parse_label_message(&data) {
                                Ok(Some(label_event)) => {
                                    total_labels += label_event.labels.len();
                                    label_events.push(label_event);
                                    tracing::info!(
                                        "successfully parsed label event with {} labels (total so far: {})",
                                        label_events.last().unwrap().labels.len(),
                                        total_labels
                                    );
                                }
                                Ok(None) => {
                                    tracing::info!("binary message was not a label event (different message type)");
                                }
                                Err(e) => {
                                    tracing::error!("failed to parse label message: {}", e);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("websocket error: {}", e);
                        break;
                    }
                }
            }
        })
        .await;

        // Timeout is expected (we want to disconnect after 5 seconds)
        drop(collection_result);

        tracing::info!(
            "disconnected from stream. received {} messages total, {} label events with {} total labels",
            message_count,
            label_events.len(),
            total_labels
        );

        if total_labels == 0 {
            tracing::warn!("no labels received from stream - skipping rest of test");
            return;
        }

        // Enqueue all label events to storage
        for label_event in &label_events {
            storage
                .enqueue_label_live(label_event)
                .expect("failed to enqueue label event");
        }

        let queue_len = storage
            .label_live_len()
            .expect("failed to get queue length");
        assert_eq!(
            queue_len,
            label_events.len(),
            "queue should contain all label events"
        );

        tracing::info!("enqueued {} label events to storage", label_events.len());

        // Process all labels through the indexer
        let mut processed_count = 0;
        while let Ok(Some((key, label_event))) = storage.dequeue_label_live() {
            let result = IndexerManager::process_label_event(&pool, &label_event).await;
            assert!(result.is_ok(), "indexing should succeed");

            storage
                .remove_label_live(&key)
                .expect("failed to remove from queue");
            processed_count += 1;
        }

        assert_eq!(
            processed_count,
            label_events.len(),
            "should process all label events"
        );

        tracing::info!("processed {} label events through indexer", processed_count);

        // Count labels in database
        let client = pool.get().await.expect("failed to get db client");

        // We need to count labels from all sources since we don't control which labeler sent them
        let db_label_count: i64 = client
            .query_one("SELECT COUNT(*) FROM label WHERE cid = ''", &[])
            .await
            .expect("failed to query label count")
            .get(0);

        tracing::info!(
            "database contains {} labels after indexing (expected at least {})",
            db_label_count,
            total_labels
        );

        // The database should have at least as many labels as we received
        // (may have more if labels were already in the database)
        assert!(
            db_label_count >= i64::try_from(total_labels).unwrap(),
            "database should contain at least {total_labels} labels, found {db_label_count}"
        );

        // Clean up - remove test labels
        // Note: We can't reliably clean up all labels we inserted since we don't know
        // which labeler DIDs were in the stream, so we'll just clean up what we can identify
        tracing::info!("integration test complete - received and indexed {total_labels} labels");
    }

    #[tokio::test]
    async fn test_firehose_live_pipeline() {
        use crate::ingester::IngesterManager;
        use crate::types::{CommitData, FirehoseEvent, IndexJob, RepoOp};

        // Initialize tracing
        drop(tracing_subscriber::fmt().with_env_filter("info").try_init());

        let (storage, _dir) = setup_test_storage();
        let pool = setup_test_pool();
        let test_did = "did:plc:firehoselivetest";

        // Cleanup any existing test data
        cleanup_test_data(&pool, test_did).await;

        tracing::info!("starting firehose_live pipeline test");

        // Step 1: Create a firehose event with operations
        let event = FirehoseEvent {
            seq: 99999,
            did: test_did.to_owned(),
            time: chrono::Utc::now().to_rfc3339(),
            kind: "commit".to_owned(),
            commit: Some(CommitData {
                rev: "test-rev-live".to_owned(),
                ops: vec![
                    RepoOp {
                        action: "create".to_owned(),
                        path: "app.bsky.feed.post/testpost1".to_owned(),
                        cid: Some(
                            "bafyreihzwnyumvubacqyflkxpsejegc6sxwkcaxv3iwm3lrn3x45gxkioa"
                                .to_owned(),
                        ),
                    },
                    RepoOp {
                        action: "create".to_owned(),
                        path: "app.bsky.feed.like/testlike1".to_owned(),
                        cid: Some(
                            "bafyreihzwnyumvubacqyflkxpsejegc6sxwkcaxv3iwm3lrn3x45gxkioa"
                                .to_owned(),
                        ),
                    },
                ],
                blocks: vec![],
            }),
        };

        // Step 2: Simulate ingester processing event (enqueue to firehose_live)
        tracing::info!("enqueuing event to firehose_live queue");
        IngesterManager::enqueue_event_for_indexing(&storage, &event)
            .await
            .expect("failed to enqueue event");

        // Verify queue has 2 jobs
        let queue_len = storage.firehose_live_len().unwrap();
        assert_eq!(queue_len, 2, "expected 2 jobs in firehose_live queue");
        tracing::info!("firehose_live queue has {queue_len} jobs");

        // Step 3: Simulate indexer dequeuing and processing jobs
        let mut processed_count = 0;

        while let Ok(Some((key, job))) = storage.dequeue_firehose_live() {
            tracing::info!("processing job: uri={}, action={:?}", job.uri, job.action);

            // For this test, we'll create minimal records directly
            // (in production, indexer would extract from CAR blocks)
            let record = match job.uri.as_str() {
                uri if uri.contains("app.bsky.feed.post") => Some(serde_json::json!({
                    "text": "Test post from firehose_live pipeline",
                    "createdAt": job.indexed_at,
                })),
                uri if uri.contains("app.bsky.feed.like") => Some(serde_json::json!({
                    "subject": {
                        "uri": "at://did:plc:test/app.bsky.feed.post/abc",
                        "cid": "bafyreihzwnyumvubacqyflkxpsejegc6sxwkcaxv3iwm3lrn3x45gxkioa"
                    },
                    "createdAt": job.indexed_at,
                })),
                _ => None,
            };

            let job_with_record = IndexJob {
                record: record.clone(),
                ..job
            };

            tracing::info!(
                "about to process job with record: uri={}, has_record={}, record={:?}",
                job_with_record.uri,
                job_with_record.record.is_some(),
                record
            );

            // Process the job through indexer
            match IndexerManager::process_job(&pool, &job_with_record).await {
                Ok(()) => tracing::info!("successfully processed job {}", job_with_record.uri),
                Err(e) => {
                    tracing::error!("failed to process job {}: {}", job_with_record.uri, e);
                    panic!("failed to process index job: {e}");
                }
            }

            // Remove from queue after successful processing
            storage
                .remove_firehose_live(&key)
                .expect("failed to remove job from queue");

            processed_count += 1;
        }

        assert_eq!(processed_count, 2, "expected to process 2 jobs");
        tracing::info!("processed {processed_count} jobs from firehose_live queue");

        // Step 4: Verify queue is empty
        let final_queue_len = storage.firehose_live_len().unwrap();
        assert_eq!(final_queue_len, 0, "queue should be empty after processing");

        // Step 5: Verify data was written to database
        let client = pool.get().await.expect("failed to get db client");

        let post_count: i64 = client
            .query_one("SELECT COUNT(*) FROM post WHERE creator = $1", &[&test_did])
            .await
            .expect("failed to query post count")
            .get(0);

        let like_count: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM \"like\" WHERE creator = $1",
                &[&test_did],
            )
            .await
            .expect("failed to query like count")
            .get(0);

        tracing::info!("database verification: {post_count} posts, {like_count} likes");

        assert_eq!(post_count, 1, "expected 1 post in database");
        assert_eq!(like_count, 1, "expected 1 like in database");

        // Cleanup
        cleanup_test_data(&pool, test_did).await;

        tracing::info!(
            "firehose_live pipeline test complete - successfully processed 2 operations end-to-end"
        );
    }

    #[tokio::test]
    async fn test_index_job_create_without_record() {
        use crate::types::{IndexJob, WriteAction};

        let pool = setup_test_pool();

        let job = IndexJob {
            uri: "at://did:plc:test/app.bsky.feed.post/123".to_owned(),
            cid: "bafytest".to_owned(),
            action: WriteAction::Create,
            record: None, // Missing record for create
            indexed_at: "2024-01-01T00:00:00Z".to_owned(),
            rev: "test".to_owned(),
        };

        let result = IndexerManager::process_job(&pool, &job).await;

        // Should fail with missing record error
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("missing record"),
            "unexpected error: {err_msg}"
        );
    }

    #[tokio::test]
    async fn test_index_job_delete_operation() {
        use crate::types::{IndexJob, WriteAction};

        let pool = setup_test_pool();
        let test_did = "did:plc:deletetest";
        let test_uri = format!("at://{test_did}/app.bsky.feed.post/abc123");

        // First create a post
        let create_job = IndexJob {
            uri: test_uri.clone(),
            cid: "bafytest1".to_owned(),
            action: WriteAction::Create,
            record: Some(serde_json::json!({
                "text": "Test post to be deleted",
                "createdAt": "2024-01-01T00:00:00Z"
            })),
            indexed_at: "2024-01-01T00:00:00Z".to_owned(),
            rev: "rev1".to_owned(),
        };

        IndexerManager::process_job(&pool, &create_job)
            .await
            .unwrap();

        // Verify post exists
        let client = pool.get().await.unwrap();
        let row = client
            .query_one("SELECT COUNT(*) FROM post WHERE uri = $1", &[&test_uri])
            .await
            .unwrap();
        let count: i64 = row.get(0);
        assert_eq!(count, 1, "post should exist before delete");

        // Now delete it
        let delete_job = IndexJob {
            uri: test_uri.clone(),
            cid: "bafytest1".to_owned(),
            action: WriteAction::Delete,
            record: None,
            indexed_at: "2024-01-01T01:00:00Z".to_owned(),
            rev: "rev2".to_owned(),
        };

        IndexerManager::process_job(&pool, &delete_job)
            .await
            .unwrap();

        // Verify post was deleted
        let row = client
            .query_one("SELECT COUNT(*) FROM post WHERE uri = $1", &[&test_uri])
            .await
            .unwrap();
        let count: i64 = row.get(0);
        assert_eq!(count, 0, "post should be deleted");

        // Cleanup
        cleanup_test_data(&pool, test_did).await;
    }

    #[tokio::test]
    async fn test_index_job_stale_write_detection() {
        use crate::types::{IndexJob, WriteAction};

        let pool = setup_test_pool();
        let test_did = "did:plc:staletest";
        let test_uri = format!("at://{test_did}/app.bsky.feed.post/xyz789");

        // Create initial post with rev2
        let initial_job = IndexJob {
            uri: test_uri.clone(),
            cid: "bafytest2".to_owned(),
            action: WriteAction::Create,
            record: Some(serde_json::json!({
                "text": "Newer post",
                "createdAt": "2024-01-01T00:00:00Z"
            })),
            indexed_at: "2024-01-01T00:00:00Z".to_owned(),
            rev: "rev2".to_owned(),
        };

        IndexerManager::process_job(&pool, &initial_job)
            .await
            .unwrap();

        // Try to write older revision (rev1)
        let stale_job = IndexJob {
            uri: test_uri.clone(),
            cid: "bafytest1".to_owned(),
            action: WriteAction::Create,
            record: Some(serde_json::json!({
                "text": "Older post",
                "createdAt": "2024-01-01T00:00:00Z"
            })),
            indexed_at: "2024-01-01T00:00:00Z".to_owned(),
            rev: "rev1".to_owned(), // Older revision
        };

        // Should succeed but skip the stale write
        IndexerManager::process_job(&pool, &stale_job)
            .await
            .unwrap();

        // Verify the newer revision is still there
        let client = pool.get().await.unwrap();
        let row = client
            .query_one("SELECT rev FROM record WHERE uri = $1", &[&test_uri])
            .await
            .unwrap();
        let stored_rev: String = row.get(0);
        assert_eq!(stored_rev, "rev2", "should keep newer revision");

        // Cleanup
        cleanup_test_data(&pool, test_did).await;
    }

    // Tests for new helper functions

    #[tokio::test]
    async fn test_update_queue_metrics() {
        let (storage, _dir) = setup_test_storage();
        let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgresql://postgres:postgres@localhost:5432/bsky_test".to_owned()
        });
        let manager = IndexerManager::new(Arc::new(storage), &database_url).unwrap();

        // Enqueue some jobs
        for i in 0..5 {
            let job = crate::types::IndexJob {
                uri: format!("at://did:plc:test/app.bsky.feed.post/test{i}"),
                cid: "bafytest".to_owned(),
                action: WriteAction::Create,
                record: Some(serde_json::json!({"text": "test"})),
                indexed_at: "2024-01-01T00:00:00Z".to_owned(),
                rev: "test".to_owned(),
            };
            manager.storage.enqueue_firehose_live(&job).unwrap();
        }

        // Call update_queue_metrics
        manager.update_queue_metrics();

        // Verify metrics were updated (can't directly access metrics, but verify it doesn't panic)
        let queue_len = manager.storage.firehose_live_len().unwrap();
        assert_eq!(queue_len, 5);
    }

    #[tokio::test]
    async fn test_dequeue_prioritized_jobs_empty_queues() {
        let (storage, _dir) = setup_test_storage();
        let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgresql://postgres:postgres@localhost:5432/bsky_test".to_owned()
        });
        let manager = IndexerManager::new(Arc::new(storage), &database_url).unwrap();

        let (jobs, label_jobs) = manager.dequeue_prioritized_jobs();
        assert_eq!(jobs.len(), 0, "should return empty jobs vec");
        assert_eq!(label_jobs.len(), 0, "should return empty label_jobs vec");
    }

    #[tokio::test]
    async fn test_dequeue_prioritized_jobs_firehose_live_priority() {
        let (storage, _dir) = setup_test_storage();
        let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgresql://postgres:postgres@localhost:5432/bsky_test".to_owned()
        });
        let manager = IndexerManager::new(Arc::new(storage), &database_url).unwrap();

        // Add jobs to both queues
        for i in 0..3 {
            let job = crate::types::IndexJob {
                uri: format!("at://did:plc:live/app.bsky.feed.post/test{i}"),
                cid: "bafylive".to_owned(),
                action: WriteAction::Create,
                record: Some(serde_json::json!({"text": "live"})),
                indexed_at: "2024-01-01T00:00:00Z".to_owned(),
                rev: "test".to_owned(),
            };
            manager.storage.enqueue_firehose_live(&job).unwrap();
        }

        for i in 0..3 {
            let job = crate::types::IndexJob {
                uri: format!("at://did:plc:backfill/app.bsky.feed.post/test{i}"),
                cid: "bafybackfill".to_owned(),
                action: WriteAction::Create,
                record: Some(serde_json::json!({"text": "backfill"})),
                indexed_at: "2024-01-01T00:00:00Z".to_owned(),
                rev: "test".to_owned(),
            };
            manager.storage.enqueue_firehose_backfill(&job).unwrap();
        }

        let (jobs, _label_jobs) = manager.dequeue_prioritized_jobs();

        // Should get firehose_live jobs first (dequeue returns same items until removed)
        assert!(!jobs.is_empty());
        // First batch should be from firehose_live
        let first_cid = &jobs[0].1.cid;
        assert_eq!(
            first_cid, "bafylive",
            "should prioritize firehose_live over backfill"
        );
    }

    #[tokio::test]
    async fn test_spawn_index_job_tasks_empty() {
        let (storage, _dir) = setup_test_storage();
        let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgresql://postgres:postgres@localhost:5432/bsky_test".to_owned()
        });
        let manager = IndexerManager::new(Arc::new(storage), &database_url).unwrap();

        let tasks = manager.spawn_index_job_tasks(vec![]).await;
        assert_eq!(
            tasks.len(),
            0,
            "should return empty tasks vec for empty input"
        );
    }

    #[tokio::test]
    async fn test_handle_job_results_success() {
        let (storage, _dir) = setup_test_storage();
        let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgresql://postgres:postgres@localhost:5432/bsky_test".to_owned()
        });
        let manager = IndexerManager::new(Arc::new(storage), &database_url).unwrap();

        // Enqueue a job
        let job = crate::types::IndexJob {
            uri: "at://did:plc:test123/app.bsky.feed.post/test123".to_owned(),
            cid: "bafytest".to_owned(),
            action: WriteAction::Create,
            record: Some(serde_json::json!({"text": "test"})),
            indexed_at: "2024-01-01T00:00:00Z".to_owned(),
            rev: "test".to_owned(),
        };
        manager.storage.enqueue_firehose_live(&job).unwrap();

        let (key, _) = manager.storage.dequeue_firehose_live().unwrap().unwrap();

        // Create a successful task result
        let task =
            tokio::spawn(async move { (key, crate::indexer::QueueSource::FirehoseLive, Ok(())) });

        manager.handle_job_results(vec![task]).await;

        // Verify job was removed (queue should be empty after removal)
        // Note: We can't verify directly as the queue still has items until we explicitly remove them
        // This test verifies the handler logic runs without panicking
    }

    #[tokio::test]
    async fn test_handle_job_results_failure() {
        let (storage, _dir) = setup_test_storage();
        let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgresql://postgres:postgres@localhost:5432/bsky_test".to_owned()
        });
        let manager = IndexerManager::new(Arc::new(storage), &database_url).unwrap();

        // Create a failed task result
        let task = tokio::spawn(async move {
            (
                b"test_key".to_vec(),
                crate::indexer::QueueSource::FirehoseLive,
                Err(crate::types::WintermuteError::Other("test error".into())),
            )
        });

        manager.handle_job_results(vec![task]).await;

        // Verify the error was handled (this test verifies error handling doesn't panic)
        // The INDEXER_RECORDS_FAILED_TOTAL metric should be incremented
    }

    #[tokio::test]
    async fn test_process_label_event_empty() {
        let pool = setup_test_pool();

        // Empty labels list should complete without error
        let label_event = LabelEvent {
            seq: 1,
            labels: vec![],
        };
        let result = IndexerManager::process_label_event(&pool, &label_event).await;
        assert!(result.is_ok());
    }

    // Test delete operations for existing collection types
    // This tests the delete_* functions which are largely uncovered
    #[tokio::test]
    async fn test_delete_operations() {
        use crate::types::{IndexJob, WriteAction};
        use serde_json::json;

        let pool = setup_test_pool();
        let test_did = "did:plc:deleteopstest";

        cleanup_test_data(&pool, test_did).await;

        let indexed_at = chrono::Utc::now().to_rfc3339();

        // Test delete for each major collection type
        let test_collections = vec![
            (
                "app.bsky.feed.post",
                json!({"text": "test post", "createdAt": indexed_at.clone()}),
            ),
            (
                "app.bsky.feed.like",
                json!({"subject": {"uri": "at://did:plc:test/app.bsky.feed.post/abc", "cid": "bafytest"}, "createdAt": indexed_at.clone()}),
            ),
            (
                "app.bsky.graph.follow",
                json!({"subject": "did:plc:test", "createdAt": indexed_at.clone()}),
            ),
            (
                "app.bsky.feed.repost",
                json!({"subject": {"uri": "at://did:plc:test/app.bsky.feed.post/def", "cid": "bafytest"}, "createdAt": indexed_at.clone()}),
            ),
            (
                "app.bsky.graph.block",
                json!({"subject": "did:plc:blocked", "createdAt": indexed_at.clone()}),
            ),
            (
                "app.bsky.actor.profile",
                json!({"displayName": "Test Profile"}),
            ),
            (
                "app.bsky.feed.generator",
                json!({"did": test_did, "displayName": "Test Feed"}),
            ),
            (
                "app.bsky.graph.list",
                json!({"name": "Test List", "purpose": "app.bsky.graph.defs#modlist"}),
            ),
            (
                "app.bsky.graph.listitem",
                json!({"subject": "did:plc:test", "list": format!("at://{test_did}/app.bsky.graph.list/testlist")}),
            ),
        ];

        for (collection, record) in test_collections {
            let uri = format!("at://{test_did}/{collection}/testrkey");

            // First create the record
            let create_job = IndexJob {
                action: WriteAction::Create,
                uri: uri.clone(),
                cid: "bafytest123".to_owned(),
                rev: "rev1".to_owned(),
                record: Some(record),
                indexed_at: indexed_at.clone(),
            };

            let result = IndexerManager::process_job(&pool, &create_job).await;
            assert!(
                result.is_ok(),
                "Failed to create {collection}: {:?}",
                result.err()
            );

            // Then delete it
            let delete_job = IndexJob {
                action: WriteAction::Delete,
                uri: uri.clone(),
                cid: String::new(),
                rev: "rev2".to_owned(),
                record: None,
                indexed_at: indexed_at.clone(),
            };

            let result = IndexerManager::process_job(&pool, &delete_job).await;
            assert!(
                result.is_ok(),
                "Failed to delete {collection}: {:?}",
                result.err()
            );
        }

        cleanup_test_data(&pool, test_did).await;
    }

    #[tokio::test]
    async fn test_newer_collection_types() {
        use crate::types::{IndexJob, WriteAction};
        use serde_json::json;

        let pool = setup_test_pool();
        let test_did = "did:plc:newercollections";
        cleanup_test_data(&pool, test_did).await;
        let indexed_at = chrono::Utc::now().to_rfc3339();

        // Test newer collection types that were previously untested
        let test_collections = vec![
            (
                "app.bsky.verification.proof",
                json!({
                    "subject": "did:plc:verified",
                    "handle": "verified.test",
                    "displayName": "Verified User",
                    "createdAt": indexed_at.clone()
                }),
            ),
            (
                "app.bsky.graph.starterpack",
                json!({
                    "name": "Test Starter Pack",
                    "createdAt": indexed_at.clone()
                }),
            ),
            (
                "app.bsky.labeler.service",
                json!({
                    "createdAt": indexed_at.clone()
                }),
            ),
            (
                "app.bsky.feed.threadgate",
                json!({
                    "post": format!("at://{test_did}/app.bsky.feed.post/testpost"),
                    "createdAt": indexed_at.clone()
                }),
            ),
            (
                "app.bsky.feed.postgate",
                json!({
                    "post": format!("at://{test_did}/app.bsky.feed.post/testpost"),
                    "createdAt": indexed_at.clone()
                }),
            ),
            (
                "app.bsky.graph.listblock",
                json!({
                    "subject": format!("at://{test_did}/app.bsky.graph.list/testlist"),
                    "createdAt": indexed_at.clone()
                }),
            ),
        ];

        for (collection, record) in &test_collections {
            let uri = format!("at://{test_did}/{collection}/testrkey");

            // First create the record
            let create_job = IndexJob {
                action: WriteAction::Create,
                uri: uri.clone(),
                cid: "bafytest123".to_owned(),
                rev: "rev1".to_owned(),
                record: Some(record.clone()),
                indexed_at: indexed_at.clone(),
            };
            let result = IndexerManager::process_job(&pool, &create_job).await;
            assert!(
                result.is_ok(),
                "Failed to create {collection}: {:?}",
                result.err()
            );

            // Then delete it
            let delete_job = IndexJob {
                action: WriteAction::Delete,
                uri: uri.clone(),
                cid: String::new(),
                rev: "rev2".to_owned(),
                record: None,
                indexed_at: indexed_at.clone(),
            };
            let result = IndexerManager::process_job(&pool, &delete_job).await;
            assert!(
                result.is_ok(),
                "Failed to delete {collection}: {:?}",
                result.err()
            );
        }

        cleanup_test_data(&pool, test_did).await;
    }
}
