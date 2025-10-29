pub mod metrics;
pub mod repo_backfiller;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Backfill event from repo_backfill stream
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackfillEvent {
    pub did: String,
    pub host: String,
    pub rev: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    pub active: bool,
}

/// Stream event to write to firehose_backfill
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum StreamEvent {
    #[serde(rename = "create")]
    Create {
        seq: i64,
        time: String,
        did: String,
        commit: String,
        rev: String,
        collection: String,
        rkey: String,
        cid: String,
        record: serde_json::Value,
    },
    #[serde(rename = "repo")]
    Repo {
        seq: i64,
        time: String,
        did: String,
        commit: String,
        rev: String,
    },
}

/// Backfiller configuration
#[derive(Debug, Clone)]
pub struct BackfillerConfig {
    pub redis_url: String,
    pub stream_in: String,
    pub stream_out: String,
    pub stream_dead_letter: String,
    pub consumer_group: String,
    pub consumer_name: String,
    pub concurrency: usize,
    pub high_water_mark: usize,
    pub http_timeout_secs: u64,
    pub max_retries: u32,
    pub retry_initial_backoff_ms: u64,
    pub retry_max_backoff_ms: u64,
    pub metrics_port: u16,
}

impl Default for BackfillerConfig {
    fn default() -> Self {
        Self {
            redis_url: "redis://localhost:6379".to_string(),
            stream_in: "repo_backfill".to_string(),
            stream_out: "firehose_backfill".to_string(),
            stream_dead_letter: "repo_backfill_dlq".to_string(),
            consumer_group: "repo_backfill_group".to_string(),
            consumer_name: "backfiller_1".to_string(),
            concurrency: 2,
            high_water_mark: 100_000,
            http_timeout_secs: 60,
            max_retries: 3,
            retry_initial_backoff_ms: 1000,
            retry_max_backoff_ms: 30000,
            metrics_port: 9090,
        }
    }
}

/// Special sequence number for backfill events
pub const SEQ_BACKFILL: i64 = -1;

/// Backfiller error types
#[derive(Error, Debug)]
pub enum BackfillerError {
    #[error("Redis error: {0}")]
    Redis(#[from] redis::RedisError),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("CAR parsing error: {0}")]
    Car(String),

    #[error("Repo verification error: {0}")]
    Verification(String),

    #[error("Identity resolution error: {0}")]
    Identity(String),

    #[error("Other error: {0}")]
    Other(#[from] anyhow::Error),
}

impl From<String> for BackfillerError {
    fn from(s: String) -> Self {
        BackfillerError::Other(anyhow::anyhow!(s))
    }
}
