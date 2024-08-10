use chrono::{DateTime, Utc};
use std::fmt;

#[derive(Debug, Serialize, Deserialize)]
pub struct SubscribeReposCommitOperation {
    pub path: String,
    pub action: String,
    pub cid: Option<String>,
}

/// Represents an update of repository state. Note that empty commits are allowed,
/// which include no repo data changes, but an update to rev and signature.
#[derive(Debug, Serialize, Deserialize)]
pub struct SubscribeReposCommit {
    pub r#type: String, // 'commit'
    pub seq: i64,
    pub time: DateTime<Utc>,
    pub rebase: bool,
    #[serde(rename = "tooBig")]
    pub too_big: bool,
    pub repo: String,
    pub commit: String,
    pub prev: Option<String>,
    pub rev: String,
    pub since: Option<String>,
    #[serde(with = "serde_bytes")]
    pub blocks: Vec<u8>,
    pub ops: Vec<SubscribeReposCommitOperation>,
    pub blobs: Vec<String>,
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

/// DEPRECATED -- Use #identity event instead
#[derive(Debug, Serialize, Deserialize)]
pub struct SubscribeReposHandle {
    pub r#type: String, // 'handle'
    pub did: String,
    pub handle: String,
    pub seq: i64,
    pub time: DateTime<Utc>,
}

/// Represents a change to an account's identity. Could be an updated handle, signing key, or
/// pds hosting endpoint. Serves as a prod to all downstream services to refresh their identity cache.
#[derive(Debug, Serialize, Deserialize)]
pub struct SubscribeReposIdentity {
    pub r#type: String, // 'identity'
    pub did: String,
    pub handle: Option<String>,
    pub seq: i64,
    pub time: DateTime<Utc>,
}

/// Represents a change to an account's identity. Could be an updated handle, signing key, or
/// pds hosting endpoint. Serves as a prod to all downstream services to refresh their identity cache.
#[derive(Debug, Serialize, Deserialize)]
pub struct SubscribeReposAccount {
    pub r#type: String, // 'account'
    pub seq: i64,
    pub did: String,
    pub time: DateTime<Utc>,
    pub active: bool,
    pub status: Option<AccountStatus>,
}

/// If active=false, this optional field indicates a reason for why the account is not active.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountStatus {
    Takendown,
    Suspended,
    Deleted,
    Deactivated,
}

impl fmt::Display for AccountStatus {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// DEPRECATED -- Use #account event instead
#[derive(Debug, Serialize, Deserialize)]
pub struct SubscribeReposTombstone {
    pub r#type: String, // 'tombstone'
    pub did: String,
    pub seq: i64,
    pub time: DateTime<Utc>,
}

pub enum SubscribeRepos {
    Commit(SubscribeReposCommit),
    Identity(SubscribeReposIdentity),
    Account(SubscribeReposAccount),
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
