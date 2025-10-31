use lazy_static::lazy_static;
use prometheus::{
    register_int_counter, register_int_gauge, Encoder, IntCounter, IntGauge, TextEncoder,
};

lazy_static! {
    // Stream indexer metrics
    /// Total events processed across all streams
    pub static ref EVENTS_PROCESSED_TOTAL: IntCounter = register_int_counter!(
        "indexer_events_processed_total",
        "Total events processed by indexer"
    )
    .unwrap();

    /// Total create events processed
    pub static ref CREATE_EVENTS_TOTAL: IntCounter = register_int_counter!(
        "indexer_create_events_total",
        "Total create events processed"
    )
    .unwrap();

    /// Total update events processed
    pub static ref UPDATE_EVENTS_TOTAL: IntCounter = register_int_counter!(
        "indexer_update_events_total",
        "Total update events processed"
    )
    .unwrap();

    /// Total delete events processed
    pub static ref DELETE_EVENTS_TOTAL: IntCounter = register_int_counter!(
        "indexer_delete_events_total",
        "Total delete events processed"
    )
    .unwrap();

    /// Total repo sync events processed
    pub static ref REPO_EVENTS_TOTAL: IntCounter = register_int_counter!(
        "indexer_repo_events_total",
        "Total repo sync events processed"
    )
    .unwrap();

    /// Total account events processed
    pub static ref ACCOUNT_EVENTS_TOTAL: IntCounter = register_int_counter!(
        "indexer_account_events_total",
        "Total account events processed"
    )
    .unwrap();

    /// Total identity/handle events processed
    pub static ref IDENTITY_EVENTS_TOTAL: IntCounter = register_int_counter!(
        "indexer_identity_events_total",
        "Total identity/handle events processed"
    )
    .unwrap();

    /// Total database writes (upserts/deletes)
    pub static ref DB_WRITES_TOTAL: IntCounter = register_int_counter!(
        "indexer_db_writes_total",
        "Total database writes performed"
    )
    .unwrap();

    /// Total processing errors
    pub static ref ERRORS_TOTAL: IntCounter = register_int_counter!(
        "indexer_errors_total",
        "Total processing errors encountered"
    )
    .unwrap();

    /// Expected errors (duplicates, invalid UTF-8, etc.)
    pub static ref EXPECTED_ERRORS_TOTAL: IntCounter = register_int_counter!(
        "indexer_expected_errors_total",
        "Total expected errors (duplicates, invalid data)"
    )
    .unwrap();

    /// Unexpected errors (database failures, etc.)
    pub static ref UNEXPECTED_ERRORS_TOTAL: IntCounter = register_int_counter!(
        "indexer_unexpected_errors_total",
        "Total unexpected errors"
    )
    .unwrap();

    // Collection-specific metrics (top collections)
    /// Posts indexed
    pub static ref POST_EVENTS_TOTAL: IntCounter = register_int_counter!(
        "indexer_post_events_total",
        "Total posts indexed (app.bsky.feed.post)"
    )
    .unwrap();

    /// Likes indexed
    pub static ref LIKE_EVENTS_TOTAL: IntCounter = register_int_counter!(
        "indexer_like_events_total",
        "Total likes indexed (app.bsky.feed.like)"
    )
    .unwrap();

    /// Reposts indexed
    pub static ref REPOST_EVENTS_TOTAL: IntCounter = register_int_counter!(
        "indexer_repost_events_total",
        "Total reposts indexed (app.bsky.feed.repost)"
    )
    .unwrap();

    /// Follows indexed
    pub static ref FOLLOW_EVENTS_TOTAL: IntCounter = register_int_counter!(
        "indexer_follow_events_total",
        "Total follows indexed (app.bsky.graph.follow)"
    )
    .unwrap();

    /// Blocks indexed
    pub static ref BLOCK_EVENTS_TOTAL: IntCounter = register_int_counter!(
        "indexer_block_events_total",
        "Total blocks indexed (app.bsky.graph.block)"
    )
    .unwrap();

    /// Profiles indexed
    pub static ref PROFILE_EVENTS_TOTAL: IntCounter = register_int_counter!(
        "indexer_profile_events_total",
        "Total profiles indexed (app.bsky.actor.profile)"
    )
    .unwrap();

    // Label indexer metrics
    /// Total labels processed
    pub static ref LABELS_PROCESSED_TOTAL: IntCounter = register_int_counter!(
        "indexer_labels_processed_total",
        "Total labels processed by label indexer"
    )
    .unwrap();

    /// Labels added (positive labels)
    pub static ref LABELS_ADDED_TOTAL: IntCounter = register_int_counter!(
        "indexer_labels_added_total",
        "Total labels added (not negated)"
    )
    .unwrap();

    /// Labels removed (negated labels)
    pub static ref LABELS_REMOVED_TOTAL: IntCounter = register_int_counter!(
        "indexer_labels_removed_total",
        "Total labels removed (negated)"
    )
    .unwrap();

    // Performance metrics
    /// Pending messages in consumer group
    pub static ref PENDING_MESSAGES: IntGauge = register_int_gauge!(
        "indexer_pending_messages",
        "Number of pending messages in consumer group"
    )
    .unwrap();

    /// Active concurrent tasks
    pub static ref ACTIVE_TASKS: IntGauge = register_int_gauge!(
        "indexer_active_tasks",
        "Number of active concurrent indexing tasks"
    )
    .unwrap();

    /// Messages ACK failures
    pub static ref ACK_FAILURES_TOTAL: IntCounter = register_int_counter!(
        "indexer_ack_failures_total",
        "Total message ACK failures"
    )
    .unwrap();
}

/// Encode metrics for Prometheus scraping
pub fn encode_metrics() -> Result<String, Box<dyn std::error::Error>> {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer)?;
    Ok(String::from_utf8(buffer)?)
}
