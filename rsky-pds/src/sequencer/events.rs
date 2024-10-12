use crate::account_manager::helpers::account::AccountStatus;
use crate::car::read_car_bytes;
use crate::common;
use crate::common::struct_to_cbor;
use crate::models::models;
use crate::repo::block_map::BlockMap;
use crate::repo::cid_set::CidSet;
use crate::repo::types::{CommitData, PreparedWrite};
use crate::repo::util::format_data_key;
use anyhow::Result;
use lexicon_cid::Cid;
use rsky_lexicon::com::atproto::sync::AccountStatus as LexiconAccountStatus;
use rsky_syntax::aturi::AtUri;
use serde::de::Error as DeserializerError;
use serde::{Deserialize, Deserializer};

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub enum CommitEvtOpAction {
    Create,
    Update,
    Delete,
}

impl CommitEvtOpAction {
    pub fn to_string(&self) -> String {
        match self {
            CommitEvtOpAction::Create => "create".to_string(),
            CommitEvtOpAction::Update => "update".to_string(),
            CommitEvtOpAction::Delete => "delete".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct CommitEvtOp {
    pub action: CommitEvtOpAction,
    pub path: String,
    pub cid: Option<Cid>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct CommitEvt {
    pub rebase: bool,
    pub too_big: bool,
    pub repo: String,
    pub commit: Cid,
    pub prev: Option<Cid>,
    pub rev: String,
    pub since: Option<String>,
    pub blocks: Vec<u8>,
    pub ops: Vec<CommitEvtOp>,
    pub blobs: Vec<Cid>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct HandleEvt {
    pub did: String,
    pub handle: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct IdentityEvt {
    pub did: String,
    pub handle: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct AccountEvt {
    pub did: String,
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<LexiconAccountStatus>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct TombstoneEvt {
    pub did: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct TypedCommitEvt {
    pub r#type: String, // 'commit'
    pub seq: i64,
    pub time: String,
    pub evt: CommitEvt,
}

impl Default for TypedCommitEvt {
    fn default() -> Self {
        Self {
            r#type: "commit".to_string(),
            seq: 0,
            time: common::now(),
            evt: CommitEvt {
                rebase: false,
                too_big: false,
                repo: "".to_string(),
                commit: Default::default(),
                prev: None,
                rev: "".to_string(),
                since: None,
                blocks: vec![],
                ops: vec![],
                blobs: vec![],
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct TypedHandleEvt {
    pub r#type: String, // 'handle'
    pub seq: i64,
    pub time: String,
    pub evt: HandleEvt,
}

impl Default for TypedHandleEvt {
    fn default() -> Self {
        Self {
            r#type: "handle".to_string(),
            seq: 0,
            time: common::now(),
            evt: HandleEvt {
                did: "".to_string(),
                handle: "".to_string(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct TypedIdentityEvt {
    pub r#type: String, // 'identity'
    pub seq: i64,
    pub time: String,
    pub evt: IdentityEvt,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct TypedAccountEvt {
    pub r#type: String, // 'account'
    pub seq: i64,
    pub time: String,
    pub evt: AccountEvt,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct TypedTombstoneEvt {
    pub r#type: String, // 'tombstone'
    pub seq: i64,
    pub time: String,
    pub evt: TombstoneEvt,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum SeqEvt {
    TypedCommitEvt(TypedCommitEvt),
    TypedHandleEvt(TypedHandleEvt),
    TypedIdentityEvt(TypedIdentityEvt),
    TypedAccountEvt(TypedAccountEvt),
    TypedTombstoneEvt(TypedTombstoneEvt),
}

impl<'de> Deserialize<'de> for SeqEvt {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        // Implement logic to determine the correct variant based on the "type" field
        // and deserialize accordingly

        // Example:
        if let Some(typ) = value.get("type") {
            match typ.as_str() {
                Some("commit") => Ok(SeqEvt::TypedCommitEvt(
                    serde_json::from_value(value).map_err(DeserializerError::custom)?,
                )),
                Some("handle") => Ok(SeqEvt::TypedHandleEvt(
                    serde_json::from_value(value).map_err(DeserializerError::custom)?,
                )),
                Some("identity") => Ok(SeqEvt::TypedIdentityEvt(
                    serde_json::from_value(value).map_err(DeserializerError::custom)?,
                )),
                Some("account") => Ok(SeqEvt::TypedAccountEvt(
                    serde_json::from_value(value).map_err(DeserializerError::custom)?,
                )),
                Some("tombstone") => Ok(SeqEvt::TypedTombstoneEvt(
                    serde_json::from_value(value).map_err(DeserializerError::custom)?,
                )),
                _ => Err(DeserializerError::custom("Unknown event type")),
            }
        } else {
            Err(DeserializerError::missing_field("type"))
        }
    }
}

impl SeqEvt {
    pub fn seq(&self) -> i64 {
        match self {
            SeqEvt::TypedCommitEvt(this) => this.seq,
            SeqEvt::TypedHandleEvt(this) => this.seq,
            SeqEvt::TypedIdentityEvt(this) => this.seq,
            SeqEvt::TypedAccountEvt(this) => this.seq,
            SeqEvt::TypedTombstoneEvt(this) => this.seq,
        }
    }
}

pub async fn format_seq_commit(
    did: String,
    commit_data: CommitData,
    writes: Vec<PreparedWrite>,
) -> Result<models::RepoSeq> {
    let too_big: bool;
    let mut ops: Vec<CommitEvtOp> = Vec::new();
    let mut blobs = CidSet::new(None);
    let car_slice: Vec<u8>;

    if writes.len() > 200 || commit_data.new_blocks.byte_size()? > 1000000 {
        too_big = true;
        let mut just_root = BlockMap::new();
        just_root.add(commit_data.new_blocks.get(commit_data.cid))?;
        car_slice = read_car_bytes(Some(&commit_data.cid), just_root).await?;
    } else {
        too_big = false;
        for w in writes {
            let uri = AtUri::new(w.uri().clone(), None)?;
            let path = format_data_key(uri.get_collection(), uri.get_rkey());
            let cid: Option<Cid>;
            let action: CommitEvtOpAction;
            match w {
                PreparedWrite::Create(w) => {
                    cid = Some(w.cid);
                    for blob in w.blobs {
                        blobs.add(blob.cid);
                    }
                    action = CommitEvtOpAction::Create;
                }
                PreparedWrite::Update(w) => {
                    cid = Some(w.cid);
                    for blob in w.blobs {
                        blobs.add(blob.cid);
                    }
                    action = CommitEvtOpAction::Update;
                }
                PreparedWrite::Delete(_) => {
                    cid = None;
                    action = CommitEvtOpAction::Delete;
                }
            }
            ops.push(CommitEvtOp { action, path, cid });
        }
        car_slice = read_car_bytes(Some(&commit_data.cid), commit_data.new_blocks).await?;
    }

    let evt = CommitEvt {
        rebase: false,
        too_big,
        repo: did.clone(),
        commit: commit_data.cid,
        prev: commit_data.prev,
        rev: commit_data.rev,
        since: commit_data.since,
        ops,
        blocks: car_slice,
        blobs: blobs.to_list(),
    };
    Ok(models::RepoSeq::new(
        did,
        "append".to_string(),
        struct_to_cbor(evt)?,
        common::now(),
    ))
}

pub async fn format_seq_handle_update(did: String, handle: String) -> Result<models::RepoSeq> {
    let evt = HandleEvt {
        did: did.clone(),
        handle,
    };
    Ok(models::RepoSeq::new(
        did,
        "handle".to_string(),
        struct_to_cbor(evt)?,
        common::now(),
    ))
}

pub async fn format_seq_identity_evt(
    did: String,
    handle: Option<String>,
) -> Result<models::RepoSeq> {
    let mut evt = IdentityEvt {
        did: did.clone(),
        handle: None,
    };
    if let Some(handle) = handle {
        evt.handle = Some(handle);
    }
    Ok(models::RepoSeq::new(
        did,
        "identity".to_string(),
        struct_to_cbor(evt)?,
        common::now(),
    ))
}

pub async fn format_seq_account_evt(did: String, status: AccountStatus) -> Result<models::RepoSeq> {
    let mut evt = AccountEvt {
        did: did.clone(),
        active: matches!(status, AccountStatus::Active),
        status: None,
    };
    if !matches!(status, AccountStatus::Active) {
        evt.status = Some(match status {
            AccountStatus::Takendown => LexiconAccountStatus::Takendown,
            AccountStatus::Suspended => LexiconAccountStatus::Suspended,
            AccountStatus::Deleted => LexiconAccountStatus::Deleted,
            AccountStatus::Deactivated => LexiconAccountStatus::Deactivated,
            _ => panic!("Conditional failed and allowed an invalid account status."),
        });
    }

    Ok(models::RepoSeq::new(
        did,
        "account".to_string(),
        struct_to_cbor(evt)?,
        common::now(),
    ))
}

pub async fn format_seq_tombstone(did: String) -> Result<models::RepoSeq> {
    let evt = TombstoneEvt { did: did.clone() };
    Ok(models::RepoSeq::new(
        did,
        "tombstone".to_string(),
        struct_to_cbor(evt)?,
        common::now(),
    ))
}
