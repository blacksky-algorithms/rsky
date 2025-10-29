use lazy_static::lazy_static;
use prometheus::{
    register_int_counter, register_int_gauge, Encoder, IntCounter, IntGauge, TextEncoder,
};

lazy_static! {
    /// Total repos processed successfully
    pub static ref REPOS_PROCESSED: IntCounter = register_int_counter!(
        "backfiller_repos_processed_total",
        "Total number of repos successfully processed"
    )
    .unwrap();

    /// Total repos that failed processing
    pub static ref REPOS_FAILED: IntCounter = register_int_counter!(
        "backfiller_repos_failed_total",
        "Total number of repos that failed processing"
    )
    .unwrap();

    /// Total repos sent to dead letter queue
    pub static ref REPOS_DEAD_LETTERED: IntCounter = register_int_counter!(
        "backfiller_repos_dead_lettered_total",
        "Total number of repos sent to dead letter queue"
    )
    .unwrap();

    /// Total records extracted from repos
    pub static ref RECORDS_EXTRACTED: IntCounter = register_int_counter!(
        "backfiller_records_extracted_total",
        "Total number of records extracted from repos"
    )
    .unwrap();

    /// Total retry attempts
    pub static ref RETRIES_ATTEMPTED: IntCounter = register_int_counter!(
        "backfiller_retries_attempted_total",
        "Total number of retry attempts"
    )
    .unwrap();

    /// Current number of repos waiting in input stream
    pub static ref REPOS_WAITING: IntGauge = register_int_gauge!(
        "backfiller_repos_waiting",
        "Current number of repos waiting in input stream"
    )
    .unwrap();

    /// Current number of repos being processed
    pub static ref REPOS_RUNNING: IntGauge = register_int_gauge!(
        "backfiller_repos_running",
        "Current number of repos actively being processed"
    )
    .unwrap();

    /// Current output stream length (backpressure indicator)
    pub static ref OUTPUT_STREAM_LENGTH: IntGauge = register_int_gauge!(
        "backfiller_output_stream_length",
        "Current length of output stream"
    )
    .unwrap();

    /// CAR fetch errors
    pub static ref CAR_FETCH_ERRORS: IntCounter = register_int_counter!(
        "backfiller_car_fetch_errors_total",
        "Total number of CAR fetch errors"
    )
    .unwrap();

    /// CAR parse errors
    pub static ref CAR_PARSE_ERRORS: IntCounter = register_int_counter!(
        "backfiller_car_parse_errors_total",
        "Total number of CAR parse errors"
    )
    .unwrap();

    /// Repo verification errors
    pub static ref VERIFICATION_ERRORS: IntCounter = register_int_counter!(
        "backfiller_verification_errors_total",
        "Total number of repo verification errors"
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
