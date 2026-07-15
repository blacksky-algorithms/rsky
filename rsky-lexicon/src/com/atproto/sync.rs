use chrono::{DateTime, Utc};
use lexicon_cid::Cid;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
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
    /// For updates and deletes, the previous record CID. For creates, omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev: Option<Cid>,
}

/// Represents an update of repository state. Note that empty commits are allowed,
/// which include no repo data changes, but an update to rev and signature.
#[derive(Debug, Serialize, Deserialize)]
pub struct SubscribeReposCommit {
    pub seq: i64,
    #[serde(serialize_with = "serialize_datetime_ms")]
    pub time: DateTime<Utc>,
    pub rebase: bool,
    #[serde(rename = "tooBig")]
    pub too_big: bool,
    pub repo: String,
    #[serde(deserialize_with = "deserialize_cid_v1")]
    pub commit: Cid,
    /// DEPRECATED -- unused in sync v1.1. Retained for deserializing legacy events.
    #[serde(
        default = "default_resource",
        deserialize_with = "deserialize_option_cid_v1",
        skip_serializing_if = "Option::is_none"
    )]
    pub prev: Option<Cid>,
    pub rev: String,
    pub since: Option<String>,
    #[serde(with = "serde_bytes")]
    pub blocks: Vec<u8>,
    pub ops: Vec<SubscribeReposCommitOperation>,
    pub blobs: Vec<String>,
    /// The root CID of the MST tree for the previous commit from this repo.
    /// Effectively required for the inductive version of the firehose.
    #[serde(rename = "prevData", default, skip_serializing_if = "Option::is_none")]
    pub prev_data: Option<Cid>,
}

/// Get the current commit CID & revision of the specified repo. Does not require auth.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetLatestCommitOutput {
    pub cid: String,
    pub rev: String,
}

/// DEPRECATED - please use com.atproto.sync.getLatestCommit instead
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetHeadOutput {
    pub root: String,
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
    Deleted,
    Desynchronized,
    Throttled,
}

/// DEPRECATED -- Use #identity event instead
#[derive(Debug, Serialize, Deserialize)]
pub struct SubscribeReposHandle {
    pub did: String,
    pub handle: String,
    pub seq: i64,
    #[serde(serialize_with = "serialize_datetime_ms")]
    pub time: DateTime<Utc>,
}

/// Represents a change to an account's identity. Could be an updated handle, signing key, or
/// pds hosting endpoint. Serves as a prod to all downstream services to refresh their identity cache.
#[derive(Debug, Serialize, Deserialize)]
pub struct SubscribeReposIdentity {
    pub did: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    pub seq: i64,
    #[serde(serialize_with = "serialize_datetime_ms")]
    pub time: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SubscribeReposSync {
    pub seq: i64,
    pub did: String,
    #[serde(with = "serde_bytes")]
    pub blocks: Vec<u8>,
    pub rev: String,
    #[serde(serialize_with = "serialize_datetime_ms")]
    pub time: DateTime<Utc>,
}

/// Represents a change to an account's identity. Could be an updated handle, signing key, or
/// pds hosting endpoint. Serves as a prod to all downstream services to refresh their identity cache.
#[derive(Debug, Serialize, Deserialize)]
pub struct SubscribeReposAccount {
    pub seq: i64,
    pub did: String,
    #[serde(serialize_with = "serialize_datetime_ms")]
    pub time: DateTime<Utc>,
    pub active: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    Desynchronized,
    Throttled,
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
    #[serde(serialize_with = "serialize_datetime_ms")]
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

/// Serializes event timestamps with millisecond precision and a `Z` suffix,
/// matching the reference implementation's `Date.toISOString()` output.
pub fn serialize_datetime_ms<S>(dt: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&format!("{}", dt.format("%Y-%m-%dT%H:%M:%S%.3fZ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    const TEST_CID: &str = "bafkreibjfgx2gprinfvicegelk5kosd6y2frmqpqzwqkg7usac74l3t2v4";

    fn test_time() -> DateTime<Utc> {
        "2026-01-12T19:45:23.307Z".parse::<DateTime<Utc>>().unwrap()
    }

    fn test_commit() -> SubscribeReposCommit {
        SubscribeReposCommit {
            seq: 1,
            time: test_time(),
            rebase: false,
            too_big: false,
            repo: "did:plc:test".to_string(),
            commit: Cid::from_str(TEST_CID).unwrap(),
            prev: None,
            rev: "3jzfcijpj2z2a".to_string(),
            since: None,
            blocks: vec![1, 2, 3],
            ops: vec![SubscribeReposCommitOperation {
                path: "app.bsky.feed.post/3jzfcijpj2z2a".to_string(),
                action: "create".to_string(),
                cid: Some(Cid::from_str(TEST_CID).unwrap()),
                prev: None,
            }],
            blobs: vec![],
            prev_data: None,
        }
    }

    #[test]
    fn commit_omits_deprecated_prev_and_absent_prev_data() {
        let value = serde_json::to_value(test_commit()).unwrap();
        let obj = value.as_object().unwrap();
        assert!(!obj.contains_key("prev"));
        assert!(!obj.contains_key("prevData"));
        assert_eq!(obj["time"], "2026-01-12T19:45:23.307Z");
        let op = value["ops"][0].as_object().unwrap();
        assert!(!op.contains_key("prev"));
        assert!(op.contains_key("cid"));
    }

    #[test]
    fn commit_includes_prev_data_and_op_prev_when_present() {
        let mut commit = test_commit();
        commit.prev_data = Some(Cid::from_str(TEST_CID).unwrap());
        commit.ops[0].prev = Some(Cid::from_str(TEST_CID).unwrap());
        commit.ops[0].action = "update".to_string();
        let value = serde_json::to_value(commit).unwrap();
        assert!(value.as_object().unwrap().contains_key("prevData"));
        assert!(value["ops"][0].as_object().unwrap().contains_key("prev"));
    }

    #[test]
    fn identity_omits_absent_handle() {
        let identity = SubscribeReposIdentity {
            did: "did:plc:test".to_string(),
            handle: None,
            seq: 1,
            time: test_time(),
        };
        let value = serde_json::to_value(identity).unwrap();
        assert!(!value.as_object().unwrap().contains_key("handle"));

        let identity = SubscribeReposIdentity {
            did: "did:plc:test".to_string(),
            handle: Some("alice.test".to_string()),
            seq: 1,
            time: test_time(),
        };
        let value = serde_json::to_value(identity).unwrap();
        assert_eq!(value["handle"], "alice.test");
    }

    #[test]
    fn account_omits_absent_status() {
        let account = SubscribeReposAccount {
            seq: 1,
            did: "did:plc:test".to_string(),
            time: test_time(),
            active: true,
            status: None,
        };
        let value = serde_json::to_value(account).unwrap();
        assert!(!value.as_object().unwrap().contains_key("status"));

        let account = SubscribeReposAccount {
            seq: 1,
            did: "did:plc:test".to_string(),
            time: test_time(),
            active: false,
            status: Some(AccountStatus::Takendown),
        };
        let value = serde_json::to_value(account).unwrap();
        assert_eq!(value["status"], "takendown");
        assert_eq!(AccountStatus::Takendown.to_string(), "Takendown");
    }

    #[test]
    fn sync_serializes_blocks_as_cbor_bytes() {
        let sync = SubscribeReposSync {
            seq: 1,
            did: "did:plc:test".to_string(),
            blocks: vec![1, 2, 3],
            rev: "3jzfcijpj2z2a".to_string(),
            time: test_time(),
        };
        let bytes = serde_ipld_dagcbor::to_vec(&sync).unwrap();
        let decoded: ipld_core::ipld::Ipld = serde_ipld_dagcbor::from_slice(&bytes).unwrap();
        let ipld_core::ipld::Ipld::Map(map) = decoded else {
            panic!("expected map");
        };
        assert!(matches!(
            map.get("blocks"),
            Some(ipld_core::ipld::Ipld::Bytes(b)) if b == &vec![1u8, 2, 3]
        ));
        assert_eq!(
            map.get("time"),
            Some(&ipld_core::ipld::Ipld::String(
                "2026-01-12T19:45:23.307Z".to_string()
            ))
        );
    }

    #[test]
    fn commit_round_trips_through_dag_cbor() {
        let mut commit = test_commit();
        commit.prev_data = Some(Cid::from_str(TEST_CID).unwrap());
        let bytes = serde_ipld_dagcbor::to_vec(&commit).unwrap();
        let decoded: SubscribeReposCommit = serde_ipld_dagcbor::from_slice(&bytes).unwrap();
        assert_eq!(decoded.seq, commit.seq);
        assert_eq!(decoded.prev, None);
        assert_eq!(decoded.prev_data, commit.prev_data);
        assert_eq!(decoded.ops[0].prev, None);
        assert_eq!(decoded.blocks, commit.blocks);
    }
}
