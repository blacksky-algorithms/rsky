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
    WebSocket(Box<tokio_tungstenite::tungstenite::Error>),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("repo error: {0}")]
    Repo(String),
    #[error("other: {0}")]
    Other(String),
}

impl WintermuteError {
    /// Returns true if this error indicates storage corruption that requires recovery
    #[must_use]
    pub fn is_storage_corrupted(&self) -> bool {
        match self {
            Self::Storage(fjall_err) => {
                // Check error message for corruption indicators
                let msg = format!("{fjall_err:?}");
                msg.contains("Poisoned")
                    || msg.contains("JournalRecovery")
                    || msg.contains("InvalidVersion")
            }
            _ => false,
        }
    }
}

impl From<tokio_tungstenite::tungstenite::Error> for WintermuteError {
    fn from(error: tokio_tungstenite::tungstenite::Error) -> Self {
        Self::WebSocket(Box::new(error))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirehoseEvent {
    pub seq: i64,
    pub did: String,
    pub time: String,
    pub kind: String,
    pub commit: Option<CommitData>,
    pub identity: Option<IdentityData>,
    pub account: Option<AccountData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityData {
    pub handle: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountData {
    pub active: bool,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitData {
    pub rev: String,
    pub ops: Vec<RepoOp>,
    pub blocks: Vec<u8>,
    /// The `since` field of the `#commit` frame: the rev this commit claims
    /// to follow. Advisory only (see `ingester::sync11`).
    #[serde(default)]
    pub since: Option<String>,
    /// The `prevData` field of the `#commit` frame: the MST root of the
    /// previous commit. `None` from a host that does not implement sync 1.1.
    #[serde(default)]
    pub prev_data: Option<String>,
    /// The MST root (`data`) of this commit, decoded from the commit block
    /// carried in `blocks`. `None` when the frame carried no commit block.
    #[serde(default)]
    pub data: Option<String>,
    /// The frame's `tooBig` flag: the relay omitted blocks.
    #[serde(default)]
    pub too_big: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoOp {
    pub action: String,
    pub path: String,
    pub cid: Option<String>,
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
    pub cid: Option<String>,
    pub val: String,
    pub neg: bool,
    pub cts: String,
    pub exp: Option<String>,
}
