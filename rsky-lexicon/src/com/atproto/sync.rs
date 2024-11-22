use chrono::{DateTime, Utc};
use lexicon_cid::Cid;
use serde::{Deserialize, Deserializer, Serialize};
use serde_cbor::tags::Tagged;
use serde_json::Value;
use std::fmt;

const CBOR_TAG_CID: u64 = 42;
const MULTIBASE_IDENTITY: u8 = 0;

#[derive(Debug, Serialize, Deserialize)]
pub struct SubscribeReposCommitOperation {
    pub path: String,
    pub action: String,
    pub cid: Option<Cid>,
}

/// Represents an update of repository state. Note that empty commits are allowed,
/// which include no repo data changes, but an update to rev and signature.
#[derive(Debug, Serialize, Deserialize)]
pub struct SubscribeReposCommit {
    pub seq: i64,
    pub time: DateTime<Utc>,
    pub rebase: bool,
    #[serde(rename = "tooBig")]
    pub too_big: bool,
    pub repo: String,
    #[serde(deserialize_with = "deserialize_cid_v1")]
    pub commit: Cid,
    #[serde(
        default = "default_resource",
        deserialize_with = "deserialize_option_cid_v1"
    )]
    pub prev: Option<Cid>,
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
    pub did: String,
    pub handle: String,
    pub seq: i64,
    pub time: DateTime<Utc>,
}

/// Represents a change to an account's identity. Could be an updated handle, signing key, or
/// pds hosting endpoint. Serves as a prod to all downstream services to refresh their identity cache.
#[derive(Debug, Serialize, Deserialize)]
pub struct SubscribeReposIdentity {
    pub did: String,
    pub handle: Option<String>,
    pub seq: i64,
    pub time: DateTime<Utc>,
}

/// Represents a change to an account's identity. Could be an updated handle, signing key, or
/// pds hosting endpoint. Serves as a prod to all downstream services to refresh their identity cache.
#[derive(Debug, Serialize, Deserialize)]
pub struct SubscribeReposAccount {
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
    pub did: String,
    pub seq: i64,
    pub time: DateTime<Utc>,
}

#[derive(Debug)]
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

pub fn deserialize_cid_v1<'de, D>(deserializer: D) -> Result<Cid, D::Error>
where
    D: Deserializer<'de>,
{
    let buf = Tagged::<serde_bytes::ByteBuf>::deserialize(deserializer)?;
    match buf.tag {
        Some(CBOR_TAG_CID) | None => {
            let mut bz = buf.value.into_vec();

            if bz.first() == Some(&MULTIBASE_IDENTITY) {
                bz.remove(0);
            }

            Ok(Cid::try_from(bz).map_err(|e| {
                serde::de::Error::custom(format!("Failed to deserialize Cid: {}", e))
            })?)
        }
        Some(_) => Err(serde::de::Error::custom("unexpected tag")),
    }
}

pub fn deserialize_option_cid_v1<'de, D>(deserializer: D) -> Result<Option<Cid>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum BlobFormat {
        Legacy(Tagged<serde_bytes::ByteBuf>),
        Link(serde_json::Map<String, Value>),
    }

    // Deserialize into an optional map, expecting an object like {"$link": "cid_string"}
    let opt_blob = Option::<BlobFormat>::deserialize(deserializer)?;

    match opt_blob {
        // If there's no object, return None
        None => Ok(None),
        Some(BlobFormat::Link(map)) => {
            // Check if the map contains the "$link" key
            if let Some(Value::String(link)) = map.get("$link") {
                // Attempt to parse the CID from the string value
                Cid::try_from(link.as_str())
                    .map(Some)
                    .map_err(serde::de::Error::custom)
            } else {
                // Return error if "$link" is missing or not a string
                Err(serde::de::Error::custom(
                    "expected \"$link\" field with CID string",
                ))
            }
        }
        Some(BlobFormat::Legacy(buf)) => match buf.tag {
            Some(CBOR_TAG_CID) | None => {
                let mut bz = buf.value.into_vec();

                if bz.first() == Some(&MULTIBASE_IDENTITY) {
                    bz.remove(0);
                }

                Ok(Some(Cid::try_from(bz).map_err(|e| {
                    serde::de::Error::custom(format!("Failed to deserialize Cid: {}", e))
                })?))
            }
            Some(_) => Err(serde::de::Error::custom("unexpected tag")),
        },
    }
}

pub fn default_resource() -> Option<Cid> {
    None
}
