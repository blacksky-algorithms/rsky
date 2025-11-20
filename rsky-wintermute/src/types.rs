use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum WintermuteError {
    #[error("storage error: {0}")]
    Storage(#[from] fjall::Error),
    #[error("postgres error: {0}")]
    Postgres(#[from] tokio_postgres::Error),
    #[error("pool error: {0}")]
    Pool(#[from] deadpool_postgres::PoolError),
    #[error("websocket error: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("repo error: {0}")]
    Repo(String),
    #[error("other: {0}")]
    Other(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirehoseEvent {
    pub seq: i64,
    pub did: String,
    pub time: String,
    pub kind: String,
    pub commit: Option<CommitData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitData {
    pub rev: String,
    pub ops: Vec<RepoOp>,
    pub blocks: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoOp {
    pub action: String,
    pub path: String,
    pub cid: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackfillJob {
    pub did: String,
    pub retry_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexJob {
    pub uri: String,
    pub cid: String,
    pub action: WriteAction,
    pub record: Option<serde_json::Value>,
    pub indexed_at: String,
    pub rev: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WriteAction {
    Create,
    Update,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelEvent {
    pub seq: i64,
    pub labels: Vec<Label>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Label {
    pub src: String,
    pub uri: String,
    pub val: String,
    pub cts: String,
}
