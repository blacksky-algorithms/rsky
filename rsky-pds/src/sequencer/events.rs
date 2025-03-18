use crate::account_manager::helpers::account::AccountStatus;
use crate::actor_store::repo::types::SyncEvtData;
use crate::models::models;
use anyhow::Result;
use atrium_api::app::bsky::graph::block;
use lexicon_cid::Cid;
use rsky_common;
use rsky_common::struct_to_cbor;
use rsky_lexicon::com::atproto::sync::AccountStatus as LexiconAccountStatus;
use rsky_repo::block_map::BlockMap;
use rsky_repo::car::blocks_to_car_file;
use rsky_repo::cid_set::CidSet;
use rsky_repo::types::{CommitData, CommitDataWithOps, PreparedWrite};
use rsky_repo::util::format_data_key;
use rsky_syntax::aturi::AtUri;
use serde::de::Error as DeserializerError;
use serde::{Deserialize, Deserializer};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CommitEvtOpAction {
    Create,
    Update,
    Delete,
}

impl fmt::Display for CommitEvtOpAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Match each variant and write its lowercase representation.
        match self {
            CommitEvtOpAction::Create => write!(f, "create"),
            CommitEvtOpAction::Update => write!(f, "update"),
            CommitEvtOpAction::Delete => write!(f, "delete"),
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
    #[serde(rename = "tooBig")]
    pub too_big: bool,
    pub repo: String,
    pub commit: Cid,
    pub prev: Option<Cid>,
    pub rev: String,
    pub since: Option<String>,
    #[serde(with = "serde_bytes")]
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
pub struct SyncEvt {
    pub did: String,
    pub blocks: Vec<u8>,
    pub rev: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct TypedSyncEvt {
    pub r#type: String, // 'commit'
    pub seq: i64,
    pub time: String,
    pub evt: SyncEvt,
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
            time: rsky_common::now(),
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
            time: rsky_common::now(),
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

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum SeqEvt {
    TypedCommitEvt(TypedCommitEvt),
    // TypedHandleEvt(TypedHandleEvt),
    TypedIdentityEvt(TypedIdentityEvt),
    TypedAccountEvt(TypedAccountEvt),
    // TypedTombstoneEvt(TypedTombstoneEvt),
    TypedSyncEvt(TypedSyncEvt),
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
                Some("sync") => Ok(SeqEvt::TypedSyncEvt(
                    serde_json::from_value(value).map_err(DeserializerError::custom)?,
                )),
                Some("identity") => Ok(SeqEvt::TypedIdentityEvt(
                    serde_json::from_value(value).map_err(DeserializerError::custom)?,
                )),
                Some("account") => Ok(SeqEvt::TypedAccountEvt(
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
            SeqEvt::TypedIdentityEvt(this) => this.seq,
            SeqEvt::TypedAccountEvt(this) => this.seq,
            SeqEvt::TypedSyncEvt(this) => this.seq,
        }
    }
}

pub async fn format_seq_commit(
    did: String,
    commit_data: CommitDataWithOps,
    writes: Vec<PreparedWrite>,
) -> Result<models::RepoSeq> {
    let too_big: bool;
    let mut ops: Vec<CommitEvtOp> = Vec::new();
    let mut blobs = CidSet::new(None);
    let car_slice: Vec<u8>;

    let mut blocks_to_send = BlockMap::new();
    blocks_to_send.add_map(commit_data.commit_data.new_blocks)?;
    blocks_to_send.add_map(commit_data.commit_data.relevant_blocks)?;

    if writes.len() > 200 || blocks_to_send.byte_size()? > 1000000 {
        too_big = true;
        let mut just_root = BlockMap::new();
        just_root.add(blocks_to_send.get(commit_data.commit_data.cid))?;
        car_slice = blocks_to_car_file(Some(&commit_data.commit_data.cid), just_root).await?;
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
        car_slice = blocks_to_car_file(Some(&commit_data.commit_data.cid), blocks_to_send).await?;
    }

    let evt = CommitEvt {
        rebase: false,
        too_big,
        repo: did.clone(),
        commit: commit_data.commit_data.cid,
        prev: commit_data.commit_data.prev,
        rev: commit_data.commit_data.rev,
        since: commit_data.commit_data.since,
        ops,
        blocks: car_slice,
        blobs: blobs.to_list(),
    };
    Ok(models::RepoSeq::new(
        did,
        "append".to_string(),
        struct_to_cbor(&evt)?,
        rsky_common::now(),
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
        struct_to_cbor(&evt)?,
        rsky_common::now(),
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
        struct_to_cbor(&evt)?,
        rsky_common::now(),
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
            AccountStatus::Desynchronized => LexiconAccountStatus::Desynchronized,
            AccountStatus::Throttled => LexiconAccountStatus::Throttled,
            _ => panic!("Conditional failed and allowed an invalid account status."),
        });
    }

    Ok(models::RepoSeq::new(
        did,
        "account".to_string(),
        struct_to_cbor(&evt)?,
        rsky_common::now(),
    ))
}

pub async fn format_seq_sync_evt(did: String, data: SyncEvtData) -> Result<models::RepoSeq> {
    let blocks = blocks_to_car_file(Some(&data.cid), data.blocks).await?;
    let evt = SyncEvt {
        did,
        rev: data.rev,
        blocks,
    };
    Ok(models::RepoSeq::new(
        evt.did.clone(),
        "sync".to_string(),
        struct_to_cbor(&evt)?,
        rsky_common::now(),
    ))
}

pub async fn sync_evt_data_from_commit(mut commit_data: CommitDataWithOps) -> Result<SyncEvtData> {
    let cid = vec![commit_data.commit_data.cid.clone()];
    match commit_data.commit_data.relevant_blocks.get_many(cid) {
        Ok(blocks_and_missing) if blocks_and_missing.missing.len() > 0 => {
            Err(anyhow::anyhow!("commit block was not found, could not build sync event"))
        },
        Ok(blocks_and_missing) => {
            Ok(SyncEvtData{
                rev: commit_data.commit_data.rev,
                cid: commit_data.commit_data.cid.clone(),
                blocks: blocks_and_missing.blocks
            })
        },
        Err(e) => {
            Err(e.into())
        },
    }
}