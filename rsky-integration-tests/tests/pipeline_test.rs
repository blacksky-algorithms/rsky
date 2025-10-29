use anyhow::Result;
use redis::AsyncCommands;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

/// Integration test for the complete data pipeline:
/// Ingester -> Redis -> Backfiller -> Redis -> Indexer -> PostgreSQL
#[tokio::test]
#[ignore] // Run with: cargo test -p rsky-integration-tests -- --ignored --nocapture
async fn test_full_pipeline() -> Result<()> {
    // Initialize tracing for test visibility
    tracing_subscriber::fmt()
        .with_env_filter("info,rsky_ingester=debug,rsky_backfiller=debug,rsky_indexer=debug")
        .with_test_writer()
        .try_init()
        .ok();

    tracing::info!("Starting integration test");

    // Setup test infrastructure
    let test_ctx = TestContext::new().await?;

    // Step 1: Start ingester (firehose -> Redis)
    tracing::info!("Step 1: Starting ingester");
    let ingester_handle = test_ctx.start_ingester().await?;

    // Wait for some events to be ingested
    sleep(Duration::from_secs(10)).await;

    // Step 2: Verify firehose_live stream has events
    tracing::info!("Step 2: Verifying firehose_live stream");
    let live_count = test_ctx.check_stream_length("firehose_live").await?;
    assert!(live_count > 0, "Expected events in firehose_live stream");
    tracing::info!("Found {} events in firehose_live", live_count);

    // Step 3: Test backfill
    tracing::info!("Step 3: Starting backfill test");

    // Queue a known good DID for backfill
    let test_did = "did:plc:w4xbfzo7kqfes5zb7r6qv3rw";
    test_ctx.queue_backfill(test_did).await?;

    // Start backfiller and measure time
    let backfill_start = std::time::Instant::now();
    let backfiller_handle = test_ctx.start_backfiller().await?;

    // Wait for backfill to process
    sleep(Duration::from_secs(5)).await;

    // Verify backfill stream has events
    let backfill_count = test_ctx.check_stream_length("firehose_backfill").await?;
    let backfill_duration = backfill_start.elapsed();
    tracing::info!(
        "Backfill completed: {} events in {:?} ({:.0} events/sec)",
        backfill_count,
        backfill_duration,
        backfill_count as f64 / backfill_duration.as_secs_f64()
    );

    // Verify we got a reasonable number of events from backfill
    assert!(backfill_count > 1000, "Expected at least 1000 events from backfilled repo, got {}", backfill_count);

    // Step 4: Start indexer (Redis streams -> PostgreSQL)
    tracing::info!("Step 4: Starting indexer");
    let indexer_handle = test_ctx.start_indexer().await?;

    // Wait longer for indexing to happen (20 seconds to capture more events)
    tracing::info!("Waiting 20 seconds for events to be indexed...");
    sleep(Duration::from_secs(20)).await;

    // Step 5: Verify data in PostgreSQL
    tracing::info!("Step 5: Verifying PostgreSQL data");
    let postgres_stats = test_ctx.check_postgres().await?;
    tracing::info!("PostgreSQL stats: {:?}", postgres_stats);

    // Step 5a: Verify backfilled DID was indexed
    let backfilled_records = test_ctx.count_records_for_did(test_did).await?;
    tracing::info!(
        "Backfilled DID {} has {} records indexed in PostgreSQL",
        test_did,
        backfilled_records
    );
    assert!(
        backfilled_records > 100,
        "Expected at least 100 records from backfilled DID, got {}",
        backfilled_records
    );

    // Assertions - at minimum we should have actor_sync data from repo commits
    assert!(
        postgres_stats.get("actor_sync").unwrap_or(&0) > &0,
        "Expected actor_sync entries (commit tracking)"
    );

    // Check for records (may or may not exist depending on what events occurred)
    let record_count = postgres_stats.get("record").unwrap_or(&0);
    let post_count = postgres_stats.get("post").unwrap_or(&0);
    let like_count = postgres_stats.get("like").unwrap_or(&0);
    let repost_count = postgres_stats.get("repost").unwrap_or(&0);
    let follow_count = postgres_stats.get("follow").unwrap_or(&0);

    tracing::info!(
        "Pipeline verification: {} records, {} posts, {} likes, {} reposts, {} follows, {} commits tracked",
        record_count,
        post_count,
        like_count,
        repost_count,
        follow_count,
        postgres_stats.get("actor_sync").unwrap_or(&0)
    );

    if *record_count > 0 {
        // Step 6: If we have records, verify we can read them
        tracing::info!("Step 6: Verifying sample records");
        let sample_records = test_ctx.get_sample_records(5).await?;

        for record in &sample_records {
            tracing::info!(
                "Sample record: uri={}, collection={}",
                record.uri,
                record.collection
            );
        }
    } else {
        tracing::warn!("No records indexed in this test run (only commit events). This is normal for short test runs.");
        tracing::info!("The pipeline is working correctly - commits are being tracked in actor_sync table.");
    }

    // Cleanup
    tracing::info!("Test complete, cleaning up");

    // Stop ingester first to prevent new events
    ingester_handle.abort();
    sleep(Duration::from_millis(500)).await;

    // Stop backfiller
    backfiller_handle.abort();
    sleep(Duration::from_millis(500)).await;

    // Give indexer significant time to drain and ACK all pending events
    // With retry logic, each message can take up to 70ms (10+20+40) to ACK
    tracing::info!("Allowing indexer to drain and ACK all pending events...");
    sleep(Duration::from_secs(5)).await;

    // Stop indexer - by now all events should be ACKed
    indexer_handle.abort();

    // Brief wait for final cleanup
    sleep(Duration::from_secs(1)).await;

    // Attempt cleanup - ignore errors from port exhaustion during shutdown
    if let Err(e) = test_ctx.cleanup().await {
        tracing::warn!("Cleanup error (expected during rapid shutdown): {:?}", e);
    }

    Ok(())
}

/// Test context that manages test infrastructure
struct TestContext {
    redis_url: String,
    database_url: String,
    redis_client: redis::Client,
    pg_pool: deadpool_postgres::Pool,
}

impl TestContext {
    async fn new() -> Result<Self> {
        // Use environment variables or defaults for test infrastructure
        let redis_url = std::env::var("TEST_REDIS_URL")
            .unwrap_or_else(|_| "redis://localhost:6379".to_string());
        let database_url = std::env::var("TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/bsky_test".to_string());

        tracing::info!("Connecting to Redis: {}", redis_url);
        tracing::info!("Connecting to PostgreSQL: {}", database_url);

        // Connect to Redis
        let redis_client = redis::Client::open(redis_url.as_str())?;
        let mut conn = redis_client.get_multiplexed_async_connection().await?;

        // Clear test streams and cursors
        for stream in &["firehose_live", "firehose_backfill", "repo_backfill", "label_live"] {
            let _: Result<(), redis::RedisError> = redis::cmd("DEL")
                .arg(stream)
                .query_async(&mut conn)
                .await;
        }

        // Clear cursor keys to start from the beginning (cursor 0)
        let _: Result<(), redis::RedisError> = redis::cmd("DEL")
            .arg("firehose_live:cursor:bsky.network")
            .query_async(&mut conn)
            .await;

        // Connect to PostgreSQL
        let mut pg_config = deadpool_postgres::Config::new();
        pg_config.url = Some(database_url.clone());
        let pg_pool = pg_config
            .create_pool(Some(deadpool_postgres::Runtime::Tokio1), tokio_postgres::NoTls)?;

        // Setup database schema (simplified for test)
        let client = pg_pool.get().await?;
        client
            .batch_execute(
                r#"
                CREATE TABLE IF NOT EXISTS record (
                    uri TEXT PRIMARY KEY,
                    cid TEXT NOT NULL,
                    did TEXT NOT NULL,
                    json JSONB NOT NULL,
                    indexed_at TIMESTAMPTZ NOT NULL
                );

                CREATE TABLE IF NOT EXISTS post (
                    uri TEXT PRIMARY KEY,
                    cid TEXT NOT NULL,
                    creator TEXT NOT NULL,
                    text TEXT NOT NULL,
                    created_at TIMESTAMPTZ NOT NULL,
                    indexed_at TIMESTAMPTZ NOT NULL
                );

                CREATE TABLE IF NOT EXISTS "like" (
                    uri TEXT PRIMARY KEY,
                    cid TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS repost (
                    uri TEXT PRIMARY KEY,
                    cid TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS follow (
                    uri TEXT PRIMARY KEY,
                    cid TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS thread_gate (
                    uri TEXT PRIMARY KEY,
                    cid TEXT NOT NULL,
                    post_uri TEXT,
                    created_at TIMESTAMPTZ NOT NULL,
                    indexed_at TIMESTAMPTZ NOT NULL
                );

                CREATE TABLE IF NOT EXISTS post_gate (
                    uri TEXT PRIMARY KEY,
                    cid TEXT NOT NULL,
                    post_uri TEXT,
                    created_at TIMESTAMPTZ NOT NULL,
                    indexed_at TIMESTAMPTZ NOT NULL
                );

                CREATE TABLE IF NOT EXISTS block (
                    uri TEXT PRIMARY KEY,
                    cid TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS list_item (
                    uri TEXT PRIMARY KEY,
                    cid TEXT NOT NULL,
                    creator TEXT NOT NULL,
                    subject_did TEXT,
                    list_uri TEXT,
                    created_at TIMESTAMPTZ NOT NULL,
                    indexed_at TIMESTAMPTZ NOT NULL
                );

                CREATE TABLE IF NOT EXISTS profile (
                    uri TEXT PRIMARY KEY
                );

                CREATE TABLE IF NOT EXISTS feed_generator (
                    uri TEXT PRIMARY KEY,
                    cid TEXT NOT NULL,
                    creator TEXT NOT NULL,
                    feed_did TEXT,
                    display_name TEXT,
                    description TEXT,
                    created_at TIMESTAMPTZ NOT NULL,
                    indexed_at TIMESTAMPTZ NOT NULL
                );

                CREATE TABLE IF NOT EXISTS list (
                    uri TEXT PRIMARY KEY,
                    cid TEXT NOT NULL,
                    creator TEXT NOT NULL,
                    name TEXT NOT NULL,
                    purpose TEXT,
                    description TEXT,
                    created_at TIMESTAMPTZ NOT NULL,
                    indexed_at TIMESTAMPTZ NOT NULL
                );

                CREATE TABLE IF NOT EXISTS list_block (
                    uri TEXT PRIMARY KEY,
                    cid TEXT NOT NULL,
                    creator TEXT NOT NULL,
                    subject_uri TEXT,
                    created_at TIMESTAMPTZ NOT NULL,
                    indexed_at TIMESTAMPTZ NOT NULL
                );

                CREATE TABLE IF NOT EXISTS chat_declaration (
                    uri TEXT PRIMARY KEY,
                    cid TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS starter_pack (
                    uri TEXT PRIMARY KEY,
                    cid TEXT NOT NULL,
                    creator TEXT NOT NULL,
                    name TEXT NOT NULL,
                    description TEXT,
                    list_uri TEXT,
                    created_at TIMESTAMPTZ NOT NULL,
                    indexed_at TIMESTAMPTZ NOT NULL
                );

                CREATE TABLE IF NOT EXISTS actor_sync (
                    did TEXT PRIMARY KEY,
                    commit_cid TEXT NOT NULL,
                    repo_rev TEXT NOT NULL,
                    last_seen TIMESTAMPTZ NOT NULL
                );

                CREATE TABLE IF NOT EXISTS actor (
                    did TEXT PRIMARY KEY,
                    handle TEXT UNIQUE,
                    indexed_at TIMESTAMPTZ,
                    upstream_status TEXT
                );

                TRUNCATE TABLE record, post, "like", repost, follow, thread_gate, post_gate, block, list_item, profile, feed_generator, list, list_block, chat_declaration, starter_pack, actor_sync, actor;
                "#,
            )
            .await?;

        Ok(Self {
            redis_url,
            database_url,
            redis_client,
            pg_pool,
        })
    }

    /// Start the ingester
    async fn start_ingester(&self) -> Result<tokio::task::JoinHandle<()>> {
        let redis_url = self.redis_url.clone();

        let handle = tokio::spawn(async move {
            let config = rsky_ingester::IngesterConfig {
                redis_url,
                relay_hosts: vec!["bsky.network".to_string()],
                labeler_hosts: vec![],
                high_water_mark: 100_000,
                batch_size: 500,
                batch_timeout_ms: 1000,
            };

            let ingester = rsky_ingester::firehose::FirehoseIngester::new(config)
                .expect("Failed to create ingester");

            let hostname = "bsky.network".to_string();
            if let Err(e) = ingester.run(hostname).await {
                tracing::error!("Ingester error: {:?}", e);
            }
        });

        // Give ingester time to connect
        sleep(Duration::from_secs(2)).await;

        Ok(handle)
    }

    /// Queue a repo for backfill
    async fn queue_backfill(&self, did: &str) -> Result<()> {
        let mut conn = self.redis_client.get_multiplexed_async_connection().await?;

        let backfill_event = rsky_backfiller::BackfillEvent {
            did: did.to_string(),
            host: "https://blacksky.app".to_string(),
            rev: "test".to_string(),
            status: None,
            active: true,
        };

        let json = serde_json::to_string(&backfill_event)?;

        let _: String = redis::cmd("XADD")
            .arg("repo_backfill")
            .arg("*")
            .arg("repo")
            .arg(json)
            .query_async(&mut conn)
            .await?;

        Ok(())
    }

    /// Start the backfiller
    async fn start_backfiller(&self) -> Result<tokio::task::JoinHandle<()>> {
        let config = rsky_backfiller::BackfillerConfig {
            redis_url: self.redis_url.clone(),
            stream_in: "repo_backfill".to_string(),
            stream_out: "firehose_backfill".to_string(),
            consumer_group: "test_backfill_group".to_string(),
            consumer_name: "test_backfiller_1".to_string(),
            concurrency: 1,
            high_water_mark: 100_000,
        };

        let handle = tokio::spawn(async move {
            let backfiller = rsky_backfiller::repo_backfiller::RepoBackfiller::new(config)
                .expect("Failed to create backfiller");

            if let Err(e) = backfiller.run().await {
                tracing::error!("Backfiller error: {:?}", e);
            }
        });

        sleep(Duration::from_secs(2)).await;

        Ok(handle)
    }

    /// Start the indexer
    async fn start_indexer(&self) -> Result<tokio::task::JoinHandle<()>> {
        let config = rsky_indexer::IndexerConfig {
            redis_url: self.redis_url.clone(),
            database_url: self.database_url.clone(),
            streams: vec!["firehose_live".to_string(), "firehose_backfill".to_string()],
            consumer_group: "test_indexer_group".to_string(),
            consumer_name: "test_indexer_1".to_string(),
            concurrency: 5,
            batch_size: 100,
        };

        let handle = tokio::spawn(async move {
            // Create indexing service
            let pg_config = deadpool_postgres::Config {
                url: Some(config.database_url.clone()),
                ..Default::default()
            };
            let pool = pg_config
                .create_pool(Some(deadpool_postgres::Runtime::Tokio1), tokio_postgres::NoTls)
                .expect("Failed to create pool");

            // Create IdResolver for handle resolution
            use tokio::sync::Mutex;
            let resolver_opts = rsky_identity::types::IdentityResolverOpts {
                timeout: None,
                plc_url: None,
                did_cache: None,
                backup_nameservers: None,
            };
            let id_resolver = Arc::new(Mutex::new(rsky_identity::IdResolver::new(resolver_opts)));

            let indexing_service = Arc::new(
                rsky_indexer::indexing::IndexingService::new_with_resolver(pool, Some(id_resolver))
            );

            // Start stream indexers
            for stream in &config.streams {
                let mut stream_config = config.clone();
                stream_config.streams = vec![stream.clone()];

                let indexer = rsky_indexer::stream_indexer::StreamIndexer::new(
                    stream_config,
                    indexing_service.clone(),
                )
                .await
                .expect("Failed to create stream indexer");

                tokio::spawn(async move {
                    if let Err(e) = indexer.run().await {
                        tracing::error!("Stream indexer error: {:?}", e);
                    }
                });
            }

            // Keep running
            loop {
                sleep(Duration::from_secs(1)).await;
            }
        });

        sleep(Duration::from_secs(2)).await;

        Ok(handle)
    }

    /// Check stream length
    async fn check_stream_length(&self, stream: &str) -> Result<usize> {
        let mut conn = self.redis_client.get_multiplexed_async_connection().await?;
        let len: usize = conn.xlen(stream).await?;
        Ok(len)
    }

    /// Check PostgreSQL for indexed data
    async fn check_postgres(&self) -> Result<HashMap<String, i64>> {
        let client = self.pg_pool.get().await?;

        let mut stats = HashMap::new();

        // Count records
        let row = client
            .query_one("SELECT COUNT(*) FROM record", &[])
            .await?;
        stats.insert("record".to_string(), row.get(0));

        // Count posts
        let row = client.query_one("SELECT COUNT(*) FROM post", &[]).await?;
        stats.insert("post".to_string(), row.get(0));

        // Count likes
        let row = client.query_one(r#"SELECT COUNT(*) FROM "like""#, &[]).await?;
        stats.insert("like".to_string(), row.get(0));

        // Count reposts
        let row = client.query_one("SELECT COUNT(*) FROM repost", &[]).await?;
        stats.insert("repost".to_string(), row.get(0));

        // Count follows
        let row = client.query_one("SELECT COUNT(*) FROM follow", &[]).await?;
        stats.insert("follow".to_string(), row.get(0));

        // Count actor_sync
        let row = client
            .query_one("SELECT COUNT(*) FROM actor_sync", &[])
            .await?;
        stats.insert("actor_sync".to_string(), row.get(0));

        // Count starter packs
        let row = client
            .query_one("SELECT COUNT(*) FROM starter_pack", &[])
            .await?;
        stats.insert("starter_pack".to_string(), row.get(0));

        Ok(stats)
    }

    /// Count records for a specific DID across all tables
    async fn count_records_for_did(&self, did: &str) -> Result<i64> {
        let client = self.pg_pool.get().await?;

        // Count from specific tables since they have creator/did columns
        // URIs are in format: at://did:plc:xxx/collection/rkey
        let uri_pattern = format!("at://{}/%", did);

        let mut total = 0i64;

        // Count posts
        if let Ok(row) = client
            .query_one("SELECT COUNT(*) FROM post WHERE uri LIKE $1", &[&uri_pattern])
            .await
        {
            total += row.get::<_, i64>(0);
        }

        // Count likes
        if let Ok(row) = client
            .query_one("SELECT COUNT(*) FROM \"like\" WHERE uri LIKE $1", &[&uri_pattern])
            .await
        {
            total += row.get::<_, i64>(0);
        }

        // Count follows
        if let Ok(row) = client
            .query_one("SELECT COUNT(*) FROM follow WHERE uri LIKE $1", &[&uri_pattern])
            .await
        {
            total += row.get::<_, i64>(0);
        }

        // Count reposts
        if let Ok(row) = client
            .query_one("SELECT COUNT(*) FROM repost WHERE uri LIKE $1", &[&uri_pattern])
            .await
        {
            total += row.get::<_, i64>(0);
        }

        // Also count in the generic record table
        if let Ok(row) = client
            .query_one("SELECT COUNT(*) FROM record WHERE uri LIKE $1", &[&uri_pattern])
            .await
        {
            total += row.get::<_, i64>(0);
        }

        // Count starter packs
        if let Ok(row) = client
            .query_one("SELECT COUNT(*) FROM starter_pack WHERE uri LIKE $1", &[&uri_pattern])
            .await
        {
            total += row.get::<_, i64>(0);
        }

        Ok(total)
    }

    /// Get sample records from PostgreSQL
    async fn get_sample_records(&self, limit: i64) -> Result<Vec<SampleRecord>> {
        let client = self.pg_pool.get().await?;

        let rows = client
            .query(
                "SELECT uri, did, indexed_at FROM record ORDER BY indexed_at DESC LIMIT $1",
                &[&limit],
            )
            .await?;

        let mut records = Vec::new();
        for row in rows {
            let uri: String = row.get(0);
            let parts: Vec<&str> = uri.split('/').collect();
            let collection = if parts.len() >= 3 {
                parts[parts.len() - 2].to_string()
            } else {
                "unknown".to_string()
            };

            records.push(SampleRecord {
                uri,
                did: row.get(1),
                collection,
                indexed_at: row.get(2),
            });
        }

        Ok(records)
    }

    /// Cleanup test resources
    async fn cleanup(&self) -> Result<()> {
        // Drop test tables
        let client = self.pg_pool.get().await?;
        client
            .batch_execute(r#"DROP TABLE IF EXISTS record, post, "like", repost, follow, thread_gate, post_gate, block, list_item, profile, feed_generator, list, list_block, chat_declaration, actor_sync, actor CASCADE;"#)
            .await?;

        // Clear Redis streams
        let mut conn = self.redis_client.get_multiplexed_async_connection().await?;
        for stream in &["firehose_live", "firehose_backfill", "repo_backfill", "label_live"] {
            let _: Result<(), redis::RedisError> = redis::cmd("DEL")
                .arg(stream)
                .query_async(&mut conn)
                .await;
        }

        Ok(())
    }
}

#[derive(Debug)]
#[allow(dead_code)]
struct SampleRecord {
    uri: String,
    did: String,
    collection: String,
    indexed_at: chrono::DateTime<chrono::Utc>,
}
