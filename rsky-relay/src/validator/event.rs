use std::borrow::Cow;
use std::cmp::Ordering;
use std::convert::Infallible;
use std::io::Write;
use std::{fmt, io};

use chrono::{DateTime, Utc};
use cid::Cid;
use rs_car_sync::CarDecodeError;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use rsky_common::tid::TID;

use crate::types::Cursor;

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

#[derive(Debug, Error)]
pub enum SerializeError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("header error: {0}")]
    Header(#[from] ciborium::ser::Error<std::io::Error>),
    #[error("body error: {0}")]
    Body(#[from] serde_ipld_dagcbor::EncodeError<std::io::Error>),
}

/// If active=false, this optional field indicates a reason for why the account is not active.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
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
        write!(f, "{self:?}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Commit {
    pub did: String,
    pub rev: TID,
    pub data: Cid,
    pub prev: Option<Cid>, // NOTE: this field is virtually always None
    pub version: u8,       // Should always be 3
    #[serde(with = "serde_bytes", skip_serializing)]
    pub sig: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum SubscribeReposCommitOperation {
    Create { path: String, cid: Cid },
    Update { path: String, cid: Cid, prev_data: Option<Cid> },
    Delete { path: String, prev_data: Option<Cid> },
}

impl PartialEq for SubscribeReposCommitOperation {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.path().eq(other.path())
    }
}

impl Eq for SubscribeReposCommitOperation {}

impl PartialOrd for SubscribeReposCommitOperation {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SubscribeReposCommitOperation {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        self.path().cmp(other.path())
    }
}

impl SubscribeReposCommitOperation {
    const fn path(&self) -> &String {
        match self {
            Self::Create { path, .. } | Self::Update { path, .. } | Self::Delete { path, .. } => {
                path
            }
        }
    }

    pub const fn is_valid(&self) -> bool {
        match self {
            Self::Create { .. } => true,
            Self::Update { prev_data, .. } | Self::Delete { prev_data, .. } => prev_data.is_some(),
        }
    }
}

/// Represents an update of repository state. Note that empty commits are allowed,
/// which include no repo data changes, but an update to rev and signature.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscribeReposCommit {
    pub seq: u64,
    pub rebase: bool,  // NOTE: DEPRECATED
    pub too_big: bool, // NOTE: DEPRECATED
    #[serde(rename = "repo")]
    pub did: String,
    pub commit: Cid,
    pub rev: TID,
    pub since: Option<TID>,
    #[serde(with = "serde_bytes")]
    pub blocks: Vec<u8>,
    pub ops: Vec<SubscribeReposCommitOperation>,
    pub blobs: Vec<String>, // NOTE: DEPRECATED
    pub prev_data: Option<Cid>,
    pub time: DateTime<Utc>,
}

/// Updates the repo to a new state, without necessarily including that state on the firehose.
/// Used to recover from broken commit streams, data loss incidents,
/// or in situations where upstream host does not know recent state of the repository.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscribeReposSync {
    pub seq: u64,
    pub did: String,
    #[serde(with = "serde_bytes")]
    pub blocks: Vec<u8>,
    pub rev: TID,
    pub time: DateTime<Utc>,
}

/// Represents a change to an account's identity. Could be an updated handle, signing key, or pds
/// hosting endpoint. Serves as a prod to all downstream services to refresh their identity cache.
#[derive(Debug, Serialize, Deserialize)]
pub struct SubscribeReposIdentity {
    pub seq: u64,
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
    pub seq: u64,
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

#[expect(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum SubscribeReposEvent {
    Commit(SubscribeReposCommit),
    Sync(SubscribeReposSync),
    Identity(SubscribeReposIdentity),
    Account(SubscribeReposAccount),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Header<'a> {
    #[serde(rename = "t", default)]
    pub type_: Cow<'a, str>,
    #[serde(rename = "op")]
    pub operation_: i8,
}

impl SubscribeReposEvent {
    pub fn parse(data: &[u8]) -> Result<Option<Self>, ParseError> {
        let mut reader = io::Cursor::new(data);

        let header = match ciborium::de::from_reader::<Header<'static>, _>(&mut reader) {
            Ok(header) => header,
            Err(err) => {
                return Err(err.into());
            }
        };
        if header.operation_ == -1 {
            return Ok(None);
        }
        let body = match header.type_.as_ref() {
            "#commit" => Self::Commit(serde_ipld_dagcbor::from_reader(&mut reader)?),
            "#sync" => Self::Sync(serde_ipld_dagcbor::from_reader(&mut reader)?),
            "#identity" => Self::Identity(serde_ipld_dagcbor::from_reader(&mut reader)?),
            "#account" => Self::Account(serde_ipld_dagcbor::from_reader(&mut reader)?),
            "#info" => {
                let info = serde_ipld_dagcbor::from_reader::<SubscribeReposInfo, _>(&mut reader)?;
                tracing::debug!(name = %info.name, message = %info.message, "received #info");
                return Ok(None);
            }
            _ => {
                return Err(ParseError::UnknownType(header.type_.into_owned()));
            }
        };

        Ok(Some(body))
    }

    pub fn serialize(self, capacity: usize, seq: Cursor) -> Result<Vec<u8>, SerializeError> {
        let mut writer = io::Cursor::new(Vec::with_capacity(capacity));
        #[expect(clippy::cast_sign_loss)]
        let time = self.time().timestamp() as u64;
        writer.write_all(&time.to_be_bytes())?;

        let header = Header { operation_: 1, type_: Cow::Borrowed(self.type_()) };
        ciborium::ser::into_writer(&header, &mut writer)?;

        match self {
            Self::Commit(mut commit) => {
                commit.seq = seq.get();
                serde_ipld_dagcbor::to_writer(&mut writer, &commit)?;
            }
            Self::Sync(mut sync) => {
                sync.seq = seq.get();
                serde_ipld_dagcbor::to_writer(&mut writer, &sync)?;
            }
            Self::Identity(mut identity) => {
                identity.seq = seq.get();
                serde_ipld_dagcbor::to_writer(&mut writer, &identity)?;
            }
            Self::Account(mut account) => {
                account.seq = seq.get();
                serde_ipld_dagcbor::to_writer(&mut writer, &account)?;
            }
        };

        Ok(writer.into_inner())
    }

    pub const fn type_(&self) -> &'static str {
        match self {
            Self::Commit(_) => "#commit",
            Self::Sync(_) => "#sync",
            Self::Identity(_) => "#identity",
            Self::Account(_) => "#account",
        }
    }

    pub fn seq(&self) -> Cursor {
        match self {
            Self::Commit(commit) => commit.seq,
            Self::Sync(sync) => sync.seq,
            Self::Identity(identity) => identity.seq,
            Self::Account(account) => account.seq,
        }
        .into()
    }

    pub const fn time(&self) -> DateTime<Utc> {
        match self {
            Self::Commit(commit) => commit.time,
            Self::Sync(sync) => sync.time,
            Self::Identity(identity) => identity.time,
            Self::Account(account) => account.time,
        }
    }

    pub fn did(&self) -> &str {
        match self {
            Self::Commit(commit) => &commit.did,
            Self::Sync(sync) => &sync.did,
            Self::Identity(identity) => &identity.did,
            Self::Account(account) => &account.did,
        }
    }
}
