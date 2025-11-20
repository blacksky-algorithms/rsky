//! Indexer integration tests
//!
//! These tests verify end-to-end indexing from backfill through postgres writes.
//! Each test uses an isolated fjall database via TempDir and shares a PostgreSQL database.
//!
//! Run with: `DATABASE_URL=... cargo test --lib indexer::tests`

#[cfg(test)]
mod indexer_tests {
    use crate::backfiller::BackfillerManager;
    use crate::indexer::IndexerManager;
    use crate::storage::Storage;
    use crate::types::{BackfillJob, WriteAction};
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

        let queue_len = storage.index_queue_len().unwrap();
        tracing::info!("backfill complete, {queue_len} records enqueued for indexing");
        assert!(
            queue_len > 5000,
            "expected more than 5000 records to be enqueued, found {queue_len}"
        );

        let indexer = IndexerManager::new(
            Arc::new(storage),
            std::env::var("DATABASE_URL").unwrap_or_else(|_| {
                "postgresql://postgres:postgres@localhost:5432/bsky_test".to_owned()
            }),
        )
        .unwrap();

        let mut processed = 0;
        let batch_size = 100;
        let mut consecutive_empty = 0;

        while consecutive_empty < 3 {
            let mut batch_processed = 0;

            for _ in 0..batch_size {
                match indexer.storage.dequeue_index() {
                    Ok(Some((key, index_job))) => {
                        let result = IndexerManager::process_job(&indexer.pool, &index_job).await;

                        match result {
                            Ok(()) => {
                                drop(indexer.storage.remove_index(&key));
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

        let indexer = IndexerManager::new(
            Arc::new(storage),
            std::env::var("DATABASE_URL").unwrap_or_else(|_| {
                "postgresql://postgres:postgres@localhost:5432/bsky_test".to_owned()
            }),
        )
        .unwrap();

        let batch_size = 100;

        loop {
            let mut batch_processed = 0;

            for _ in 0..batch_size {
                match indexer.storage.dequeue_index() {
                    Ok(Some((key, index_job))) => {
                        let result = IndexerManager::process_job(&indexer.pool, &index_job).await;

                        if result.is_ok() {
                            drop(indexer.storage.remove_index(&key));
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
}
