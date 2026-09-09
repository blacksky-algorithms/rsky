use std::sync::LazyLock;
use std::time::Duration;

pub const CAPACITY_FIREHOSE: usize = 1 << 16;
pub const CAPACITY_BACKFILL: usize = 1 << 14;
pub const CAPACITY_INDEX: usize = 1 << 14;

pub const WORKERS_INGESTER: usize = 4;

// Fjall storage config - tunable via environment variables
// On high-memory servers (200GB+ RAM), these should be increased significantly
// Rule of thumb: CACHE_SIZE = 20-25% of RAM, WRITE_BUFFER_SIZE = 1-2% of RAM
pub static CACHE_SIZE: LazyLock<u64> = LazyLock::new(|| {
    std::env::var("FJALL_CACHE_SIZE_GB")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map_or(32 * 1024 * 1024 * 1024, |gb| gb * 1024 * 1024 * 1024) // Default: 32GB
});

pub static WRITE_BUFFER_SIZE: LazyLock<u64> = LazyLock::new(|| {
    std::env::var("FJALL_WRITE_BUFFER_SIZE_GB")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map_or(2 * 1024 * 1024 * 1024, |gb| gb * 1024 * 1024 * 1024) // Default: 2GB
});

pub const FSYNC_MS: Option<u16> = Some(1000);
pub const MEMTABLE_SIZE: u32 = 256 * 1024 * 1024; // 256MB (up from 64MB)
pub const BLOCK_SIZE: u32 = 64 * 1024;

/// Upper bound on the `repo_backfill` queue, in entries. `0` disables the bound.
///
/// The relay enumerator is a producer with no consumer whenever
/// `BACKFILLER_WORKERS=0`, which is the recommended setting where repo backfill is
/// handled elsewhere. Unbounded, the queue reaches millions of entries within hours;
/// its Fjall partition grows to gigabytes, and the LSM read and compaction paths
/// allocate block buffers in proportion. The result reads as a memory leak -- resident
/// memory climbs steadily and only a restart reclaims it -- when it is really a queue
/// nothing drains.
pub static REPO_BACKFILL_MAX_QUEUE: LazyLock<usize> = LazyLock::new(|| {
    std::env::var("REPO_BACKFILL_MAX_QUEUE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(250_000)
});

/// Whether the enumerator may enqueue another repo.
///
/// Separate from the enumeration loop so the bound is testable without a relay or a
/// storage engine.
#[must_use]
pub const fn repo_backfill_has_room(queue_len: usize, max_queue: usize) -> bool {
    max_queue == 0 || queue_len < max_queue
}

pub const FIREHOSE_PING_INTERVAL: Duration = Duration::from_secs(30);

// Cursor save interval - like indigo/tap's cursorSaveInterval
// Saves cursor to Fjall/Postgres periodically instead of every event
// This prevents Fjall poisoning from high-frequency writes
pub static CURSOR_SAVE_INTERVAL: LazyLock<Duration> = LazyLock::new(|| {
    let secs = std::env::var("CURSOR_SAVE_INTERVAL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5); // Default: save cursor every 5 seconds
    Duration::from_secs(secs)
});

// Fresh-subscription start cursor: unset = live; 0 = oldest (full backfill window); N = from seq N.
pub static FIREHOSE_INITIAL_CURSOR: LazyLock<Option<i64>> = LazyLock::new(|| {
    std::env::var("FIREHOSE_INITIAL_CURSOR")
        .ok()
        .and_then(|s| s.trim().parse().ok())
});

// Indexer config - tunable via environment variables
pub static WORKERS_INDEXER: LazyLock<usize> = LazyLock::new(|| {
    std::env::var("INDEXER_WORKERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(16) // Default: 16 concurrent index workers
});

pub static INDEXER_BATCH_SIZE: LazyLock<usize> = LazyLock::new(|| {
    std::env::var("INDEXER_BATCH_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000) // Default: 1000 records per batch
});

// Number of parallel batch processors for backfill indexing
// Each worker dequeues and processes batches independently
// Should be tuned based on DB pool size (e.g., pool_size / 2)
pub static INDEXER_BATCH_WORKERS: LazyLock<usize> = LazyLock::new(|| {
    std::env::var("INDEXER_BATCH_WORKERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4) // Default: 4 parallel batch workers
});

// Maximum concurrent indexer tasks for backfill processing
// Higher values can increase throughput but also increase DB connection contention
// Should be tuned based on DB pool size and available resources
pub static INDEXER_MAX_CONCURRENT: LazyLock<usize> = LazyLock::new(|| {
    std::env::var("INDEXER_MAX_CONCURRENT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(200) // Default: 200 concurrent tasks (increased from 50)
});

// Handle resolution: revalidate handles after this duration
pub const HANDLE_REINDEX_INTERVAL_VALID: Duration = Duration::from_secs(24 * 60 * 60); // 1 day
pub const HANDLE_REINDEX_INTERVAL_INVALID: Duration = Duration::from_secs(60 * 60); // 1 hour
pub const IDENTITY_RESOLVER_TIMEOUT: Duration = Duration::from_secs(3);

// Handle resolution concurrency - process multiple handles in parallel
pub static HANDLE_RESOLUTION_CONCURRENCY: LazyLock<usize> = LazyLock::new(|| {
    std::env::var("HANDLE_RESOLUTION_CONCURRENCY")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(50) // Default: 50 concurrent handle resolutions
});

pub static HANDLE_RESOLUTION_BATCH_SIZE: LazyLock<usize> = LazyLock::new(|| {
    std::env::var("HANDLE_RESOLUTION_BATCH_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(500) // Default: 500 actors per batch
});

// Priority window for recently-indexed actors (resolve new actors faster)
pub const HANDLE_PRIORITY_WINDOW: Duration = Duration::from_secs(6 * 60 * 60); // 6 hours

// Backfiller config - tunable via environment variables for 15B+ record backfills
pub static WORKERS_BACKFILLER: LazyLock<usize> = LazyLock::new(|| {
    std::env::var("BACKFILLER_WORKERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(32) // Default: 32 concurrent repo fetches
});

pub static BACKFILLER_BATCH_SIZE: LazyLock<usize> = LazyLock::new(|| {
    std::env::var("BACKFILLER_BATCH_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000) // Default: dequeue 1000 repos per batch
});

pub static BACKFILLER_OUTPUT_HIGH_WATER_MARK: LazyLock<usize> = LazyLock::new(|| {
    std::env::var("BACKFILLER_OUTPUT_HIGH_WATER_MARK")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100_000) // Default: 100k records in output queue before backpressure
});

pub static BACKFILLER_TIMEOUT_SECS: LazyLock<u64> = LazyLock::new(|| {
    std::env::var("BACKFILLER_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(120) // Default: 2 minutes per repo fetch
});

#[must_use]
pub fn backfiller_timeout() -> Duration {
    Duration::from_secs(*BACKFILLER_TIMEOUT_SECS)
}

// Inline processing concurrency for firehose events
// Should be proportional to DB_POOL_SIZE to avoid excessive connection contention
/// Live indexing updates aggregates inline (`post_agg`/`profile_agg`). Set `LIVE_AGGREGATES=false`
/// to defer them (e.g. while a bulk load runs and a full recompute will follow).
pub static LIVE_AGGREGATES: LazyLock<bool> = LazyLock::new(|| {
    std::env::var("LIVE_AGGREGATES")
        .map(|v| v != "false" && v != "0")
        .unwrap_or(true)
});

/// Jobs drained from the `firehose_live` queue per indexing batch.
pub static FIREHOSE_LIVE_DRAIN_BATCH: LazyLock<usize> = LazyLock::new(|| {
    std::env::var("FIREHOSE_LIVE_DRAIN_BATCH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2000)
});

/// Concurrent shards a `firehose_live` batch is split into, partitioned by repo
/// DID so per-repo ordering holds. 1 = single-shard (previous behavior).
pub static FIREHOSE_LIVE_SHARDS: LazyLock<usize> = LazyLock::new(|| {
    std::env::var("FIREHOSE_LIVE_SHARDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|n| *n >= 1)
        .unwrap_or(1)
});

/// Serialize live like inserts across shards (the like index is contention-prone).
/// Set `LIVE_LIKE_SERIALIZE=false` to let shards write likes concurrently.
pub static LIVE_LIKE_SERIALIZE: LazyLock<bool> = LazyLock::new(|| {
    std::env::var("LIVE_LIKE_SERIALIZE")
        .map(|v| v != "false" && v != "0")
        .unwrap_or(true)
});

pub static INLINE_CONCURRENCY: LazyLock<usize> = LazyLock::new(|| {
    std::env::var("INLINE_CONCURRENCY")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100) // Default: 100 concurrent inline indexing tasks (5x pool size)
});

// Database pool size, applied PER POOL -- and pools are created per relay host and per
// labeler host, not once globally. The real ceiling is therefore
//   DB_POOL_SIZE * (relay hosts + labeler hosts)          -- ingester + labels
//   + max(DB_POOL_SIZE/4, FIREHOSE_LIVE_SHARDS * 8)       -- indexer live
//   + max(DB_POOL_SIZE/2, 10) + max(DB_POOL_SIZE/4, 5)    -- indexer backfill + labels
// With 3 relays, 3 labelers, DB_POOL_SIZE=36 and 6 live shards that is 291 connections,
// which will exhaust a server running the Postgres default max_connections. Size it
// against the host lists, not against the number of components.
pub static DB_POOL_SIZE: LazyLock<usize> = LazyLock::new(|| {
    std::env::var("DB_POOL_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(20) // Default: 20 connections per pool
});

// Pool timeouts. deadpool leaves all three unset by default, which means a caller that
// cannot get a connection waits *forever* instead of failing: an exhausted pool surfaces
// as silently stalled indexing rather than an error, and a connection the server closed
// underneath the pool is never noticed (every pool here uses RecyclingMethod::Fast, which
// runs no validation query on checkout). Bound all three so starvation is observable.
//
// Set any of these to 0 to disable that individual timeout and restore the old
// unbounded behaviour.
const DEFAULT_DB_WAIT_TIMEOUT_SECS: u64 = 30;
const DEFAULT_DB_CREATE_TIMEOUT_SECS: u64 = 10;
const DEFAULT_DB_RECYCLE_TIMEOUT_SECS: u64 = 10;

/// Parse a timeout given in whole seconds. An absent or unparseable value falls back to
/// `default_secs`; an explicit `0` means "no timeout" and maps to [`None`].
#[must_use]
fn parse_timeout_secs(raw: Option<&str>, default_secs: u64) -> Option<Duration> {
    let secs = raw
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(default_secs);
    if secs == 0 {
        None
    } else {
        Some(Duration::from_secs(secs))
    }
}

fn timeout_from_env(var: &str, default_secs: u64) -> Option<Duration> {
    parse_timeout_secs(std::env::var(var).ok().as_deref(), default_secs)
}

/// How long to wait for a free slot before giving up.
pub static DB_WAIT_TIMEOUT: LazyLock<Option<Duration>> =
    LazyLock::new(|| timeout_from_env("DB_WAIT_TIMEOUT_SECS", DEFAULT_DB_WAIT_TIMEOUT_SECS));

/// How long to wait for a brand-new connection to be established.
pub static DB_CREATE_TIMEOUT: LazyLock<Option<Duration>> =
    LazyLock::new(|| timeout_from_env("DB_CREATE_TIMEOUT_SECS", DEFAULT_DB_CREATE_TIMEOUT_SECS));

/// How long to wait for an existing connection to be recycled for reuse.
pub static DB_RECYCLE_TIMEOUT: LazyLock<Option<Duration>> =
    LazyLock::new(|| timeout_from_env("DB_RECYCLE_TIMEOUT_SECS", DEFAULT_DB_RECYCLE_TIMEOUT_SECS));

/// Build the [`Timeouts`] every pool in this crate shares.
#[must_use]
pub fn pg_pool_timeouts() -> deadpool_postgres::Timeouts {
    deadpool_postgres::Timeouts {
        wait: *DB_WAIT_TIMEOUT,
        create: *DB_CREATE_TIMEOUT,
        recycle: *DB_RECYCLE_TIMEOUT,
    }
}

/// The single place a Postgres pool is configured. Use this instead of
/// `PoolConfig::new(size)`, which leaves every timeout unset.
#[must_use]
pub fn pg_pool_config(max_size: usize) -> deadpool_postgres::PoolConfig {
    deadpool_postgres::PoolConfig {
        max_size,
        timeouts: pg_pool_timeouts(),
        ..Default::default()
    }
}

/// Session setup run once per pooled connection, before any staging DDL.
///
/// `client_min_messages=warning` is required because the bulk indexer issues
/// `CREATE TEMP TABLE IF NOT EXISTS` for its staging tables, while pools recycle with
/// `RecyclingMethod::Fast` and so never discard them. Every batch after the first
/// therefore raises a `duplicate_table` NOTICE, which tokio-postgres logs at INFO; that
/// stream dominates the journal and collapses log retention. Setting it on the session
/// means the server never generates or transmits the notice, which a client-side log
/// filter cannot achieve.
///
/// This is a `SET` rather than a libpq `options` string on purpose. `Config::options`
/// replaces whatever the connection URL carried, and the URL is where the deployment
/// supplies `-csearch_path=...`; setting it here would silently drop the schema and
/// resolve every unqualified name against `public`.
pub const SESSION_SETUP_SQL: &str = "SET client_min_messages TO warning;";

/// Build a pool whose connections are ready for `indexer::bulk`.
///
/// The staging-table DDL runs once per connection in a `post_create` hook instead of on
/// every batch. Pools that reach those functions must be built here or they fail with
/// `undefined_table`; a pool built any other way has no staging tables.
pub fn create_pg_pool(
    database_url: &str,
    pool_config: deadpool_postgres::PoolConfig,
) -> Result<deadpool_postgres::Pool, crate::types::WintermuteError> {
    use crate::types::WintermuteError;

    let mut cfg = deadpool_postgres::Config::new();
    cfg.url = Some(database_url.to_owned());
    cfg.manager = Some(deadpool_postgres::ManagerConfig {
        recycling_method: deadpool_postgres::RecyclingMethod::Fast,
    });
    cfg.pool = Some(pool_config);

    cfg.builder(deadpool_postgres::tokio_postgres::NoTls)
        .map_err(|e| WintermuteError::Other(format!("pool config invalid: {e}")))?
        .runtime(deadpool_postgres::Runtime::Tokio1)
        .post_create(deadpool_postgres::Hook::async_fn(|client, _| {
            Box::pin(async move {
                client
                    .batch_execute(&format!(
                        "{SESSION_SETUP_SQL}{}",
                        crate::indexer::bulk::BULK_STAGING_DDL
                    ))
                    .await
                    .map_err(|e| {
                        deadpool_postgres::HookError::message(format!(
                            "bulk staging DDL failed: {e}"
                        ))
                    })?;
                Ok(())
            })
        }))
        .build()
        .map_err(|e| WintermuteError::Other(format!("pool creation failed: {e}")))
}

// Backfiller direct write mode - bypass Fjall queue and write directly to PostgreSQL
// This eliminates the Fjall dequeue bottleneck (~3.5s per batch) for backfill operations
pub static BACKFILLER_DIRECT_WRITE: LazyLock<bool> = LazyLock::new(|| {
    std::env::var("BACKFILLER_DIRECT_WRITE")
        .ok()
        .is_none_or(|s| s == "true" || s == "1") // Default: enabled (bypass Fjall)
});

// Backfiller DB pool size - separate from main pool for direct write mode
pub static BACKFILLER_DB_POOL_SIZE: LazyLock<usize> = LazyLock::new(|| {
    std::env::var("BACKFILLER_DB_POOL_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(32) // Default: 32 connections for backfiller (matches worker count)
});

/// Stop writing like/follow/repost/block rows to the record table once the
/// dataplane synthesizes those records from the typed tables.
pub static RECORD_SKIP_BOILERPLATE: LazyLock<bool> = LazyLock::new(|| {
    std::env::var("RECORD_SKIP_BOILERPLATE")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false)
});

#[must_use]
pub fn boilerplate_collection(collection: &str) -> bool {
    matches!(
        collection,
        "app.bsky.feed.like"
            | "app.bsky.feed.repost"
            | "app.bsky.graph.follow"
            | "app.bsky.graph.block"
    )
}

// Comma-separated NSID prefixes (e.g. "app.bsky.,chat.bsky.,site.standard.") allowed in the record store. Empty = unset.
pub static RECORD_COLLECTION_ALLOWLIST: LazyLock<Vec<String>> = LazyLock::new(|| {
    std::env::var("RECORD_COLLECTION_ALLOWLIST")
        .ok()
        .map(|s| {
            s.split(',')
                .map(|p| p.trim().to_owned())
                .filter(|p| !p.is_empty())
                .collect()
        })
        .unwrap_or_default()
});

const LEGACY_INGEST_PREFIXES: [&str; 2] = ["app.bsky.", "chat.bsky."];

fn record_allowed(allow: &[String], collection: &str) -> bool {
    allow.is_empty() || allow.iter().any(|p| collection.starts_with(p.as_str()))
}

fn ingest_allowed(allow: &[String], collection: &str) -> bool {
    if allow.is_empty() {
        LEGACY_INGEST_PREFIXES
            .iter()
            .any(|p| collection.starts_with(p))
    } else {
        allow.iter().any(|p| collection.starts_with(p.as_str()))
    }
}

// Live indexer: unset allowlist allows all collections.
#[must_use]
pub fn record_collection_allowed(collection: &str) -> bool {
    record_allowed(&RECORD_COLLECTION_ALLOWLIST, collection)
}

// Backfiller/direct-index: unset allowlist falls back to the legacy bsky/chat filter.
#[must_use]
pub fn ingest_collection_allowed(collection: &str) -> bool {
    ingest_allowed(&RECORD_COLLECTION_ALLOWLIST, collection)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefixes(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn record_allowed_empty_allows_everything() {
        let allow: Vec<String> = Vec::new();
        assert!(record_allowed(&allow, "app.bsky.feed.post"));
        assert!(record_allowed(&allow, "com.whtwnd.blog.entry"));
    }

    #[test]
    fn record_allowed_filters_by_prefix() {
        let allow = prefixes(&["app.bsky.", "chat.bsky.", "site.standard."]);
        assert!(record_allowed(&allow, "app.bsky.feed.post"));
        assert!(record_allowed(&allow, "chat.bsky.actor.declaration"));
        assert!(record_allowed(&allow, "site.standard.profile"));
        assert!(!record_allowed(&allow, "com.whtwnd.blog.entry"));
        assert!(!record_allowed(&allow, "app.popsky.feed.post"));
    }

    #[test]
    fn ingest_allowed_empty_uses_legacy_bsky_chat() {
        let allow: Vec<String> = Vec::new();
        assert!(ingest_allowed(&allow, "app.bsky.feed.post"));
        assert!(ingest_allowed(&allow, "chat.bsky.actor.declaration"));
        assert!(!ingest_allowed(&allow, "site.standard.profile"));
        assert!(!ingest_allowed(&allow, "com.whtwnd.blog.entry"));
    }

    #[test]
    fn ingest_allowed_uses_configured_allowlist_when_set() {
        let allow = prefixes(&["app.bsky.", "chat.bsky.", "site.standard."]);
        assert!(ingest_allowed(&allow, "site.standard.profile"));
        assert!(ingest_allowed(&allow, "app.bsky.feed.post"));
        assert!(!ingest_allowed(&allow, "com.whtwnd.blog.entry"));
    }

    #[test]
    fn public_wrappers_use_env_allowlist() {
        // Unset in the test process: live allows all, ingest uses legacy filter.
        assert!(record_collection_allowed("com.whtwnd.blog.entry"));
        assert!(ingest_collection_allowed("app.bsky.feed.post"));
        assert!(!ingest_collection_allowed("com.whtwnd.blog.entry"));
    }

    #[test]
    fn timeout_absent_uses_default() {
        assert_eq!(parse_timeout_secs(None, 30), Some(Duration::from_secs(30)));
    }

    #[test]
    fn timeout_parses_explicit_value() {
        assert_eq!(
            parse_timeout_secs(Some("45"), 30),
            Some(Duration::from_secs(45))
        );
    }

    #[test]
    fn timeout_zero_disables() {
        // 0 is the documented escape hatch back to deadpool's unbounded behaviour.
        assert_eq!(parse_timeout_secs(Some("0"), 30), None);
    }

    #[test]
    fn timeout_unparseable_falls_back_to_default() {
        assert_eq!(
            parse_timeout_secs(Some("banana"), 30),
            Some(Duration::from_secs(30))
        );
        assert_eq!(
            parse_timeout_secs(Some(""), 7),
            Some(Duration::from_secs(7))
        );
        // Negative values do not parse as u64, so they fall back rather than wrapping.
        assert_eq!(
            parse_timeout_secs(Some("-5"), 7),
            Some(Duration::from_secs(7))
        );
    }

    #[test]
    fn timeout_trims_surrounding_whitespace() {
        assert_eq!(
            parse_timeout_secs(Some("  12\n"), 30),
            Some(Duration::from_secs(12))
        );
    }

    #[test]
    fn timeout_default_of_zero_stays_disabled() {
        assert_eq!(parse_timeout_secs(None, 0), None);
    }

    #[test]
    fn timeout_from_env_reads_the_named_var() {
        // Unset in the test process, so this exercises the fallback path.
        assert_eq!(
            timeout_from_env("RSKY_WINTERMUTE_TIMEOUT_VAR_THAT_IS_NOT_SET", 11),
            Some(Duration::from_secs(11))
        );
    }

    #[test]
    fn pool_timeouts_bound_all_three_by_default() {
        // The whole point of the helper: none of these may be None by default, or a
        // starved pool waits forever instead of erroring.
        let t = pg_pool_timeouts();
        assert_eq!(
            t.wait,
            Some(Duration::from_secs(DEFAULT_DB_WAIT_TIMEOUT_SECS))
        );
        assert_eq!(
            t.create,
            Some(Duration::from_secs(DEFAULT_DB_CREATE_TIMEOUT_SECS))
        );
        assert_eq!(
            t.recycle,
            Some(Duration::from_secs(DEFAULT_DB_RECYCLE_TIMEOUT_SECS))
        );
    }

    #[test]
    fn pool_config_carries_size_and_timeouts() {
        let cfg = pg_pool_config(12);
        assert_eq!(cfg.max_size, 12);
        assert_eq!(cfg.timeouts.wait, pg_pool_timeouts().wait);
        assert_eq!(cfg.timeouts.create, pg_pool_timeouts().create);
        assert_eq!(cfg.timeouts.recycle, pg_pool_timeouts().recycle);
    }

    #[test]
    fn session_setup_sets_client_min_messages() {
        assert!(SESSION_SETUP_SQL.contains("client_min_messages"));
        assert!(SESSION_SETUP_SQL.trim_end().ends_with(';'));
    }

    #[test]
    fn session_setup_does_not_touch_search_path() {
        // Config::options replaces what the URL carried, and the URL is where the
        // deployment supplies -csearch_path. This must never move back there.
        assert!(!SESSION_SETUP_SQL.contains("search_path"));
    }

    #[test]
    fn repo_backfill_bound_admits_below_the_limit() {
        assert!(repo_backfill_has_room(0, 10));
        assert!(repo_backfill_has_room(9, 10));
    }

    #[test]
    fn repo_backfill_bound_rejects_at_and_above_the_limit() {
        assert!(!repo_backfill_has_room(10, 10));
        assert!(!repo_backfill_has_room(11, 10));
        // A queue already past the bound when the process starts must not grow further.
        assert!(!repo_backfill_has_room(5_000_000, 250_000));
    }

    #[test]
    fn repo_backfill_bound_of_zero_means_unbounded() {
        assert!(repo_backfill_has_room(0, 0));
        assert!(repo_backfill_has_room(usize::MAX, 0));
    }
}
