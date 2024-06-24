use chrono::{DateTime, Utc};

#[derive(Debug, Deserialize)]
pub struct SubscribeReposCommitOperation {
    pub path: String,
    pub action: String,
    pub cid: Option<String>,
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

/// Get the hosting status for a repository, on this server.
/// Expected to be implemented by PDS and Relay.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetRepoStatusOutput {
    pub did: String,
    pub active: bool,
    // If active=false, this optional field indicates a possible reason for why the account
    // is not active. If active=false and no status is supplied, then the host makes no claim for
    // why the repository is no longer being hosted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<RepoStatus>,
    // Optional field, the current rev of the repo, if active=true
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
}

/// List blob CIDs for an account, since some repo revision. Does not require auth;
/// implemented by PDS
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListBlobsOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub cids: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListReposOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub repos: Vec<RefRepo>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RepoStatus {
    Takedown,
    Suspended,
    Deactivated,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RefRepo {
    pub did: String,
    // Current repo commit CID
    pub head: String,
    pub rev: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
    // If active=false, this optional field indicates a possible reason for why the account
    // is not active. If active=false and no status is supplied, then the host makes no claim for
    // why the repository is no longer being hosted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<RepoStatus>,
}
