use std::borrow::Cow;
use std::cmp::Ordering;
use std::convert::Infallible;
use std::{fmt, io};

use chrono::{DateTime, Utc};
use cid::Cid;
use rs_car_sync::CarDecodeError;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use vec1::Vec1;

use rsky_common::tid::TID;

use crate::types::Cursor;

pub type DidEndpoint = Option<Box<str>>;
pub type DidKey = [u8; 35];

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("header error: {0}")]
    Header(#[from] ciborium::de::Error<std::io::Error>),
    #[error("body error: {0}")]
    Body(#[from] serde_ipld_dagcbor::DecodeError<std::io::Error>),
    #[error("chrono error: {0}")]
    Chrono(#[from] chrono::ParseError),
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

    #[cfg(not(feature = "labeler"))]
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
    #[serde(default)]
    pub rebase: bool, // NOTE: DEPRECATED
    #[serde(default)]
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

/// Subscribe to stream of labels (and negations). Public endpoint implemented by mod services.
/// Uses same sequencing scheme as repo event stream.
#[derive(Debug, Serialize, Deserialize)]
pub struct SubscribeLabels {
    pub seq: u64,
    pub labels: Vec1<SubscribeLabel>,
}

/// Metadata tag on an atproto resource (eg, repo or record).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscribeLabel {
    /// The AT Protocol version of the label object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ver: Option<u8>,
    /// DID of the actor who created this label.
    pub src: String,
    /// AT URI of the record, repository (account), or other resource that this label applies to.
    pub uri: String,
    /// Optionally, CID specifying the specific version of 'uri' resource this label applies to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cid: Option<String>,
    /// The short string name of the value or type of this label.
    pub val: String,
    /// If true, this is a negation label, overwriting a previous label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub neg: Option<bool>,
    /// Timestamp when this label was created.
    pub cts: String,
    #[serde(skip)]
    pub cts_dt: DateTime<Utc>,
    /// Timestamp at which this label expires (no longer applies).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<String>,
    /// Signature of dag-cbor encoded label.
    #[serde(with = "serde_bytes", skip_serializing_if = "Option::is_none")]
    pub sig: Option<Vec<u8>>,
}

#[expect(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum SubscribeReposEvent {
    Commit(SubscribeReposCommit),
    Sync(SubscribeReposSync),
    Identity(SubscribeReposIdentity),
    Account(SubscribeReposAccount),
    Labels(SubscribeLabels),
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
            "#labels" => {
                let mut labels: SubscribeLabels = serde_ipld_dagcbor::from_reader(&mut reader)?;
                for label in &mut labels.labels {
                    label.cts_dt = label.cts.parse()?;
                }
                Self::Labels(labels)
            }
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
            Self::Labels(mut labels) => {
                labels.seq = seq.get();
                serde_ipld_dagcbor::to_writer(&mut writer, &labels)?;
            }
        }

        Ok(writer.into_inner())
    }

    pub const fn type_(&self) -> &'static str {
        match self {
            Self::Commit(_) => "#commit",
            Self::Sync(_) => "#sync",
            Self::Identity(_) => "#identity",
            Self::Account(_) => "#account",
            Self::Labels(_) => "#labels",
        }
    }

    pub fn seq(&self) -> Cursor {
        match self {
            Self::Commit(commit) => commit.seq,
            Self::Sync(sync) => sync.seq,
            Self::Identity(identity) => identity.seq,
            Self::Account(account) => account.seq,
            Self::Labels(labels) => labels.seq,
        }
        .into()
    }

    pub fn time(&self) -> DateTime<Utc> {
        match self {
            Self::Commit(commit) => commit.time,
            Self::Sync(sync) => sync.time,
            Self::Identity(identity) => identity.time,
            Self::Account(account) => account.time,
            Self::Labels(labels) => labels.labels.last().cts_dt,
        }
    }

    pub fn did(&self) -> &str {
        match self {
            Self::Commit(commit) => &commit.did,
            Self::Sync(sync) => &sync.did,
            Self::Identity(identity) => &identity.did,
            Self::Account(account) => &account.did,
            Self::Labels(labels) => &labels.labels.last().src,
        }
    }

    #[cfg(feature = "labeler")]
    pub fn commit(&self) -> Result<Option<(&[SubscribeLabel], ())>, ParseError> {
        match self {
            Self::Labels(labels) => Ok(Some((&labels.labels, ()))),
            _ => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_time() -> DateTime<Utc> {
        // Deterministic timestamp with millisecond precision (matches AT spec).
        DateTime::parse_from_rfc3339("2026-01-12T19:45:23.307Z").unwrap().with_timezone(&Utc)
    }

    fn empty_cid() -> Cid {
        // Smallest valid CID: dag-cbor codec, sha256 of empty bytes.
        Cid::try_from("bafyreigbtj4x7ip5legnfznufuopl4sg4knzc2cof6duas4b3q2fy6swua").unwrap()
    }

    fn tid() -> TID {
        TID::new("3kqcb45gzpk2c".to_owned()).unwrap()
    }

    fn make_commit() -> SubscribeReposCommit {
        SubscribeReposCommit {
            seq: 0,
            rebase: false,
            too_big: false,
            did: "did:plc:test".to_owned(),
            commit: empty_cid(),
            rev: tid(),
            since: None,
            blocks: vec![1, 2, 3, 4],
            ops: vec![SubscribeReposCommitOperation::Create {
                path: "app.bsky.feed.post/abc".to_owned(),
                cid: empty_cid(),
            }],
            blobs: vec![],
            prev_data: None,
            time: fixed_time(),
        }
    }

    fn make_sync() -> SubscribeReposSync {
        SubscribeReposSync {
            seq: 0,
            did: "did:plc:test".to_owned(),
            blocks: vec![9, 9, 9],
            rev: tid(),
            time: fixed_time(),
        }
    }

    fn make_identity() -> SubscribeReposIdentity {
        SubscribeReposIdentity {
            seq: 0,
            did: "did:plc:test".to_owned(),
            time: fixed_time(),
            handle: Some("alice.test".to_owned()),
        }
    }

    fn make_account() -> SubscribeReposAccount {
        SubscribeReposAccount {
            seq: 0,
            did: "did:plc:test".to_owned(),
            time: fixed_time(),
            active: false,
            status: Some(AccountStatus::Deactivated),
        }
    }

    fn make_label() -> SubscribeLabel {
        // Labels in the wild always carry sig; the deserializer requires it (no #[serde(default)]).
        SubscribeLabel {
            ver: Some(1),
            src: "did:plc:labeler".to_owned(),
            uri: "at://did:plc:test/app.bsky.feed.post/abc".to_owned(),
            cid: Some("bafyreigbtj4x7ip5legnfznufuopl4sg4knzc2cof6duas4b3q2fy6swua".to_owned()),
            val: "spam".to_owned(),
            neg: Some(false),
            cts: "2026-01-12T19:45:23.307Z".to_owned(),
            cts_dt: fixed_time(),
            exp: Some("2027-01-01T00:00:00.000Z".to_owned()),
            sig: Some(vec![0u8; 64]),
        }
    }

    fn make_labels() -> SubscribeLabels {
        SubscribeLabels { seq: 0, labels: Vec1::new(make_label()) }
    }

    #[test]
    fn commit_round_trip() {
        let event = SubscribeReposEvent::Commit(make_commit());
        let bytes = event.serialize(64, Cursor::from(42)).unwrap();
        let parsed = SubscribeReposEvent::parse(&bytes).unwrap().unwrap();
        match parsed {
            SubscribeReposEvent::Commit(c) => {
                assert_eq!(c.seq, 42);
                assert_eq!(c.did, "did:plc:test");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn sync_round_trip() {
        let event = SubscribeReposEvent::Sync(make_sync());
        let bytes = event.serialize(64, Cursor::from(7)).unwrap();
        let parsed = SubscribeReposEvent::parse(&bytes).unwrap().unwrap();
        assert!(matches!(parsed, SubscribeReposEvent::Sync(s) if s.seq == 7));
    }

    #[test]
    fn identity_round_trip() {
        let event = SubscribeReposEvent::Identity(make_identity());
        let bytes = event.serialize(64, Cursor::from(8)).unwrap();
        let parsed = SubscribeReposEvent::parse(&bytes).unwrap().unwrap();
        assert!(matches!(parsed, SubscribeReposEvent::Identity(i) if i.seq == 8));
    }

    #[test]
    fn account_round_trip() {
        let event = SubscribeReposEvent::Account(make_account());
        let bytes = event.serialize(64, Cursor::from(9)).unwrap();
        let parsed = SubscribeReposEvent::parse(&bytes).unwrap().unwrap();
        assert!(matches!(parsed, SubscribeReposEvent::Account(a)
            if a.seq == 9 && a.status == Some(AccountStatus::Deactivated)));
    }

    #[test]
    fn labels_round_trip_populates_cts_dt() {
        let event = SubscribeReposEvent::Labels(make_labels());
        let bytes = event.serialize(64, Cursor::from(10)).unwrap();
        let parsed = SubscribeReposEvent::parse(&bytes).unwrap().unwrap();
        match parsed {
            SubscribeReposEvent::Labels(l) => {
                assert_eq!(l.seq, 10);
                assert_eq!(l.labels.last().val, "spam");
                // parse() must populate cts_dt from cts string.
                assert_eq!(l.labels.last().cts_dt, fixed_time());
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_op_minus_one_returns_none() {
        // Header { type: "", op: -1 } -> ciborium-encoded.
        let mut buf = Vec::new();
        ciborium::ser::into_writer(&Header { type_: Cow::Borrowed(""), operation_: -1 }, &mut buf)
            .unwrap();
        assert!(SubscribeReposEvent::parse(&buf).unwrap().is_none());
    }

    #[test]
    fn parse_info_returns_none() {
        let mut buf = Vec::new();
        ciborium::ser::into_writer(
            &Header { type_: Cow::Borrowed("#info"), operation_: 1 },
            &mut buf,
        )
        .unwrap();
        let info =
            SubscribeReposInfo { name: "OutdatedCursor".to_owned(), message: "msg".to_owned() };
        serde_ipld_dagcbor::to_writer(&mut buf, &info).unwrap();
        assert!(SubscribeReposEvent::parse(&buf).unwrap().is_none());
    }

    #[test]
    fn parse_unknown_type_errors() {
        let mut buf = Vec::new();
        ciborium::ser::into_writer(
            &Header { type_: Cow::Borrowed("#bogus"), operation_: 1 },
            &mut buf,
        )
        .unwrap();
        assert!(matches!(
            SubscribeReposEvent::parse(&buf),
            Err(ParseError::UnknownType(t)) if t == "#bogus"
        ));
    }

    #[test]
    fn parse_garbage_header_errors() {
        // Truncated input -> ciborium header decode error.
        assert!(matches!(SubscribeReposEvent::parse(&[]), Err(ParseError::Header(_))));
    }

    #[test]
    fn parse_bad_body_errors() {
        let mut buf = Vec::new();
        ciborium::ser::into_writer(
            &Header { type_: Cow::Borrowed("#commit"), operation_: 1 },
            &mut buf,
        )
        .unwrap();
        // body intentionally missing -> dag-cbor decode error
        assert!(matches!(SubscribeReposEvent::parse(&buf), Err(ParseError::Body(_))));
    }

    #[test]
    fn type_str_is_correct_for_each_variant() {
        assert_eq!(SubscribeReposEvent::Commit(make_commit()).type_(), "#commit");
        assert_eq!(SubscribeReposEvent::Sync(make_sync()).type_(), "#sync");
        assert_eq!(SubscribeReposEvent::Identity(make_identity()).type_(), "#identity");
        assert_eq!(SubscribeReposEvent::Account(make_account()).type_(), "#account");
        assert_eq!(SubscribeReposEvent::Labels(make_labels()).type_(), "#labels");
    }

    #[test]
    fn seq_returns_per_variant_seq() {
        let mut commit = make_commit();
        commit.seq = 1;
        assert_eq!(SubscribeReposEvent::Commit(commit).seq().get(), 1);
        let mut sync = make_sync();
        sync.seq = 2;
        assert_eq!(SubscribeReposEvent::Sync(sync).seq().get(), 2);
        let mut identity = make_identity();
        identity.seq = 3;
        assert_eq!(SubscribeReposEvent::Identity(identity).seq().get(), 3);
        let mut account = make_account();
        account.seq = 4;
        assert_eq!(SubscribeReposEvent::Account(account).seq().get(), 4);
        let mut labels = make_labels();
        labels.seq = 5;
        assert_eq!(SubscribeReposEvent::Labels(labels).seq().get(), 5);
    }

    #[test]
    fn time_returns_per_variant_time() {
        let t = fixed_time();
        assert_eq!(SubscribeReposEvent::Commit(make_commit()).time(), t);
        assert_eq!(SubscribeReposEvent::Sync(make_sync()).time(), t);
        assert_eq!(SubscribeReposEvent::Identity(make_identity()).time(), t);
        assert_eq!(SubscribeReposEvent::Account(make_account()).time(), t);
        assert_eq!(SubscribeReposEvent::Labels(make_labels()).time(), t);
    }

    #[test]
    fn did_returns_per_variant_subject() {
        assert_eq!(SubscribeReposEvent::Commit(make_commit()).did(), "did:plc:test");
        assert_eq!(SubscribeReposEvent::Sync(make_sync()).did(), "did:plc:test");
        assert_eq!(SubscribeReposEvent::Identity(make_identity()).did(), "did:plc:test");
        assert_eq!(SubscribeReposEvent::Account(make_account()).did(), "did:plc:test");
        assert_eq!(SubscribeReposEvent::Labels(make_labels()).did(), "did:plc:labeler");
    }

    #[test]
    fn account_status_display_matches_debug() {
        assert_eq!(format!("{}", AccountStatus::Takendown), "Takendown");
        assert_eq!(format!("{}", AccountStatus::Suspended), "Suspended");
        assert_eq!(format!("{}", AccountStatus::Deleted), "Deleted");
        assert_eq!(format!("{}", AccountStatus::Deactivated), "Deactivated");
        assert_eq!(format!("{}", AccountStatus::Desynchronized), "Desynchronized");
        assert_eq!(format!("{}", AccountStatus::Throttled), "Throttled");
    }

    #[test]
    fn commit_op_path_and_eq() {
        let a = SubscribeReposCommitOperation::Create { path: "p1".to_owned(), cid: empty_cid() };
        let b = SubscribeReposCommitOperation::Update {
            path: "p1".to_owned(),
            cid: empty_cid(),
            prev_data: None,
        };
        let c = SubscribeReposCommitOperation::Delete { path: "p2".to_owned(), prev_data: None };
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a < c);
        assert_eq!(a.partial_cmp(&c), Some(Ordering::Less));
    }

    #[cfg(not(feature = "labeler"))]
    #[test]
    fn commit_op_is_valid() {
        assert!(
            SubscribeReposCommitOperation::Create { path: "p".to_owned(), cid: empty_cid() }
                .is_valid()
        );
        assert!(
            SubscribeReposCommitOperation::Update {
                path: "p".to_owned(),
                cid: empty_cid(),
                prev_data: Some(empty_cid()),
            }
            .is_valid()
        );
        assert!(
            !SubscribeReposCommitOperation::Update {
                path: "p".to_owned(),
                cid: empty_cid(),
                prev_data: None,
            }
            .is_valid()
        );
        assert!(
            SubscribeReposCommitOperation::Delete {
                path: "p".to_owned(),
                prev_data: Some(empty_cid()),
            }
            .is_valid()
        );
        assert!(
            !SubscribeReposCommitOperation::Delete { path: "p".to_owned(), prev_data: None }
                .is_valid()
        );
    }
}
