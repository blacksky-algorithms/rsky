pub mod consumer;
pub mod did_helpers;
pub mod indexing;
pub mod label_indexer;
pub mod stream_indexer;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Error types for the indexer
#[derive(Debug, Error)]
pub enum IndexerError {
    #[error("Redis error: {0}")]
    Redis(#[from] redis::RedisError),

    #[error("Database error: {0}")]
    Database(anyhow::Error),

    #[error("Database pool error: {0}")]
    Pool(#[from] deadpool_postgres::PoolError),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Invalid URI: {0}")]
    InvalidUri(String),

    #[error("Other error: {0}")]
    Other(#[from] anyhow::Error),
}

/// Stream event from Redis (matches ingester StreamEvent)
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
        #[serde(default = "default_seq")]
        seq: i64,
        time: String,
        did: String,
        commit: String,
        rev: String,
    },
    #[serde(rename = "account")]
    Account {
        #[serde(default = "default_seq")]
        seq: i64,
        time: String,
        did: String,
        active: bool,
        status: Option<String>,
    },
    #[serde(rename = "identity")]
    Identity {
        #[serde(default = "default_seq")]
        seq: i64,
        time: String,
        did: String,
        handle: Option<String>,
    },
}

/// Label stream event from Redis
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

/// Default seq value for events that don't specify one
fn default_seq() -> i64 {
    SEQ_BACKFILL
}

/// Redis stream names
pub mod streams {
    pub const FIREHOSE_LIVE: &str = "firehose_live";
    pub const FIREHOSE_BACKFILL: &str = "firehose_backfill";
    pub const LABEL_LIVE: &str = "label_live";
}

/// Configuration for the indexer
#[derive(Debug, Clone)]
pub struct IndexerConfig {
    pub redis_url: String,
    pub database_url: String,
    pub streams: Vec<String>,
    pub consumer_group: String,
    pub consumer_name: String,
    pub concurrency: usize,
    pub batch_size: usize,
}

impl Default for IndexerConfig {
    fn default() -> Self {
        Self {
            redis_url: "redis://localhost:6379".to_string(),
            database_url: "postgres://localhost/bsky".to_string(),
            streams: vec![
                streams::FIREHOSE_LIVE.to_string(),
                streams::FIREHOSE_BACKFILL.to_string(),
            ],
            consumer_group: "firehose_group".to_string(),
            consumer_name: "indexer_1".to_string(),
            concurrency: 100,
            batch_size: 500,
        }
    }
}

/// Metrics tracking for the indexer
#[derive(Debug, Clone, Default)]
pub struct IndexerMetrics {
    pub processed: u64,
    pub failed: u64,
    pub waiting: i64,
    pub running: i64,
}

impl IndexerError {
    /// Check if this error should be logged at WARN instead of ERROR
    /// Returns true for:
    /// - Duplicate key violations (expected during concurrent/backfill processing)
    /// - Invalid UTF-8 byte sequences (bad data from firehose)
    pub fn is_expected_error(&self) -> bool {
        match self {
            IndexerError::Database(err) => {
                let err_msg = err.to_string().to_lowercase();

                // Duplicate key violations - these are expected and handled by ON CONFLICT
                if err_msg.contains("duplicate key") {
                    return true;
                }

                // Invalid UTF-8 sequences (null bytes, etc.) - bad data we should skip
                if err_msg.contains("invalid byte sequence for encoding") {
                    return true;
                }

                false
            }
            _ => false,
        }
    }
}
