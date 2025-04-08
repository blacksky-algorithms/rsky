use std::convert::Infallible;
use std::fmt;
use std::io::Cursor;

use chrono::{DateTime, Utc};
use cid::Cid;
use rs_car_sync::{CarDecodeError, CarReader};
use serde::{Deserialize, Deserializer, Serialize};
use serde_cbor::tags::Tagged;
use thiserror::Error;

const CBOR_TAG_CID: u64 = 42;
const MULTIBASE_IDENTITY: u8 = 0;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("header error: {0}")]
    Header(#[from] ciborium::de::Error<std::io::Error>),
    #[error("body error: {0}")]
    Body(#[from] serde_ipld_dagcbor::DecodeError<std::io::Error>),
    #[error("car error: {0}")]
    Car(#[from] CarDecodeError),
    #[error("missing root: {0}")]
    MissingRoot(rs_car_sync::Cid),
    #[error("commit error: {0}")]
    Commit(#[from] serde_ipld_dagcbor::DecodeError<Infallible>),
    #[error("unknown type: {0}")]
    UnknownType(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Create,
    Update,
    Delete,
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
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Commit {
    pub did: String,
    pub rev: String,
    pub data: Cid,
    pub prev: Option<Cid>,
    pub version: u8, // Should be 3
    #[serde(with = "serde_bytes", skip_serializing)]
    pub sig: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SubscribeReposCommitOperation {
    pub action: Action,
    pub path: String,
    pub cid: Option<Cid>,
    #[serde(default = "default_resource", deserialize_with = "deserialize_option_cid_v1")]
    pub prev_data: Option<Cid>,
}

/// Represents an update of repository state. Note that empty commits are allowed,
/// which include no repo data changes, but an update to rev and signature.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscribeReposCommit {
    pub seq: i64,
    pub rebase: bool,
    pub too_big: bool,
    pub repo: String,
    #[serde(deserialize_with = "deserialize_cid_v1")]
    pub commit: Cid,
    pub rev: String,
    pub since: Option<String>,
    #[serde(with = "serde_bytes")]
    pub blocks: Vec<u8>,
    pub ops: Vec<SubscribeReposCommitOperation>,
    pub blobs: Vec<String>,
    #[serde(default = "default_resource", deserialize_with = "deserialize_option_cid_v1")]
    pub prev_data: Option<Cid>,
    pub time: DateTime<Utc>,
}

/// Updates the repo to a new state, without necessarily including that state on the firehose.
/// Used to recover from broken commit streams, data loss incidents,
/// or in situations where upstream host does not know recent state of the repository.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscribeReposSync {
    pub seq: i64,
    pub did: String,
    #[serde(with = "serde_bytes")]
    pub blocks: Vec<u8>,
    pub rev: String,
    pub time: DateTime<Utc>,
}

/// Represents a change to an account's identity. Could be an updated handle, signing key, or pds
/// hosting endpoint. Serves as a prod to all downstream services to refresh their identity cache.
#[derive(Debug, Serialize, Deserialize)]
pub struct SubscribeReposIdentity {
    pub seq: i64,
    pub did: String,
    pub time: DateTime<Utc>,
    pub handle: Option<String>,
}

/// Represents a change to an account's status on a host (eg, PDS or Relay).
/// The semantics of this event are that the status is at the host which emitted the event,
/// not necessarily that at the currently active PDS.
/// Eg, a Relay takedown would emit a takedown with active=false, even if the PDS is still active.
#[derive(Debug, Serialize, Deserialize)]
pub struct SubscribeReposAccount {
    pub seq: i64,
    pub did: String,
    pub time: DateTime<Utc>,
    pub active: bool,
    pub status: Option<AccountStatus>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SubscribeReposInfo {
    pub name: String,
    pub message: String,
}

#[derive(Debug)]
pub enum SubscribeReposEvent {
    Commit(SubscribeReposCommit),
    Sync(SubscribeReposSync),
    Identity(SubscribeReposIdentity),
    Account(SubscribeReposAccount),
}

impl SubscribeReposEvent {
    pub fn parse(data: &[u8]) -> Result<Option<SubscribeReposEvent>, ParseError> {
        #[derive(Debug, Deserialize)]
        pub struct Header {
            #[serde(rename(deserialize = "t"))]
            pub type_: String,
            #[serde(rename(deserialize = "op"))]
            pub _operation: u8,
        }

        let mut reader = Cursor::new(data);

        let header = ciborium::de::from_reader::<Header, _>(&mut reader)?;
        let body = match header.type_.as_str() {
            "#commit" => SubscribeReposEvent::Commit(serde_ipld_dagcbor::from_reader(&mut reader)?),
            "#sync" => SubscribeReposEvent::Sync(serde_ipld_dagcbor::from_reader(&mut reader)?),
            "#identity" => {
                SubscribeReposEvent::Identity(serde_ipld_dagcbor::from_reader(&mut reader)?)
            }
            "#account" => {
                SubscribeReposEvent::Account(serde_ipld_dagcbor::from_reader(&mut reader)?)
            }
            "#info" => {
                let info = serde_ipld_dagcbor::from_reader::<SubscribeReposInfo, _>(&mut reader)?;
                tracing::debug!("received info: {} ({})", info.name, info.message);
                return Ok(None);
            }
            _ => {
                tracing::debug!("received unknown header {:?}", header.type_.as_str());
                return Err(ParseError::UnknownType(header.type_));
            }
        };

        Ok(Some(body))
    }

    pub fn id(&self) -> i64 {
        match self {
            SubscribeReposEvent::Commit(commit) => commit.seq,
            SubscribeReposEvent::Sync(sync) => sync.seq,
            SubscribeReposEvent::Identity(identity) => identity.seq,
            SubscribeReposEvent::Account(account) => account.seq,
        }
    }

    pub fn did(&self) -> &str {
        match self {
            SubscribeReposEvent::Commit(commit) => &commit.repo,
            SubscribeReposEvent::Sync(sync) => &sync.did,
            SubscribeReposEvent::Identity(identity) => &identity.did,
            SubscribeReposEvent::Account(account) => &account.did,
        }
    }

    pub fn commit(&self) -> Result<Option<Commit>, ParseError> {
        let mut blocks = match self {
            SubscribeReposEvent::Commit(commit) => commit.blocks.as_slice(),
            SubscribeReposEvent::Sync(sync) => sync.blocks.as_slice(),
            SubscribeReposEvent::Identity(_) | SubscribeReposEvent::Account(_) => {
                return Ok(None);
            }
        };
        let reader = CarReader::new(&mut blocks, true)?;
        let root_cid = reader.header.roots[0];
        for next in reader {
            let (cid, block) = next?;
            if cid == root_cid {
                return Ok(serde_ipld_dagcbor::from_slice(&block)?);
            }
        }
        Err(ParseError::MissingRoot(root_cid))
    }
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
        Link(serde_json::Map<String, serde_json::Value>),
    }

    // Deserialize into an optional map, expecting an object like {"$link": "cid_string"}
    let opt_blob = Option::<BlobFormat>::deserialize(deserializer)?;

    match opt_blob {
        // If there's no object, return None
        None => Ok(None),
        Some(BlobFormat::Link(map)) => {
            // Check if the map contains the "$link" key
            if let Some(serde_json::Value::String(link)) = map.get("$link") {
                // Attempt to parse the CID from the string value
                Cid::try_from(link.as_str()).map(Some).map_err(serde::de::Error::custom)
            } else {
                // Return error if "$link" is missing or not a string
                Err(serde::de::Error::custom("expected \"$link\" field with CID string"))
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
