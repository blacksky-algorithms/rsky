use prometheus::{
    Encoder, IntCounter, IntCounterVec, IntGauge, IntGaugeVec, TextEncoder, register_int_counter,
    register_int_counter_vec, register_int_gauge, register_int_gauge_vec,
};
use std::sync::LazyLock;

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

pub static INDEXER_QUEUE_LENGTH: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!(
        "indexer_queue_length",
        "Current length of indexer input queue"
    )
    .unwrap()
});

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

/// Encode all metrics in Prometheus text format
pub fn encode_metrics() -> Result<String, prometheus::Error> {
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
