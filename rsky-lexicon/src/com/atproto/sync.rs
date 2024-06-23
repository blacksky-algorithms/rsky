use chrono::{DateTime, Utc};
use libipld::Cid;

#[derive(Debug, Deserialize)]
pub struct SubscribeReposCommitOperation {
    pub path: String,
    pub action: String,
    pub cid: Option<Cid>,
}

#[derive(Debug, Deserialize)]
pub struct SubscribeReposCommit {
    #[serde(with = "serde_bytes")]
    pub blocks: Vec<u8>,
    pub commit: String,
    #[serde(rename(deserialize = "ops"))]
    pub operations: Vec<SubscribeReposCommitOperation>,
    pub prev: Option<String>,
    pub rebase: bool,
    pub repo: String,
    #[serde(rename(deserialize = "seq"))]
    pub sequence: i64,
    pub time: DateTime<Utc>,
    #[serde(rename(deserialize = "tooBig"))]
    pub too_big: bool,
}

/// Get the current commit CID & revision of the specified repo. Does not require auth.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetLatestCommitOutput {
    pub cid: String,
    pub rev: String,
}

#[derive(Debug, Deserialize)]
pub struct SubscribeReposHandle {
    pub did: String,
    pub handle: String,
    #[serde(rename(deserialize = "seq"))]
    pub sequence: i64,
    pub time: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct SubscribeReposTombstone {
    pub did: String,
    #[serde(rename(deserialize = "seq"))]
    pub sequence: i64,
    pub time: DateTime<Utc>,
}

pub enum SubscribeRepos {
    Commit(SubscribeReposCommit),
    Handle(SubscribeReposHandle),
    Tombstone(SubscribeReposTombstone),
}
