use prometheus::{
    Encoder, IntCounter, IntCounterVec, IntGauge, IntGaugeVec, TextEncoder, register_int_counter,
    register_int_counter_vec, register_int_gauge, register_int_gauge_vec,
};
use std::sync::{LazyLock, Mutex};

// =============================================================================
// INGESTER METRICS
// =============================================================================

/// Firehose event processing
pub static INGESTER_FIREHOSE_EVENTS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        "ingester_firehose_events_total",
        "Total number of firehose events processed",
        &["stream"]
    )
    .unwrap()
});

pub static INGESTER_LAST_EVENT_TIME_SECONDS: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!(
        "ingester_last_event_time_seconds",
        "Unix timestamp of the most recent firehose event's declared time"
    )
    .unwrap()
});

pub static INGESTER_FIREHOSE_CREATE_EVENTS_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter!(
        "ingester_firehose_create_events_total",
        "Total number of create events from firehose"
    )
    .unwrap()
});

pub static INGESTER_FIREHOSE_UPDATE_EVENTS_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter!(
        "ingester_firehose_update_events_total",
        "Total number of update events from firehose"
    )
    .unwrap()
});

pub static INGESTER_FIREHOSE_DELETE_EVENTS_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter!(
        "ingester_firehose_delete_events_total",
        "Total number of delete events from firehose"
    )
    .unwrap()
});

pub static INGESTER_FIREHOSE_FILTERED_OPERATIONS_TOTAL: LazyLock<IntCounter> =
    LazyLock::new(|| {
        register_int_counter!(
            "ingester_firehose_filtered_operations_total",
            "Total number of operations filtered out (non-bsky collections)"
        )
        .unwrap()
    });

/// Stream lengths (Redis/fjall queue lengths)
pub static INGESTER_FIREHOSE_LIVE_LENGTH: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!(
        "ingester_firehose_live_length",
        "Current length of firehose_live stream"
    )
    .unwrap()
});

pub static INGESTER_REPO_BACKFILL_LENGTH: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!(
        "ingester_repo_backfill_length",
        "Current length of repo_backfill stream"
    )
    .unwrap()
});

pub static INGESTER_LABEL_LIVE_LENGTH: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!(
        "ingester_label_live_length",
        "Current length of label_live stream"
    )
    .unwrap()
});

pub static INGESTER_FIREHOSE_BACKFILL_LENGTH: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!(
        "ingester_firehose_backfill_length",
        "Current length of firehose_backfill stream"
    )
    .unwrap()
});

/// Backfill progress
pub static INGESTER_BACKFILL_REPOS_FETCHED_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter!(
        "ingester_backfill_repos_fetched_total",
        "Total number of repos fetched for backfill enumeration"
    )
    .unwrap()
});

pub static INGESTER_BACKFILL_REPOS_WRITTEN_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter!(
        "ingester_backfill_repos_written_total",
        "Total number of repos written to backfill queue"
    )
    .unwrap()
});

pub static INGESTER_BACKFILL_COMPLETE: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!(
        "ingester_backfill_complete",
        "Whether backfill enumeration is complete (0=in progress, 1=complete)"
    )
    .unwrap()
});

/// Connection and backpressure
pub static INGESTER_WEBSOCKET_CONNECTIONS: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    register_int_gauge_vec!(
        "ingester_websocket_connections",
        "Number of active websocket connections",
        &["type"]
    )
    .unwrap()
});

pub static INGESTER_BACKPRESSURE_ACTIVE: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!(
        "ingester_backpressure_active",
        "Whether backpressure is currently active (0=no, 1=yes)"
    )
    .unwrap()
});

pub static INGESTER_EVENTS_IN_MEMORY: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!(
        "ingester_events_in_memory",
        "Number of events currently buffered in memory"
    )
    .unwrap()
});

pub static INGESTER_OPS_FILTERED_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter!(
        "ingester_ops_filtered_total",
        "Total firehose ops dropped by the collection allowlist before enqueue"
    )
    .unwrap()
});

/// Errors
pub static INGESTER_ERRORS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        "ingester_errors_total",
        "Total number of ingester errors",
        &["type"]
    )
    .unwrap()
});

pub static INGESTER_BACKFILL_FETCH_ERRORS_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter!(
        "ingester_backfill_fetch_errors_total",
        "Total number of errors fetching repos during backfill enumeration"
    )
    .unwrap()
});

pub static INGESTER_BACKFILL_CURSOR_SKIPS_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter!(
        "ingester_backfill_cursor_skips_total",
        "Total number of cursor skips during backfill enumeration"
    )
    .unwrap()
});

pub static INGESTER_BACKFILL_CURSOR_RESET_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter!(
        "ingester_backfill_cursor_reset_total",
        "Total number of cursor resets due to Fjall data loss detection"
    )
    .unwrap()
});

pub static STORAGE_RECOVERY_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter!(
        "storage_recovery_total",
        "Total number of times storage was recovered from corruption"
    )
    .unwrap()
});

// =============================================================================
// BACKFILLER METRICS
// =============================================================================

/// Repository processing
pub static BACKFILLER_REPOS_PROCESSED_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter!(
        "backfiller_repos_processed_total",
        "Total number of repositories processed by backfiller"
    )
    .unwrap()
});

pub static BACKFILLER_REPOS_FAILED_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter!(
        "backfiller_repos_failed_total",
        "Total number of repositories that failed to process"
    )
    .unwrap()
});

pub static BACKFILLER_REPOS_DEAD_LETTERED_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter!(
        "backfiller_repos_dead_lettered_total",
        "Total number of repositories moved to dead letter queue"
    )
    .unwrap()
});

pub static BACKFILLER_RETRIES_ATTEMPTED_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter!(
        "backfiller_retries_attempted_total",
        "Total number of retry attempts"
    )
    .unwrap()
});

/// Record extraction
pub static BACKFILLER_RECORDS_EXTRACTED_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter!(
        "backfiller_records_extracted_total",
        "Total number of records extracted from repositories"
    )
    .unwrap()
});

pub static BACKFILLER_RECORDS_FILTERED_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter!(
        "backfiller_records_filtered_total",
        "Total number of records filtered out (non-bsky collections)"
    )
    .unwrap()
});

/// Queue status
pub static BACKFILLER_OUTPUT_STREAM_LENGTH: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!(
        "backfiller_output_stream_length",
        "Current length of backfiller output stream (firehose_backfill)"
    )
    .unwrap()
});

pub static BACKFILLER_REPOS_WAITING: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!(
        "backfiller_repos_waiting",
        "Number of repositories waiting in input queue"
    )
    .unwrap()
});

pub static BACKFILLER_REPOS_RUNNING: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!(
        "backfiller_repos_running",
        "Number of repositories currently being processed"
    )
    .unwrap()
});

/// Errors
pub static BACKFILLER_CAR_FETCH_ERRORS_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter!(
        "backfiller_car_fetch_errors_total",
        "Total number of CAR file fetch errors"
    )
    .unwrap()
});

pub static BACKFILLER_CAR_PARSE_ERRORS_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter!(
        "backfiller_car_parse_errors_total",
        "Total number of CAR file parse errors"
    )
    .unwrap()
});

pub static BACKFILLER_VERIFICATION_ERRORS_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter!(
        "backfiller_verification_errors_total",
        "Total number of repository verification errors"
    )
    .unwrap()
});

pub static BACKFILLER_BACKPRESSURE_EVENTS_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter!(
        "backfiller_backpressure_events_total",
        "Total number of times backfiller paused due to output stream backpressure"
    )
    .unwrap()
});

// =============================================================================
// INDEXER METRICS
// =============================================================================

/// Event type counters
pub static INDEXER_POST_EVENTS_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter!(
        "indexer_post_events_total",
        "Total number of post events indexed"
    )
    .unwrap()
});

pub static INDEXER_LIKE_EVENTS_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter!(
        "indexer_like_events_total",
        "Total number of like events indexed"
    )
    .unwrap()
});

pub static INDEXER_REPOST_EVENTS_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter!(
        "indexer_repost_events_total",
        "Total number of repost events indexed"
    )
    .unwrap()
});

pub static INDEXER_FOLLOW_EVENTS_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter!(
        "indexer_follow_events_total",
        "Total number of follow events indexed"
    )
    .unwrap()
});

pub static INDEXER_BLOCK_EVENTS_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter!(
        "indexer_block_events_total",
        "Total number of block events indexed"
    )
    .unwrap()
});

pub static INDEXER_PROFILE_EVENTS_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter!(
        "indexer_profile_events_total",
        "Total number of profile events indexed"
    )
    .unwrap()
});

/// Overall indexer stats
pub static INDEXER_RECORDS_PROCESSED_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter!(
        "indexer_records_processed_total",
        "Total number of records processed by indexer"
    )
    .unwrap()
});

pub static INDEXER_RECORDS_FAILED_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter!(
        "indexer_records_failed_total",
        "Total number of records that failed to index"
    )
    .unwrap()
});

pub static INDEXER_STALE_WRITES_SKIPPED_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter!(
        "indexer_stale_writes_skipped_total",
        "Total number of stale writes skipped (older rev)"
    )
    .unwrap()
});

pub static INDEXER_RECORDS_FILTERED_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter!(
        "indexer_records_filtered_total",
        "Total number of records skipped (collection not in allowlist)"
    )
    .unwrap()
});

pub static INDEXER_QUEUE_LENGTH: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!(
        "indexer_queue_length",
        "Current length of indexer input queue"
    )
    .unwrap()
});

// =============================================================================
// DATABASE POOL METRICS
// =============================================================================
//
// Every Postgres pool in this process is bounded by DB_POOL_SIZE, but that is a
// PER-POOL limit and pools are created per relay host AND per labeler host, plus
// three more inside the indexer. config.rs:186-193 works the ceiling out to 291
// connections for a plausible configuration — past a default max_connections of
// 100. Until now nothing reported actual usage: `pool.status()` was never called
// anywhere in the crate, and the only pool observability was a single line at
// indexer startup logging the CONFIGURED sizes of three of the pools.
//
// That gap matters more since the wait/create/recycle timeouts were bounded.
// Before, an exhausted pool blocked forever and showed up as silently stalled
// indexing. Now it returns an error after DB_WAIT_TIMEOUT_SECS — visible in
// ingester_errors_total, but indistinguishable there from any other failure.
// These four gauges are what make the difference legible: a pool sitting at
// available=0 with waiting>0 is starvation, and nothing else looks like that.

pub static DB_POOL_MAX_SIZE: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    register_int_gauge_vec!(
        "db_pool_max_size",
        "Configured maximum connections for this pool",
        &["pool"]
    )
    .unwrap()
});

pub static DB_POOL_SIZE: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    register_int_gauge_vec!(
        "db_pool_size",
        "Connections currently held by this pool, idle or checked out",
        &["pool"]
    )
    .unwrap()
});

pub static DB_POOL_AVAILABLE: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    register_int_gauge_vec!(
        "db_pool_available",
        "Idle connections available for immediate checkout",
        &["pool"]
    )
    .unwrap()
});

pub static DB_POOL_WAITING: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    register_int_gauge_vec!(
        "db_pool_waiting",
        "Callers blocked waiting for a connection. Sustained non-zero means \
         starvation, and each waiter fails once DB_WAIT_TIMEOUT_SECS elapses",
        &["pool"]
    )
    .unwrap()
});

/// Pools registered for sampling, keyed by a stable name.
///
/// A `Pool` is a handle around an `Arc`, so holding one here neither keeps the
/// underlying connections alive nor costs anything to clone.
static POOLS: LazyLock<Mutex<Vec<(String, deadpool_postgres::Pool)>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

/// Register a pool under `name`, replacing any pool already registered under it.
///
/// Replacing rather than appending is deliberate. The ingester builds a fresh
/// pool each time it re-establishes a firehose connection, so appending would
/// grow this list without bound across a long-lived process and report stale
/// gauges for pools that no longer exist. Keying on name means the registry is
/// bounded by the number of distinct pools, which is what the label set already
/// assumes.
pub fn register_pool(name: impl Into<String>, pool: &deadpool_postgres::Pool) {
    let name = name.into();
    // A panicking holder must not take pool observability down with it; the data
    // behind this lock is a plain Vec that cannot be left half-updated.
    let mut pools = POOLS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(slot) = pools.iter_mut().find(|(n, _)| *n == name) {
        slot.1 = pool.clone();
    } else {
        pools.push((name, pool.clone()));
    }
}

/// Refresh every pool gauge from `Pool::status()`.
///
/// Called from the /metrics handler rather than on a timer: the values are read
/// straight out of the pool's own atomics, so sampling costs nothing measurable,
/// and doing it at scrape time means what is exported is what was true when
/// Prometheus asked rather than up to a tick earlier.
pub fn sample_pools() {
    let pools = POOLS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    for (name, pool) in pools.iter() {
        let s = pool.status();
        let labels = &[name.as_str()];
        DB_POOL_MAX_SIZE
            .with_label_values(labels)
            .set(i64::try_from(s.max_size).unwrap_or(i64::MAX));
        DB_POOL_SIZE
            .with_label_values(labels)
            .set(i64::try_from(s.size).unwrap_or(i64::MAX));
        DB_POOL_AVAILABLE
            .with_label_values(labels)
            .set(i64::try_from(s.available).unwrap_or(i64::MAX));
        DB_POOL_WAITING
            .with_label_values(labels)
            .set(i64::try_from(s.waiting).unwrap_or(i64::MAX));
    }
}

/// How many registry entries carry `name`. Exposed for tests, which run in
/// parallel against this shared registry and so cannot assert on its total size.
#[must_use]
pub fn count_pools_named(name: &str) -> usize {
    POOLS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .filter(|(n, _)| n == name)
        .count()
}

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

/// Encode all metrics in Prometheus text format
pub fn encode_metrics() -> Result<String, prometheus::Error> {
    // Pool gauges are pull-based: nothing else writes them, so they have to be
    // refreshed here or they would export whatever was true at registration.
    sample_pools();

    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer)?;
    String::from_utf8(buffer)
        .map_err(|e| prometheus::Error::Msg(format!("failed to convert metrics to UTF-8: {e}")))
}

/// Initialize metrics that need starting values
pub fn initialize_metrics() {
    // Set initial values for gauge metrics
    INGESTER_BACKPRESSURE_ACTIVE.set(0);
    INGESTER_BACKFILL_COMPLETE.set(0);
    INGESTER_EVENTS_IN_MEMORY.set(0);
    BACKFILLER_REPOS_RUNNING.set(0);
}

#[cfg(test)]
mod pool_metric_tests {
    use super::*;
    use deadpool_postgres::{Config, ManagerConfig, RecyclingMethod, Runtime};
    use tokio_postgres::NoTls;

    fn a_pool(size: usize) -> deadpool_postgres::Pool {
        let mut cfg = Config::new();
        cfg.url = Some("postgres://nobody@127.0.0.1:1/none".to_owned());
        cfg.manager = Some(ManagerConfig {
            recycling_method: RecyclingMethod::Fast,
        });
        cfg.pool = Some(crate::config::pg_pool_config(size));
        // deadpool connects lazily, so this never touches the network.
        cfg.create_pool(Some(Runtime::Tokio1), NoTls).unwrap()
    }

    /// The ingester rebuilds its pool on every firehose reconnect. Appending
    /// would grow the registry without bound over a long-lived process and keep
    /// reporting gauges for pools that no longer exist.
    #[test]
    fn registering_the_same_name_replaces_rather_than_appends() {
        register_pool("test:replace", &a_pool(7));
        assert_eq!(count_pools_named("test:replace"), 1);

        register_pool("test:replace", &a_pool(9));
        assert_eq!(
            count_pools_named("test:replace"),
            1,
            "re-registering an existing name must not add a second entry"
        );

        sample_pools();
        assert_eq!(
            DB_POOL_MAX_SIZE.with_label_values(&["test:replace"]).get(),
            9,
            "the gauge should reflect the pool registered most recently"
        );
    }

    #[test]
    fn distinct_names_are_tracked_separately() {
        register_pool("test:distinct_a", &a_pool(3));
        register_pool("test:distinct_b", &a_pool(4));
        sample_pools();

        assert_eq!(DB_POOL_MAX_SIZE.with_label_values(&["test:distinct_a"]).get(), 3);
        assert_eq!(DB_POOL_MAX_SIZE.with_label_values(&["test:distinct_b"]).get(), 4);
    }

    /// A fresh pool holds nothing and nobody is blocked on it. The point of the
    /// assertion is the pairing: available=0 alone is normal, available=0 WITH
    /// waiting>0 is starvation, and that is the distinction the gauges exist to
    /// make visible.
    #[test]
    fn a_fresh_pool_is_empty_and_uncontended() {
        register_pool("test:fresh", &a_pool(5));
        sample_pools();

        assert_eq!(DB_POOL_SIZE.with_label_values(&["test:fresh"]).get(), 0);
        assert_eq!(DB_POOL_AVAILABLE.with_label_values(&["test:fresh"]).get(), 0);
        assert_eq!(DB_POOL_WAITING.with_label_values(&["test:fresh"]).get(), 0);
    }
}
