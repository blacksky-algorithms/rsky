pub mod backfill;
pub mod batcher;
pub mod firehose;
pub mod labeler;
pub mod metrics;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Error types for the ingester
#[derive(Debug, Error)]
pub enum IngesterError {
    #[error("Redis error: {0}")]
    Redis(#[from] redis::RedisError),

    #[error("WebSocket error: {0}")]
    WebSocket(#[from] tungstenite::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Backpressure: stream length {0} exceeds high water mark {1}")]
    Backpressure(usize, usize),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Other error: {0}")]
    Other(#[from] anyhow::Error),
}

/// Stream event types that can be written to Redis
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
    #[serde(rename = "update")]
    Update {
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
    #[serde(rename = "delete")]
    Delete {
        seq: i64,
        time: String,
        did: String,
        commit: String,
        rev: String,
        collection: String,
        rkey: String,
    },
    #[serde(rename = "repo")]
    Repo {
        seq: i64,
        time: String,
        did: String,
        commit: String,
        rev: String,
    },
    #[serde(rename = "account")]
    Account {
        seq: i64,
        time: String,
        did: String,
        active: bool,
        status: Option<String>,
    },
    #[serde(rename = "identity")]
    Identity {
        seq: i64,
        time: String,
        did: String,
        handle: String,
    },
}

/// Backfill event for repo backfill stream
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackfillEvent {
    pub did: String,
    pub host: String,
    pub rev: String,
    pub status: Option<String>,
    pub active: bool,
}

/// Label stream event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelStreamEvent {
    pub seq: i64,
    pub labels: Vec<Label>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Label {
    pub src: String,
    pub uri: String,
    pub cid: Option<String>,
    pub val: String,
    pub neg: Option<bool>,
    pub cts: String,
}

/// Special sequence number for backfill events
pub const SEQ_BACKFILL: i64 = -1;

/// Redis stream names
pub mod streams {
    pub const FIREHOSE_LIVE: &str = "firehose_live";
    pub const FIREHOSE_BACKFILL: &str = "firehose_backfill";
    pub const REPO_BACKFILL: &str = "repo_backfill";
    pub const LABEL_LIVE: &str = "label_live";
}

/// Configuration for the ingester
#[derive(Debug, Clone)]
pub struct IngesterConfig {
    pub redis_url: String,
    pub relay_hosts: Vec<String>,
    pub labeler_hosts: Vec<String>,
    pub high_water_mark: usize,
    pub batch_size: usize,
    pub batch_timeout_ms: u64,
}

impl Default for IngesterConfig {
    fn default() -> Self {
        Self {
            redis_url: "redis://localhost:6379".to_string(),
            relay_hosts: vec!["bsky.network".to_string()],
            labeler_hosts: vec![],
            high_water_mark: 100_000,
            batch_size: 500,
            batch_timeout_ms: 1000,
        }
    }
}
