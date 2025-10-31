use lazy_static::lazy_static;
use prometheus::{
    register_int_counter, register_int_gauge, Encoder, IntCounter, IntGauge, TextEncoder,
};

lazy_static! {
    // Firehose metrics
    /// Total incoming events from firehose
    pub static ref FIREHOSE_EVENTS_TOTAL: IntCounter = register_int_counter!(
        "ingester_firehose_events_total",
        "Total events received from firehose"
    )
    .unwrap();

    /// Total create events from firehose
    pub static ref FIREHOSE_CREATE_EVENTS: IntCounter = register_int_counter!(
        "ingester_firehose_create_events_total",
        "Total create events from firehose"
    )
    .unwrap();

    /// Total update events from firehose
    pub static ref FIREHOSE_UPDATE_EVENTS: IntCounter = register_int_counter!(
        "ingester_firehose_update_events_total",
        "Total update events from firehose"
    )
    .unwrap();

    /// Total delete events from firehose
    pub static ref FIREHOSE_DELETE_EVENTS: IntCounter = register_int_counter!(
        "ingester_firehose_delete_events_total",
        "Total delete events from firehose"
    )
    .unwrap();

    /// Total events written to Redis streams
    pub static ref STREAM_EVENTS_TOTAL: IntCounter = register_int_counter!(
        "ingester_stream_events_total",
        "Total events written to Redis streams"
    )
    .unwrap();

    /// Total errors encountered
    pub static ref ERRORS_TOTAL: IntCounter = register_int_counter!(
        "ingester_errors_total",
        "Total errors encountered"
    )
    .unwrap();

    /// Current firehose_live stream length
    pub static ref FIREHOSE_LIVE_LENGTH: IntGauge = register_int_gauge!(
        "ingester_firehose_live_length",
        "Current length of firehose_live stream"
    )
    .unwrap();

    /// Current label_live stream length
    pub static ref LABEL_LIVE_LENGTH: IntGauge = register_int_gauge!(
        "ingester_label_live_length",
        "Current length of label_live stream"
    )
    .unwrap();

    /// Current repo_backfill stream length
    pub static ref REPO_BACKFILL_LENGTH: IntGauge = register_int_gauge!(
        "ingester_repo_backfill_length",
        "Current length of repo_backfill stream"
    )
    .unwrap();

    /// Backpressure active (1 = yes, 0 = no)
    pub static ref BACKPRESSURE_ACTIVE: IntGauge = register_int_gauge!(
        "ingester_backpressure_active",
        "Whether backpressure is currently active (1=yes, 0=no)"
    )
    .unwrap();

    /// WebSocket connections active
    pub static ref WEBSOCKET_CONNECTIONS: IntGauge = register_int_gauge!(
        "ingester_websocket_connections",
        "Number of active WebSocket connections"
    )
    .unwrap();

    /// Events in memory (unbounded channel)
    pub static ref EVENTS_IN_MEMORY: IntGauge = register_int_gauge!(
        "ingester_events_in_memory",
        "Number of events in memory waiting to be written"
    )
    .unwrap();

    // Labeler metrics
    /// Total incoming events from labeler
    pub static ref LABELER_EVENTS_TOTAL: IntCounter = register_int_counter!(
        "ingester_labeler_events_total",
        "Total events received from labeler"
    )
    .unwrap();

    /// Total labels written
    pub static ref LABELS_WRITTEN_TOTAL: IntCounter = register_int_counter!(
        "ingester_labels_written_total",
        "Total labels written to label_live stream"
    )
    .unwrap();

    // Backfill Ingester metrics
    /// Total repos fetched from listRepos
    pub static ref BACKFILL_REPOS_FETCHED: IntCounter = register_int_counter!(
        "ingester_backfill_repos_fetched_total",
        "Total repos fetched from listRepos"
    )
    .unwrap();

    /// Total repos written to repo_backfill stream
    pub static ref BACKFILL_REPOS_WRITTEN: IntCounter = register_int_counter!(
        "ingester_backfill_repos_written_total",
        "Total repos written to repo_backfill stream"
    )
    .unwrap();

    /// Backfill fetch errors
    pub static ref BACKFILL_FETCH_ERRORS: IntCounter = register_int_counter!(
        "ingester_backfill_fetch_errors_total",
        "Total errors fetching from listRepos"
    )
    .unwrap();

    /// Backfill cursor skips (when errors force cursor advancement)
    pub static ref BACKFILL_CURSOR_SKIPS: IntCounter = register_int_counter!(
        "ingester_backfill_cursor_skips_total",
        "Total cursor skips due to persistent errors"
    )
    .unwrap();

    /// Backfill complete (1 = done, 0 = in progress)
    pub static ref BACKFILL_COMPLETE: IntGauge = register_int_gauge!(
        "ingester_backfill_complete",
        "Whether backfill is complete (1=done, 0=in progress)"
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
