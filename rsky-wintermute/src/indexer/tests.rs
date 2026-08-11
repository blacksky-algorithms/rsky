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

        // post_agg is keyed by post uri, so it must go before the posts do.
        drop(
            client
                .execute(
                    "DELETE FROM post_agg WHERE uri IN (SELECT uri FROM post WHERE creator = $1)",
                    &[&did],
                )
                .await,
        );

        // One DELETE per candidate column: a single OR query fails outright on
        // tables that lack any one of these columns, silently deleting nothing.
        // Table names are quoted ("like" is a reserved word).
        for table in &tables {
            for column in ["creator", "did", "author"] {
                let query = format!("DELETE FROM \"{table}\" WHERE {column} = $1");
                drop(client.execute(&query, &[&did]).await);
            }
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
            priority: false,
        };

        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .unwrap();

        tracing::info!("processing backfill job for {test_did}");
        let result =
            BackfillerManager::process_job(&storage, &http_client, &dashmap::DashMap::new(), &job)
                .await;

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
                            IndexerManager::process_job(&indexer.pool_backfill, &index_job, false)
                                .await;

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
            priority: false,
        };

        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .unwrap();

        let result =
            BackfillerManager::process_job(&storage, &http_client, &dashmap::DashMap::new(), &job)
                .await;
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
                            IndexerManager::process_job(&indexer.pool_backfill, &index_job, false)
                                .await;

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

        let result = IndexerManager::process_job(&pool, &valid_job, false).await;
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
                cid: None,
                val: "spam".to_owned(),
                neg: false,
                cts: "2025-01-20T10:00:00Z".to_owned(),
                exp: None,
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
                "SELECT src, uri, val, cts, neg FROM label WHERE src = $1 AND cid = ''",
                &[&test_src],
            )
            .await
            .unwrap();

        let src: String = row.get(0);
        let uri: String = row.get(1);
        let val: String = row.get(2);
        let cts: String = row.get(3);
        let neg: bool = row.get(4);

        assert_eq!(src, test_src);
        assert_eq!(uri, test_uri);
        assert_eq!(val, "spam");
        assert_eq!(cts, "2025-01-20T10:00:00Z");
        assert!(!neg, "expected neg to be false");

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
                    cid: None,
                    val: "spam".to_owned(),
                    neg: false,
                    cts: "2025-01-20T10:00:00Z".to_owned(),
                    exp: None,
                },
                crate::types::Label {
                    src: test_src.to_owned(),
                    uri: "at://did:plc:user2/app.bsky.feed.post/post2".to_owned(),
                    cid: None,
                    val: "nsfw".to_owned(),
                    neg: false,
                    cts: "2025-01-20T10:01:00Z".to_owned(),
                    exp: None,
                },
                crate::types::Label {
                    src: test_src.to_owned(),
                    uri: "at://did:plc:user3/app.bsky.feed.post/post3".to_owned(),
                    cid: None,
                    val: "porn".to_owned(),
                    neg: false,
                    cts: "2025-01-20T10:02:00Z".to_owned(),
                    exp: None,
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
                cid: None,
                val: "spam".to_owned(),
                neg: false,
                cts: "2025-01-20T10:00:00Z".to_owned(),
                exp: None,
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
                cid: None,
                val: "spam".to_owned(),
                neg: false,
                cts: "2025-01-20T11:00:00Z".to_owned(), // Different timestamp
                exp: None,
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
                cid: None,
                val: "spam".to_owned(),
                neg: false,
                cts: "2025-01-20T10:00:00Z".to_owned(),
                exp: None,
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
                cid: None,
                val: "spam".to_owned(),
                neg: false,
                cts: "2025-01-20T10:01:00Z".to_owned(),
                exp: None,
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
                cid: None,
                val: "spam".to_owned(),
                neg: false,
                cts: "2025-01-20T10:00:00Z".to_owned(),
                exp: None,
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
                cid: None,
                val: "nsfw".to_owned(),
                neg: false,
                cts: "2025-01-20T10:01:00Z".to_owned(),
                exp: None,
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
                cid: None,
                val: "spam".to_owned(),
                neg: false,
                cts: "2025-01-20T10:00:00Z".to_owned(),
                exp: None,
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
    async fn test_label_negation() {
        let pool = setup_test_pool();
        let test_src = "did:plc:test_labeler_negation";
        let test_uri = "did:plc:user_negation_test";

        cleanup_test_labels(&pool, test_src).await;

        // First: apply a takedown label
        let label_event1 = crate::types::LabelEvent {
            seq: 8000,
            labels: vec![crate::types::Label {
                src: test_src.to_owned(),
                uri: test_uri.to_owned(),
                cid: None,
                val: "!takedown".to_owned(),
                neg: false,
                cts: "2025-01-20T10:00:00Z".to_owned(),
                exp: None,
            }],
        };

        let result = IndexerManager::process_label_event(&pool, &label_event1).await;
        assert!(result.is_ok());

        // Verify label exists with neg=false
        let client = pool.get().await.unwrap();
        let row = client
            .query_one(
                "SELECT neg FROM label WHERE src = $1 AND uri = $2 AND cid = '' AND val = '!takedown'",
                &[&test_src, &test_uri],
            )
            .await
            .unwrap();
        let neg: bool = row.get(0);
        assert!(!neg, "initial label should have neg=false");

        // Second: negate the takedown label
        let label_event2 = crate::types::LabelEvent {
            seq: 8001,
            labels: vec![crate::types::Label {
                src: test_src.to_owned(),
                uri: test_uri.to_owned(),
                cid: None,
                val: "!takedown".to_owned(),
                neg: true,
                cts: "2025-01-20T11:00:00Z".to_owned(),
                exp: None,
            }],
        };

        let result = IndexerManager::process_label_event(&pool, &label_event2).await;
        assert!(result.is_ok());

        // Verify label now has neg=true (upserted, not duplicated)
        let count: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM label WHERE src = $1 AND uri = $2 AND cid = '' AND val = '!takedown'",
                &[&test_src, &test_uri],
            )
            .await
            .unwrap()
            .get(0);
        assert_eq!(count, 1, "should still be 1 row after negation");

        let row = client
            .query_one(
                "SELECT neg, cts FROM label WHERE src = $1 AND uri = $2 AND cid = '' AND val = '!takedown'",
                &[&test_src, &test_uri],
            )
            .await
            .unwrap();
        let neg: bool = row.get(0);
        let cts: String = row.get(1);
        assert!(neg, "neg should be true after negation");
        assert_eq!(cts, "2025-01-20T11:00:00Z", "cts should be updated");

        cleanup_test_labels(&pool, test_src).await;
    }

    #[tokio::test]
    async fn test_label_negation_cid_mismatch() {
        // When a negation arrives with a different CID than the original label,
        // the original should still be negated. This is the scenario that caused
        // The Green List takedown to persist even after Bluesky restored the list.
        let pool = setup_test_pool();
        let test_src = "did:plc:test_labeler_cid_mismatch";
        let test_uri = "at://did:plc:test_user/app.bsky.graph.list/cid_mismatch_test";

        cleanup_test_labels(&pool, test_src).await;

        // Apply a takedown label with empty CID
        let label_event1 = crate::types::LabelEvent {
            seq: 9000,
            labels: vec![crate::types::Label {
                src: test_src.to_owned(),
                uri: test_uri.to_owned(),
                cid: None,
                val: "!takedown".to_owned(),
                neg: false,
                cts: "2025-02-05T21:00:00Z".to_owned(),
                exp: None,
            }],
        };

        let result = IndexerManager::process_label_event(&pool, &label_event1).await;
        assert!(result.is_ok());

        // Negate with a DIFFERENT CID (this is what Bluesky's moderation does)
        let label_event2 = crate::types::LabelEvent {
            seq: 9001,
            labels: vec![crate::types::Label {
                src: test_src.to_owned(),
                uri: test_uri.to_owned(),
                cid: Some("bafyreidtaceuvwlkvtbrgjeite7jegu3zxwxoqpk7rsnj7ty6jpfnmp7uq".to_owned()),
                val: "!takedown".to_owned(),
                neg: true,
                cts: "2025-02-05T21:28:14.424Z".to_owned(),
                exp: None,
            }],
        };

        let result = IndexerManager::process_label_event(&pool, &label_event2).await;
        assert!(result.is_ok());

        let client = pool.get().await.unwrap();

        // The original empty-CID label should now be negated
        let row = client
            .query_one(
                "SELECT neg FROM label WHERE src = $1 AND uri = $2 AND cid = '' AND val = '!takedown'",
                &[&test_src, &test_uri],
            )
            .await
            .unwrap();
        let neg: bool = row.get(0);
        assert!(
            neg,
            "original empty-CID label should be negated when negation arrives with different CID"
        );

        // The negation row itself should also exist with neg=true
        let neg_row = client
            .query_one(
                "SELECT neg FROM label WHERE src = $1 AND uri = $2 AND cid = 'bafyreidtaceuvwlkvtbrgjeite7jegu3zxwxoqpk7rsnj7ty6jpfnmp7uq' AND val = '!takedown'",
                &[&test_src, &test_uri],
            )
            .await
            .unwrap();
        let neg2: bool = neg_row.get(0);
        assert!(neg2, "negation row should have neg=true");

        // Both rows should have neg=true -- no active takedown remains
        let active_count: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM label WHERE src = $1 AND uri = $2 AND val = '!takedown' AND neg = false",
                &[&test_src, &test_uri],
            )
            .await
            .unwrap()
            .get(0);
        assert_eq!(
            active_count, 0,
            "no active (neg=false) takedown labels should remain"
        );

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
            identity: None,
            account: None,
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
            match IndexerManager::process_job(&pool, &job_with_record, false).await {
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

        let result = IndexerManager::process_job(&pool, &job, false).await;

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

        IndexerManager::process_job(&pool, &create_job, false)
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

        IndexerManager::process_job(&pool, &delete_job, false)
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

        IndexerManager::process_job(&pool, &initial_job, false)
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
        IndexerManager::process_job(&pool, &stale_job, false)
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

    async fn count_one(client: &deadpool_postgres::Client, sql: &str, uri: &str) -> i64 {
        client.query_one(sql, &[&uri]).await.unwrap().get(0)
    }

    const NOTIF_BY_RECORD: &str = "SELECT COUNT(*) FROM notification WHERE \"recordUri\" = $1";

    fn subject_record(subject_uri: &str) -> serde_json::Value {
        serde_json::json!({
            "subject": {"uri": subject_uri, "cid": "bafysubject1"},
            "createdAt": "2024-01-01T00:00:00Z"
        })
    }

    #[tokio::test]
    async fn test_delete_like_retracts_notification() {
        use crate::types::{IndexJob, WriteAction};

        let pool = setup_test_pool();
        let test_did = "did:plc:likenotifretract";
        let subject_did = "did:plc:likenotifretractsubject";
        let test_uri = format!("at://{test_did}/app.bsky.feed.like/ln1");
        let subject_uri = format!("at://{subject_did}/app.bsky.feed.post/sub1");

        cleanup_test_data(&pool, test_did).await;
        cleanup_test_data(&pool, subject_did).await;

        IndexerManager::process_job(
            &pool,
            &IndexJob {
                uri: test_uri.clone(),
                cid: "bafylike1".to_owned(),
                action: WriteAction::Create,
                record: Some(subject_record(&subject_uri)),
                indexed_at: "2024-01-01T00:00:00Z".to_owned(),
                rev: "rev1".to_owned(),
            },
            false,
        )
        .await
        .unwrap();

        let client = pool.get().await.unwrap();
        assert_eq!(
            count_one(&client, NOTIF_BY_RECORD, &test_uri).await,
            1,
            "like notification should exist before delete"
        );

        IndexerManager::process_job(
            &pool,
            &IndexJob {
                uri: test_uri.clone(),
                cid: "bafylike1".to_owned(),
                action: WriteAction::Delete,
                record: None,
                indexed_at: "2024-01-01T01:00:00Z".to_owned(),
                rev: "rev2".to_owned(),
            },
            false,
        )
        .await
        .unwrap();

        assert_eq!(
            count_one(
                &client,
                "SELECT COUNT(*) FROM \"like\" WHERE uri = $1",
                &test_uri
            )
            .await,
            0,
            "like row should be deleted"
        );
        assert_eq!(
            count_one(&client, NOTIF_BY_RECORD, &test_uri).await,
            0,
            "like notification should be retracted"
        );

        cleanup_test_data(&pool, test_did).await;
        cleanup_test_data(&pool, subject_did).await;
    }

    #[tokio::test]
    async fn test_delete_repost_retracts_notification_with_boilerplate_skip() {
        use crate::types::{IndexJob, WriteAction};

        let pool = setup_test_pool();
        let test_did = "did:plc:repostnotifretract";
        let subject_did = "did:plc:repostnotifretractsubject";
        let test_uri = format!("at://{test_did}/app.bsky.feed.repost/rn1");
        let subject_uri = format!("at://{subject_did}/app.bsky.feed.post/sub1");

        cleanup_test_data(&pool, test_did).await;
        cleanup_test_data(&pool, subject_did).await;

        IndexerManager::process_job(
            &pool,
            &IndexJob {
                uri: test_uri.clone(),
                cid: "bafyrepost1".to_owned(),
                action: WriteAction::Create,
                record: Some(subject_record(&subject_uri)),
                indexed_at: "2024-01-01T00:00:00Z".to_owned(),
                rev: "rev1".to_owned(),
            },
            true,
        )
        .await
        .unwrap();

        let client = pool.get().await.unwrap();
        assert_eq!(
            count_one(&client, NOTIF_BY_RECORD, &test_uri).await,
            1,
            "repost notification should exist before delete"
        );
        assert_eq!(
            count_one(
                &client,
                "SELECT COUNT(*) FROM record WHERE uri = $1",
                &test_uri
            )
            .await,
            0,
            "boilerplate skip should leave no record row to gate on"
        );

        IndexerManager::process_job(
            &pool,
            &IndexJob {
                uri: test_uri.clone(),
                cid: "bafyrepost1".to_owned(),
                action: WriteAction::Delete,
                record: None,
                indexed_at: "2024-01-01T01:00:00Z".to_owned(),
                rev: "rev2".to_owned(),
            },
            true,
        )
        .await
        .unwrap();

        assert_eq!(
            count_one(
                &client,
                "SELECT COUNT(*) FROM repost WHERE uri = $1",
                &test_uri
            )
            .await,
            0,
            "repost row should be deleted"
        );
        assert_eq!(
            count_one(
                &client,
                "SELECT COUNT(*) FROM feed_item WHERE uri = $1",
                &test_uri
            )
            .await,
            0,
            "feed_item row should be deleted"
        );
        assert_eq!(
            count_one(&client, NOTIF_BY_RECORD, &test_uri).await,
            0,
            "repost notification should be retracted"
        );

        cleanup_test_data(&pool, test_did).await;
        cleanup_test_data(&pool, subject_did).await;
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

            let result = IndexerManager::process_job(&pool, &create_job, false).await;
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

            let result = IndexerManager::process_job(&pool, &delete_job, false).await;
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
                "app.bsky.graph.verification",
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
            let result = IndexerManager::process_job(&pool, &create_job, false).await;
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
            let result = IndexerManager::process_job(&pool, &delete_job, false).await;
            assert!(
                result.is_ok(),
                "Failed to delete {collection}: {:?}",
                result.err()
            );
        }

        cleanup_test_data(&pool, test_did).await;
    }

    #[tokio::test]
    async fn bulk_post_langs_tags_round_trip_as_arrays() {
        use crate::indexer::bulk::{self, PostCopyRow};

        let pool = setup_test_pool();
        let test_did = "did:plc:wintermute-test-langs-arrays";
        cleanup_test_data(&pool, test_did).await;

        let client = pool.get().await.unwrap();
        client
            .execute(
                "INSERT INTO actor (did, \"indexedAt\") VALUES ($1, NOW()) \
                 ON CONFLICT (did) DO NOTHING",
                &[&test_did],
            )
            .await
            .unwrap();

        let uri = format!("at://{test_did}/app.bsky.feed.post/langstest1");
        let langs_json = vec![serde_json::json!("en"), serde_json::json!("pt-BR")];
        let tags_json = vec![serde_json::json!(r#"tag "quoted""#)];
        let row = PostCopyRow {
            uri: uri.clone(),
            cid: "bafyreihhl5mpvjkrhnnagen2fomozzhnhhdq2jr6cego2nzbvmwewv5rd4".to_owned(),
            creator: test_did.to_owned(),
            text: "langs round trip".to_owned(),
            reply_root: None,
            reply_root_cid: None,
            reply_parent: None,
            reply_parent_cid: None,
            created_at: "2026-07-30T00:00:00.000Z".to_owned(),
            indexed_at: "2026-07-30T00:00:00.000Z".to_owned(),
            langs: Some(bulk::pg_text_array_literal(&langs_json)),
            tags: Some(bulk::pg_text_array_literal(&tags_json)),
        };
        bulk::copy_insert_posts(&client, &[row], true)
            .await
            .unwrap();

        let db_row = client
            .query_one(
                "SELECT langs::text[], tags::text[] FROM post WHERE uri = $1",
                &[&uri],
            )
            .await
            .unwrap();
        let langs: Vec<String> = db_row.get(0);
        let tags: Vec<String> = db_row.get(1);
        assert_eq!(langs, vec!["en".to_owned(), "pt-BR".to_owned()]);
        assert_eq!(tags, vec![r#"tag "quoted""#.to_owned()]);

        cleanup_test_data(&pool, test_did).await;
    }

    #[tokio::test]
    async fn bulk_aggregates_increment_exactly_and_are_replay_safe() {
        use crate::indexer::bulk::{self, PostCopyRow};

        let pool = setup_test_pool();
        let creator = "did:plc:wintermute-test-agg-creator";
        let subject = "did:plc:wintermute-test-agg-subject";
        for did in [creator, subject] {
            cleanup_test_data(&pool, did).await;
        }

        let client = pool.get().await.unwrap();
        for did in [creator, subject] {
            client
                .execute(
                    "INSERT INTO actor (did, \"indexedAt\") VALUES ($1, NOW()) \
                     ON CONFLICT (did) DO NOTHING",
                    &[&did],
                )
                .await
                .unwrap();
        }

        let post = |rkey: &str| PostCopyRow {
            uri: format!("at://{creator}/app.bsky.feed.post/{rkey}"),
            cid: "bafyreihhl5mpvjkrhnnagen2fomozzhnhhdq2jr6cego2nzbvmwewv5rd4".to_owned(),
            creator: creator.to_owned(),
            text: format!("agg test {rkey}"),
            reply_root: None,
            reply_root_cid: None,
            reply_parent: None,
            reply_parent_cid: None,
            created_at: "2026-07-30T00:00:00.000Z".to_owned(),
            indexed_at: "2026-07-30T00:00:00.000Z".to_owned(),
            langs: None,
            tags: None,
        };
        let posts = vec![post("aggpost1"), post("aggpost2")];
        bulk::copy_insert_posts(&client, &posts, true)
            .await
            .unwrap();
        // Replay the identical batch: dupes insert nothing and add nothing.
        bulk::copy_insert_posts(&client, &posts, true)
            .await
            .unwrap();

        let follow = bulk::FollowCopyRow {
            uri: format!("at://{creator}/app.bsky.graph.follow/aggfollow1"),
            cid: "bafyreihhl5mpvjkrhnnagen2fomozzhnhhdq2jr6cego2nzbvmwewv5rd4".to_owned(),
            creator: creator.to_owned(),
            subject_did: subject.to_owned(),
            created_at: "2026-07-30T00:00:00.000Z".to_owned(),
            indexed_at: "2026-07-30T00:00:00.000Z".to_owned(),
            via: None,
            via_cid: None,
        };
        bulk::copy_insert_follows(&client, std::slice::from_ref(&follow), true)
            .await
            .unwrap();
        bulk::copy_insert_follows(&client, std::slice::from_ref(&follow), true)
            .await
            .unwrap();

        let row = client
            .query_one(
                "SELECT \"postsCount\", \"followsCount\" FROM profile_agg WHERE did = $1",
                &[&creator],
            )
            .await
            .unwrap();
        assert_eq!(row.get::<_, i64>(0), 2, "postsCount must be exactly 2");
        assert_eq!(row.get::<_, i64>(1), 1, "followsCount must be exactly 1");

        let row = client
            .query_one(
                "SELECT \"followersCount\" FROM profile_agg WHERE did = $1",
                &[&subject],
            )
            .await
            .unwrap();
        assert_eq!(row.get::<_, i64>(0), 1, "followersCount must be exactly 1");

        for did in [creator, subject] {
            cleanup_test_data(&pool, did).await;
        }
    }

    #[test]
    fn collect_subject_notifications_recipients_and_self_skips() {
        let record = serde_json::json!({
            "subject": {"uri": "at://did:plc:subject-author/app.bsky.feed.post/abc"},
            "via": {"uri": "at://did:plc:reposter/app.bsky.feed.repost/xyz"},
        });
        let mut rows = Vec::new();
        IndexerManager::collect_subject_notifications(
            &mut rows,
            &record,
            "did:plc:liker",
            "at://did:plc:liker/app.bsky.feed.like/1",
            "cid1",
            "at://did:plc:subject-author/app.bsky.feed.post/abc",
            "2026-07-30T00:00:00.000Z",
            "like",
            "like-via-repost",
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].did, "did:plc:subject-author");
        assert_eq!(rows[0].reason, "like");
        assert_eq!(
            rows[0].reason_subject.as_deref(),
            Some("at://did:plc:subject-author/app.bsky.feed.post/abc")
        );
        assert_eq!(rows[1].did, "did:plc:reposter");
        assert_eq!(rows[1].reason, "like-via-repost");

        // Self-like and self-via generate nothing
        let mut rows = Vec::new();
        IndexerManager::collect_subject_notifications(
            &mut rows,
            &serde_json::json!({
                "subject": {"uri": "at://did:plc:liker/app.bsky.feed.post/own"},
                "via": {"uri": "at://did:plc:liker/app.bsky.feed.repost/own"},
            }),
            "did:plc:liker",
            "at://did:plc:liker/app.bsky.feed.like/2",
            "cid2",
            "at://did:plc:liker/app.bsky.feed.post/own",
            "2026-07-30T00:00:00.000Z",
            "like",
            "like-via-repost",
        );
        assert!(rows.is_empty());

        // Empty subject generates nothing
        let mut rows = Vec::new();
        IndexerManager::collect_subject_notifications(
            &mut rows,
            &serde_json::json!({}),
            "did:plc:liker",
            "at://did:plc:liker/app.bsky.feed.like/3",
            "cid3",
            "",
            "2026-07-30T00:00:00.000Z",
            "like",
            "like-via-repost",
        );
        assert!(rows.is_empty());
    }

    #[tokio::test]
    async fn bulk_notifications_and_post_agg_are_exact_and_replay_safe() {
        use crate::indexer::bulk::{self, NotificationRow, PostCopyRow};

        let pool = setup_test_pool();
        let author = "did:plc:wintermute-test-notif-author";
        let recipient = "did:plc:wintermute-test-notif-recip";
        for did in [author, recipient] {
            cleanup_test_data(&pool, did).await;
        }

        let client = pool.get().await.unwrap();
        for did in [author, recipient] {
            client
                .execute(
                    "INSERT INTO actor (did, \"indexedAt\") VALUES ($1, NOW()) \
                     ON CONFLICT (did) DO NOTHING",
                    &[&did],
                )
                .await
                .unwrap();
        }

        let parent_uri = format!("at://{recipient}/app.bsky.feed.post/notifparent");
        let cid = "bafyreihhl5mpvjkrhnnagen2fomozzhnhhdq2jr6cego2nzbvmwewv5rd4".to_owned();
        let ts = "2026-07-30T00:00:00.000Z".to_owned();
        client
            .execute("DELETE FROM post_agg WHERE uri = $1", &[&parent_uri])
            .await
            .unwrap();
        client
            .execute("DELETE FROM quote WHERE subject = $1", &[&parent_uri])
            .await
            .unwrap();

        // Parent post by recipient, then a batch reply by author: replyCount
        // increments exactly once across a replay.
        let parent = PostCopyRow {
            uri: parent_uri.clone(),
            cid: cid.clone(),
            creator: recipient.to_owned(),
            text: "parent".to_owned(),
            reply_root: None,
            reply_root_cid: None,
            reply_parent: None,
            reply_parent_cid: None,
            created_at: ts.clone(),
            indexed_at: ts.clone(),
            langs: None,
            tags: None,
        };
        let reply = PostCopyRow {
            uri: format!("at://{author}/app.bsky.feed.post/notifreply"),
            cid: cid.clone(),
            creator: author.to_owned(),
            text: "reply".to_owned(),
            reply_root: Some(parent_uri.clone()),
            reply_root_cid: Some(cid.clone()),
            reply_parent: Some(parent_uri.clone()),
            reply_parent_cid: Some(cid.clone()),
            created_at: ts.clone(),
            indexed_at: ts.clone(),
            langs: None,
            tags: None,
        };
        let applied = bulk::copy_insert_posts(&client, &[parent, reply], true)
            .await
            .unwrap();
        assert_eq!(applied.len(), 2, "both posts must apply");
        let replay_row = PostCopyRow {
            uri: format!("at://{author}/app.bsky.feed.post/notifreply"),
            cid: cid.clone(),
            creator: author.to_owned(),
            text: "reply replay".to_owned(),
            reply_root: Some(parent_uri.clone()),
            reply_root_cid: Some(cid.clone()),
            reply_parent: Some(parent_uri.clone()),
            reply_parent_cid: Some(cid.clone()),
            created_at: ts.clone(),
            indexed_at: ts.clone(),
            langs: None,
            tags: None,
        };
        let replayed = bulk::copy_insert_posts(&client, &[replay_row], true)
            .await
            .unwrap();
        assert!(replayed.is_empty(), "replayed post must not re-apply");

        // Batch likes and reposts of the parent: likeCount/repostCount exact
        // across replays.
        let like = bulk::SubjectRecordRow {
            uri: format!("at://{author}/app.bsky.feed.like/notiflike"),
            cid: cid.clone(),
            creator: author.to_owned(),
            subject: parent_uri.clone(),
            subject_cid: cid.clone(),
            created_at: ts.clone(),
            indexed_at: ts.clone(),
            via: None,
            via_cid: None,
        };
        let like_applied = bulk::copy_insert_likes(&client, std::slice::from_ref(&like), true)
            .await
            .unwrap();
        assert_eq!(like_applied.len(), 1);
        let like_replayed = bulk::copy_insert_likes(&client, std::slice::from_ref(&like), true)
            .await
            .unwrap();
        assert!(like_replayed.is_empty());

        let repost = bulk::SubjectRecordRow {
            uri: format!("at://{author}/app.bsky.feed.repost/notifrepost"),
            cid: cid.clone(),
            creator: author.to_owned(),
            subject: parent_uri.clone(),
            subject_cid: cid.clone(),
            created_at: ts.clone(),
            indexed_at: ts.clone(),
            via: None,
            via_cid: None,
        };
        bulk::copy_insert_reposts(&client, std::slice::from_ref(&repost), true)
            .await
            .unwrap();
        bulk::copy_insert_reposts(&client, std::slice::from_ref(&repost), true)
            .await
            .unwrap();

        // Batch quote of the parent: quoteCount exact across replays.
        let quote = (
            format!("at://{author}/app.bsky.feed.post/notifquote"),
            cid.clone(),
            parent_uri.clone(),
            cid.clone(),
            ts.clone(),
            ts.clone(),
        );
        bulk::copy_insert_quotes(&client, std::slice::from_ref(&quote), true)
            .await
            .unwrap();
        bulk::copy_insert_quotes(&client, std::slice::from_ref(&quote), true)
            .await
            .unwrap();

        let agg = client
            .query_one(
                "SELECT \"replyCount\", \"likeCount\", \"repostCount\", \"quoteCount\" \
                 FROM post_agg WHERE uri = $1",
                &[&parent_uri],
            )
            .await
            .unwrap();
        assert_eq!(agg.get::<_, i64>(0), 1, "replyCount must be exactly 1");
        assert_eq!(agg.get::<_, i64>(1), 1, "likeCount must be exactly 1");
        assert_eq!(agg.get::<_, i64>(2), 1, "repostCount must be exactly 1");
        assert_eq!(agg.get::<_, i64>(3), 1, "quoteCount must be exactly 1");

        // Bulk notifications dedupe on (did, recordUri, reason).
        let notif = NotificationRow {
            did: recipient.to_owned(),
            author: author.to_owned(),
            record_uri: format!("at://{author}/app.bsky.feed.like/notiflike"),
            record_cid: cid.clone(),
            reason: "like",
            reason_subject: Some(parent_uri.clone()),
            sort_at: ts.clone(),
        };
        bulk::copy_insert_notifications(&client, std::slice::from_ref(&notif))
            .await
            .unwrap();
        bulk::copy_insert_notifications(&client, std::slice::from_ref(&notif))
            .await
            .unwrap();
        let notif_count: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM notification WHERE did = $1 AND author = $2",
                &[&recipient, &author],
            )
            .await
            .unwrap()
            .get(0);
        assert_eq!(notif_count, 1, "notification must dedupe to exactly 1");

        client
            .execute("DELETE FROM post_agg WHERE uri = $1", &[&parent_uri])
            .await
            .unwrap();
        for did in [author, recipient] {
            cleanup_test_data(&pool, did).await;
        }
    }

    #[tokio::test]
    async fn batch_toggle_sequence_keeps_final_like_and_exact_counts() {
        use crate::indexer::bulk::PostCopyRow;
        use crate::types::{IndexJob, WriteAction};

        let pool = setup_test_pool();
        let liker = "did:plc:wintermute-test-toggle-liker";
        let author = "did:plc:wintermute-test-toggle-author";
        for did in [liker, author] {
            cleanup_test_data(&pool, did).await;
        }

        let client = pool.get().await.unwrap();
        for did in [liker, author] {
            client
                .execute(
                    "INSERT INTO actor (did, \"indexedAt\") VALUES ($1, NOW()) \
                     ON CONFLICT (did) DO NOTHING",
                    &[&did],
                )
                .await
                .unwrap();
        }

        let post_uri = format!("at://{author}/app.bsky.feed.post/togglepost");
        let cid = "bafyreihhl5mpvjkrhnnagen2fomozzhnhhdq2jr6cego2nzbvmwewv5rd4".to_owned();
        let ts = "2026-07-31T00:00:00.000Z".to_owned();
        client
            .execute("DELETE FROM post_agg WHERE uri = $1", &[&post_uri])
            .await
            .unwrap();
        crate::indexer::bulk::copy_insert_posts(
            &client,
            &[PostCopyRow {
                uri: post_uri.clone(),
                cid: cid.clone(),
                creator: author.to_owned(),
                text: "toggle target".to_owned(),
                reply_root: None,
                reply_root_cid: None,
                reply_parent: None,
                reply_parent_cid: None,
                created_at: ts.clone(),
                indexed_at: ts.clone(),
                langs: None,
                tags: None,
            }],
            true,
        )
        .await
        .unwrap();

        let like_record = serde_json::json!({
            "$type": "app.bsky.feed.like",
            "subject": {"uri": post_uri, "cid": cid},
            "createdAt": ts,
        });
        let like_uri = |rkey: &str| format!("at://{liker}/app.bsky.feed.like/{rkey}");
        let job = |rkey: &str, action: WriteAction, rev: &str, with_record: bool| IndexJob {
            uri: like_uri(rkey),
            cid: cid.clone(),
            action,
            record: with_record.then(|| like_record.clone()),
            indexed_at: ts.clone(),
            rev: rev.to_owned(),
        };

        // like, unlike, re-like within ONE drain batch: the phase split
        // (creates before deletes) must not eat the final like.
        let jobs = vec![
            (
                b"k1".to_vec(),
                job("toggle1", WriteAction::Create, "3a", true),
            ),
            (
                b"k2".to_vec(),
                job("toggle1", WriteAction::Delete, "3b", false),
            ),
            (
                b"k3".to_vec(),
                job("toggle2", WriteAction::Create, "3c", true),
            ),
        ];
        let (results, batch_failed) =
            IndexerManager::process_jobs_batch(&pool, &jobs, false, false).await;
        assert!(!batch_failed);
        assert_eq!(results.len(), 3);
        for (_, r) in &results {
            assert!(r.is_ok(), "job failed: {r:?}");
        }

        let final_like: Option<String> = client
            .query_opt(
                "SELECT uri FROM \"like\" WHERE creator = $1 AND subject = $2",
                &[&liker, &post_uri],
            )
            .await
            .unwrap()
            .map(|r| r.get(0));
        assert_eq!(
            final_like.as_deref(),
            Some(like_uri("toggle2").as_str()),
            "the re-like must survive the toggle sequence"
        );
        let agg: i64 = client
            .query_one(
                "SELECT \"likeCount\" FROM post_agg WHERE uri = $1",
                &[&post_uri],
            )
            .await
            .unwrap()
            .get(0);
        assert_eq!(agg, 1, "likeCount must be exactly 1 after toggle");

        // Cross-batch toggle: unlike toggle2 and re-like as toggle3 in ONE
        // batch (the create of toggle2 was a prior batch). Deletes must run
        // before creates or toggle3 dies on the (subject, creator) conflict
        // with toggle2's still-present row.
        let cross_jobs = vec![
            (
                b"k4".to_vec(),
                job("toggle2", WriteAction::Delete, "3d", false),
            ),
            (
                b"k5".to_vec(),
                job("toggle3", WriteAction::Create, "3e", true),
            ),
        ];
        let (results, batch_failed) =
            IndexerManager::process_jobs_batch(&pool, &cross_jobs, false, false).await;
        assert!(!batch_failed);
        assert!(results.iter().all(|(_, r)| r.is_ok()));

        let final_like: Option<String> = client
            .query_opt(
                "SELECT uri FROM \"like\" WHERE creator = $1 AND subject = $2",
                &[&liker, &post_uri],
            )
            .await
            .unwrap()
            .map(|r| r.get(0));
        assert_eq!(
            final_like.as_deref(),
            Some(like_uri("toggle3").as_str()),
            "the cross-batch re-like must survive the phase split"
        );
        let agg: i64 = client
            .query_one(
                "SELECT \"likeCount\" FROM post_agg WHERE uri = $1",
                &[&post_uri],
            )
            .await
            .unwrap()
            .get(0);
        assert_eq!(
            agg, 1,
            "likeCount must be exactly 1 after cross-batch toggle"
        );

        // Final unlike in its own batch: the batch delete path must decrement.
        let delete_jobs = vec![(
            b"k6".to_vec(),
            job("toggle3", WriteAction::Delete, "3f", false),
        )];
        let (results, batch_failed) =
            IndexerManager::process_jobs_batch(&pool, &delete_jobs, false, false).await;
        assert!(!batch_failed);
        assert!(results.iter().all(|(_, r)| r.is_ok()));

        let remaining: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM \"like\" WHERE creator = $1 AND subject = $2",
                &[&liker, &post_uri],
            )
            .await
            .unwrap()
            .get(0);
        assert_eq!(remaining, 0, "unlike must delete the like row");
        let agg: i64 = client
            .query_one(
                "SELECT \"likeCount\" FROM post_agg WHERE uri = $1",
                &[&post_uri],
            )
            .await
            .unwrap()
            .get(0);
        assert_eq!(agg, 0, "likeCount must return to exactly 0 after unlike");

        // A batch reply must notify the parent author via the set-based walk.
        let reply_uri = format!("at://{liker}/app.bsky.feed.post/togglereply");
        let reply_record = serde_json::json!({
            "$type": "app.bsky.feed.post",
            "text": "reply via batch",
            "reply": {
                "root": {"uri": post_uri, "cid": cid},
                "parent": {"uri": post_uri, "cid": cid},
            },
            "createdAt": ts,
        });
        let reply_jobs = vec![(
            b"k7".to_vec(),
            IndexJob {
                uri: reply_uri.clone(),
                cid: cid.clone(),
                action: WriteAction::Create,
                record: Some(reply_record),
                indexed_at: ts.clone(),
                rev: "3g".to_owned(),
            },
        )];
        let (results, batch_failed) =
            IndexerManager::process_jobs_batch(&pool, &reply_jobs, false, false).await;
        assert!(!batch_failed);
        assert!(results.iter().all(|(_, r)| r.is_ok()));

        let reply_notif: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM notification \
                 WHERE did = $1 AND \"recordUri\" = $2 AND reason = 'reply'",
                &[&author, &reply_uri],
            )
            .await
            .unwrap()
            .get(0);
        assert_eq!(
            reply_notif, 1,
            "parent author must get a reply notification"
        );

        client
            .execute("DELETE FROM post_agg WHERE uri = $1", &[&post_uri])
            .await
            .unwrap();
        for did in [liker, author] {
            cleanup_test_data(&pool, did).await;
        }
    }

    #[test]
    fn shard_live_jobs_is_deterministic_and_did_sticky() {
        use crate::types::{IndexJob, WriteAction};

        let job = |did: &str, rkey: &str| IndexJob {
            uri: format!("at://{did}/app.bsky.feed.post/{rkey}"),
            cid: "cid".to_owned(),
            action: WriteAction::Create,
            record: None,
            indexed_at: "2026-08-01T00:00:00.000Z".to_owned(),
            rev: "3a".to_owned(),
        };
        let batch: Vec<(Vec<u8>, IndexJob)> = (0u8..40)
            .map(|i| {
                let did = format!("did:plc:sharddid{}", i % 7);
                (vec![i], job(&did, &format!("r{i}")))
            })
            .collect();

        let single = IndexerManager::shard_live_jobs(batch.clone(), 1);
        assert_eq!(single.len(), 1);
        assert_eq!(single[0].len(), 40);

        let shards = IndexerManager::shard_live_jobs(batch.clone(), 4);
        assert_eq!(shards.len(), 4);
        assert_eq!(shards.iter().map(Vec::len).sum::<usize>(), 40);

        let mut did_to_shard = std::collections::HashMap::new();
        for (idx, shard) in shards.iter().enumerate() {
            for (_, j) in shard {
                let did = j.uri.split('/').nth(2).unwrap().to_owned();
                if let Some(prev) = did_to_shard.insert(did, idx) {
                    assert_eq!(prev, idx, "did split across shards");
                }
            }
        }
        assert!(
            did_to_shard
                .values()
                .collect::<std::collections::HashSet<_>>()
                .len()
                > 1,
            "test dids all hashed to one shard"
        );

        let again = IndexerManager::shard_live_jobs(batch, 4);
        for (a, b) in shards.iter().zip(again.iter()) {
            assert!(
                a.iter().map(|(k, _)| k).eq(b.iter().map(|(k, _)| k)),
                "shard assignment not deterministic"
            );
        }
    }

    #[tokio::test]
    async fn live_enqueue_wakes_waiting_drain() {
        use crate::types::{IndexJob, WriteAction};

        let (storage, _dir) = setup_test_storage();
        let storage = Arc::new(storage);
        let job = IndexJob {
            uri: "at://did:plc:wintermute-test-wakeup/app.bsky.feed.post/r1".to_owned(),
            cid: "cid".to_owned(),
            action: WriteAction::Create,
            record: None,
            indexed_at: "2026-08-01T00:00:00.000Z".to_owned(),
            rev: "3a".to_owned(),
        };

        // An enqueue from a plain thread stores a permit even with no waiter,
        // so the subsequent wait completes without consuming the timeout.
        let enqueuer = {
            let storage = Arc::clone(&storage);
            let job = job.clone();
            std::thread::spawn(move || storage.enqueue_firehose_live(&job).unwrap())
        };
        enqueuer.join().unwrap();

        let start = std::time::Instant::now();
        storage
            .wait_for_live_enqueue(std::time::Duration::from_secs(5))
            .await;
        assert!(
            start.elapsed() < std::time::Duration::from_secs(1),
            "stored notify permit did not wake the waiter"
        );

        let start = std::time::Instant::now();
        storage
            .wait_for_live_enqueue(std::time::Duration::from_millis(20))
            .await;
        assert!(start.elapsed() >= std::time::Duration::from_millis(20));
    }

    async fn shard_pass_snapshot(
        pool: &Pool,
        post_uris: &[String],
        authors: &[String],
    ) -> (Vec<i64>, Vec<i64>, Vec<i64>) {
        let client = pool.get().await.unwrap();
        let mut like_rows = Vec::new();
        let mut like_counts = Vec::new();
        for uri in post_uris {
            let rows: i64 = client
                .query_one("SELECT COUNT(*) FROM \"like\" WHERE subject = $1", &[&uri])
                .await
                .unwrap()
                .get(0);
            like_rows.push(rows);
            let agg: i64 = client
                .query_one(
                    "SELECT COALESCE((SELECT \"likeCount\" FROM post_agg WHERE uri = $1), 0)",
                    &[&uri],
                )
                .await
                .unwrap()
                .get(0);
            like_counts.push(agg);
        }
        let mut notif_counts = Vec::new();
        for did in authors {
            let n: i64 = client
                .query_one(
                    "SELECT COUNT(*) FROM notification WHERE did = $1 AND reason = 'like'",
                    &[&did],
                )
                .await
                .unwrap()
                .get(0);
            notif_counts.push(n);
        }
        (like_rows, like_counts, notif_counts)
    }

    async fn run_shard_pass(
        pool: &Pool,
        jobs: &[(Vec<u8>, crate::types::IndexJob)],
        shards: usize,
    ) {
        let shard_batches = IndexerManager::shard_live_jobs(jobs.to_vec(), shards);
        let results = IndexerManager::process_live_shards(pool, &shard_batches, false).await;
        assert_eq!(results.len(), jobs.len());
        for (_, r) in &results {
            assert!(r.is_ok(), "job failed: {r:?}");
        }
    }

    #[tokio::test]
    async fn sharded_live_drain_matches_single_shard_results() {
        use crate::types::{IndexJob, WriteAction};

        let pool = setup_test_pool();
        let authors: Vec<String> = (0..3)
            .map(|i| format!("did:plc:wintermute-test-shard-author{i}"))
            .collect();
        let likers: Vec<String> = (0..3)
            .map(|i| format!("did:plc:wintermute-test-shard-liker{i}"))
            .collect();
        let all_dids: Vec<&str> = authors
            .iter()
            .chain(likers.iter())
            .map(String::as_str)
            .collect();

        let cid = "bafyreihhl5mpvjkrhnnagen2fomozzhnhhdq2jr6cego2nzbvmwewv5rd4".to_owned();
        let ts = "2026-08-01T00:00:00.000Z".to_owned();
        let post_uris: Vec<String> = authors
            .iter()
            .enumerate()
            .map(|(i, a)| format!("at://{a}/app.bsky.feed.post/shardpost{i}"))
            .collect();
        let toggle_uri = format!("at://{}/app.bsky.feed.like/shardtoggle", likers[0]);

        let mut jobs: Vec<(Vec<u8>, IndexJob)> = Vec::new();
        let mut key = 0u8;
        let mut push = |jobs: &mut Vec<(Vec<u8>, IndexJob)>,
                        uri: String,
                        action: WriteAction,
                        record: Option<serde_json::Value>,
                        rev: &str| {
            key += 1;
            jobs.push((
                vec![key],
                IndexJob {
                    uri,
                    cid: cid.clone(),
                    action,
                    record,
                    indexed_at: ts.clone(),
                    rev: rev.to_owned(),
                },
            ));
        };
        for (i, uri) in post_uris.iter().enumerate() {
            let record = serde_json::json!({
                "$type": "app.bsky.feed.post",
                "text": format!("shard post {i}"),
                "createdAt": ts,
            });
            push(
                &mut jobs,
                uri.clone(),
                WriteAction::Create,
                Some(record),
                "3a",
            );
        }
        for (i, post_uri) in post_uris.iter().enumerate() {
            for (j, liker) in likers.iter().enumerate() {
                let record = serde_json::json!({
                    "$type": "app.bsky.feed.like",
                    "subject": {"uri": post_uri, "cid": cid},
                    "createdAt": ts,
                });
                push(
                    &mut jobs,
                    format!("at://{liker}/app.bsky.feed.like/sl{i}{j}"),
                    WriteAction::Create,
                    Some(record),
                    "3a",
                );
            }
        }
        let second_toggle_uri = format!("at://{}/app.bsky.feed.like/shardtoggle2", likers[1]);
        for (uri, subject) in [
            (&toggle_uri, &post_uris[0]),
            (&second_toggle_uri, &post_uris[1]),
        ] {
            let record = serde_json::json!({
                "$type": "app.bsky.feed.like",
                "subject": {"uri": subject, "cid": cid},
                "createdAt": ts,
            });
            push(
                &mut jobs,
                uri.clone(),
                WriteAction::Create,
                Some(record),
                "3a",
            );
            push(&mut jobs, uri.clone(), WriteAction::Delete, None, "3b");
        }

        let mut snapshots = Vec::new();
        for shards in [1usize, 3] {
            for did in &all_dids {
                cleanup_test_data(&pool, did).await;
            }
            let client = pool.get().await.unwrap();
            for did in &all_dids {
                client
                    .execute(
                        "INSERT INTO actor (did, \"indexedAt\") VALUES ($1, NOW()) \
                         ON CONFLICT (did) DO NOTHING",
                        &[&did],
                    )
                    .await
                    .unwrap();
            }
            drop(client);

            run_shard_pass(&pool, &jobs, shards).await;

            let snap = shard_pass_snapshot(&pool, &post_uris, &authors).await;
            // Exact counts: 3 likes per post; the toggled like must be gone.
            assert_eq!(snap.0, vec![3, 3, 3], "like rows wrong at shards={shards}");
            assert_eq!(snap.1, vec![3, 3, 3], "likeCount wrong at shards={shards}");
            let client = pool.get().await.unwrap();
            for uri in [&toggle_uri, &second_toggle_uri] {
                let toggle_left: i64 = client
                    .query_one("SELECT COUNT(*) FROM \"like\" WHERE uri = $1", &[uri])
                    .await
                    .unwrap()
                    .get(0);
                assert_eq!(toggle_left, 0, "toggled like survived at shards={shards}");
            }
            snapshots.push(snap);
        }
        assert_eq!(
            snapshots[0], snapshots[1],
            "sharded results diverge from single-shard results"
        );

        for did in &all_dids {
            cleanup_test_data(&pool, did).await;
        }
    }

    #[tokio::test]
    async fn via_attribution_persists_from_batch_path() {
        use crate::types::{IndexJob, WriteAction};

        let pool = setup_test_pool();
        let author = "did:plc:wintermute-test-via-author";
        let actor = "did:plc:wintermute-test-via-actor";
        for did in [author, actor] {
            cleanup_test_data(&pool, did).await;
        }
        let client = pool.get().await.unwrap();
        for did in [author, actor] {
            client
                .execute(
                    "INSERT INTO actor (did, \"indexedAt\") VALUES ($1, NOW()) \
                     ON CONFLICT (did) DO NOTHING",
                    &[&did],
                )
                .await
                .unwrap();
        }
        drop(client);

        let cid = "bafyreihhl5mpvjkrhnnagen2fomozzhnhhdq2jr6cego2nzbvmwewv5rd4";
        let ts = "2026-08-03T00:00:00.000Z";
        let post_uri = format!("at://{author}/app.bsky.feed.post/viapost");
        let via_uri = format!("at://{author}/app.bsky.feed.repost/viasource");
        let subject = serde_json::json!({"uri": post_uri, "cid": cid});
        let via = serde_json::json!({"uri": via_uri, "cid": cid});

        let job = |coll: &str, rkey: &str, record: serde_json::Value| IndexJob {
            uri: format!("at://{actor}/{coll}/{rkey}"),
            cid: cid.to_owned(),
            action: WriteAction::Create,
            record: Some(record),
            indexed_at: ts.to_owned(),
            rev: "3a".to_owned(),
        };
        let jobs = vec![
            (
                b"v1".to_vec(),
                job(
                    "app.bsky.feed.like",
                    "withvia",
                    serde_json::json!({"$type": "app.bsky.feed.like", "subject": subject, "via": via, "createdAt": ts}),
                ),
            ),
            (
                b"v2".to_vec(),
                job(
                    "app.bsky.feed.like",
                    "novia",
                    serde_json::json!({"$type": "app.bsky.feed.like", "subject": {"uri": format!("at://{author}/app.bsky.feed.post/viapost2"), "cid": cid}, "createdAt": ts}),
                ),
            ),
            (
                b"v3".to_vec(),
                job(
                    "app.bsky.feed.repost",
                    "withvia",
                    serde_json::json!({"$type": "app.bsky.feed.repost", "subject": subject, "via": via, "createdAt": ts}),
                ),
            ),
            (
                b"v4".to_vec(),
                job(
                    "app.bsky.graph.follow",
                    "withvia",
                    serde_json::json!({"$type": "app.bsky.graph.follow", "subject": author, "via": via, "createdAt": ts}),
                ),
            ),
        ];
        let (results, batch_failed) =
            IndexerManager::process_jobs_batch(&pool, &jobs, false, false).await;
        assert!(!batch_failed);
        for (_, r) in &results {
            assert!(r.is_ok(), "job failed: {r:?}");
        }

        let client = pool.get().await.unwrap();
        for (table, coll, rkey, expect_via) in [
            ("\"like\"", "app.bsky.feed.like", "withvia", true),
            ("\"like\"", "app.bsky.feed.like", "novia", false),
            ("repost", "app.bsky.feed.repost", "withvia", true),
            ("follow", "app.bsky.graph.follow", "withvia", true),
        ] {
            let uri = format!("at://{actor}/{coll}/{rkey}");
            let row = client
                .query_one(
                    &format!("SELECT via, \"viaCid\" FROM {table} WHERE uri = $1"),
                    &[&uri],
                )
                .await
                .unwrap();
            let got_via: Option<String> = row.get(0);
            let got_via_cid: Option<String> = row.get(1);
            if expect_via {
                assert_eq!(got_via.as_deref(), Some(via_uri.as_str()), "{uri}");
                assert_eq!(got_via_cid.as_deref(), Some(cid), "{uri}");
            } else {
                assert!(got_via.is_none() && got_via_cid.is_none(), "{uri}");
            }
        }
        drop(client);

        for did in [author, actor] {
            cleanup_test_data(&pool, did).await;
        }
    }

    #[tokio::test]
    async fn skip_boilerplate_omits_record_rows_with_exact_effects() {
        use crate::types::{IndexJob, WriteAction};

        let pool = setup_test_pool();
        let author = "did:plc:wintermute-test-skip-author";
        let liker = "did:plc:wintermute-test-skip-liker";
        for did in [author, liker] {
            cleanup_test_data(&pool, did).await;
        }
        let client = pool.get().await.unwrap();
        for did in [author, liker] {
            client
                .execute(
                    "INSERT INTO actor (did, \"indexedAt\") VALUES ($1, NOW()) \
                     ON CONFLICT (did) DO NOTHING",
                    &[&did],
                )
                .await
                .unwrap();
        }
        drop(client);

        let cid = "bafyreihhl5mpvjkrhnnagen2fomozzhnhhdq2jr6cego2nzbvmwewv5rd4";
        let ts = "2026-08-04T00:00:00.000Z";
        let post_uri = format!("at://{author}/app.bsky.feed.post/skippost");
        let like_uri = format!("at://{liker}/app.bsky.feed.like/skiplike");

        let jobs = vec![
            (
                b"s1".to_vec(),
                IndexJob {
                    uri: post_uri.clone(),
                    cid: cid.to_owned(),
                    action: WriteAction::Create,
                    record: Some(serde_json::json!({
                        "$type": "app.bsky.feed.post", "text": "skip test", "createdAt": ts
                    })),
                    indexed_at: ts.to_owned(),
                    rev: "3a".to_owned(),
                },
            ),
            (
                b"s2".to_vec(),
                IndexJob {
                    uri: like_uri.clone(),
                    cid: cid.to_owned(),
                    action: WriteAction::Create,
                    record: Some(serde_json::json!({
                        "$type": "app.bsky.feed.like",
                        "subject": {"uri": post_uri, "cid": cid},
                        "createdAt": ts
                    })),
                    indexed_at: ts.to_owned(),
                    rev: "3a".to_owned(),
                },
            ),
        ];

        let (results, batch_failed) =
            IndexerManager::process_jobs_batch(&pool, &jobs, false, true).await;
        assert!(!batch_failed);
        for (_, r) in &results {
            assert!(r.is_ok(), "job failed: {r:?}");
        }

        let client = pool.get().await.unwrap();
        let record_rows: i64 = client
            .query_one(
                "SELECT count(*) FROM record WHERE uri = ANY($1)",
                &[&vec![post_uri.clone(), like_uri.clone()]],
            )
            .await
            .unwrap()
            .get(0);
        assert_eq!(record_rows, 1, "only the post belongs in the record table");
        let post_record: i64 = client
            .query_one("SELECT count(*) FROM record WHERE uri = $1", &[&post_uri])
            .await
            .unwrap()
            .get(0);
        assert_eq!(post_record, 1);
        let like_row: i64 = client
            .query_one("SELECT count(*) FROM \"like\" WHERE uri = $1", &[&like_uri])
            .await
            .unwrap()
            .get(0);
        assert_eq!(like_row, 1, "typed like row must exist without record row");
        let like_count: i64 = client
            .query_one(
                "SELECT COALESCE((SELECT \"likeCount\" FROM post_agg WHERE uri = $1), 0)",
                &[&post_uri],
            )
            .await
            .unwrap()
            .get(0);
        assert_eq!(like_count, 1);
        let notif: i64 = client
            .query_one(
                "SELECT count(*) FROM notification WHERE did = $1 AND \"recordUri\" = $2",
                &[&author, &like_uri],
            )
            .await
            .unwrap()
            .get(0);
        assert_eq!(notif, 1, "like notification must exist");
        drop(client);

        // Replay: idempotent without the record gate.
        let (replay_results, replay_failed) =
            IndexerManager::process_jobs_batch(&pool, &jobs, false, true).await;
        assert!(!replay_failed);
        for (_, r) in &replay_results {
            assert!(r.is_ok());
        }
        let client = pool.get().await.unwrap();
        let like_count: i64 = client
            .query_one(
                "SELECT COALESCE((SELECT \"likeCount\" FROM post_agg WHERE uri = $1), 0)",
                &[&post_uri],
            )
            .await
            .unwrap()
            .get(0);
        assert_eq!(like_count, 1, "replay must not double count");
        drop(client);

        // Delete the like with the flag on: typed row gone, count exact.
        let delete_jobs = vec![(
            b"s3".to_vec(),
            IndexJob {
                uri: like_uri.clone(),
                cid: String::new(),
                action: WriteAction::Delete,
                record: None,
                indexed_at: ts.to_owned(),
                rev: "3b".to_owned(),
            },
        )];
        let (del_results, del_failed) =
            IndexerManager::process_jobs_batch(&pool, &delete_jobs, false, true).await;
        assert!(!del_failed);
        for (_, r) in &del_results {
            assert!(r.is_ok());
        }
        let client = pool.get().await.unwrap();
        let like_row: i64 = client
            .query_one("SELECT count(*) FROM \"like\" WHERE uri = $1", &[&like_uri])
            .await
            .unwrap()
            .get(0);
        assert_eq!(like_row, 0, "typed like row must be deleted");
        let like_count: i64 = client
            .query_one(
                "SELECT COALESCE((SELECT \"likeCount\" FROM post_agg WHERE uri = $1), 0)",
                &[&post_uri],
            )
            .await
            .unwrap()
            .get(0);
        assert_eq!(like_count, 0, "likeCount must return to zero");
        drop(client);

        for did in [author, liker] {
            cleanup_test_data(&pool, did).await;
        }
    }
}
