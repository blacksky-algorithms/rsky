use crate::car::read_car_bytes;
use crate::common::struct_to_cbor;
use crate::models::models;
use crate::repo::block_map::BlockMap;
use crate::repo::cid_set::CidSet;
use crate::repo::types::{CommitData, PreparedWrite};
use crate::repo::util::format_data_key;
use anyhow::Result;
use libipld::Cid;
use crate::common;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub enum CommitEvtOpAction {
    Create,
    Update,
    Delete,
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
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct TombstoneEvt {
    pub did: String,
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
        car_slice = read_car_bytes(&commit_data.cid, just_root).await?;
    } else {
        too_big = false;
        for w in writes {
            let parts = w.uri().split("/").collect::<Vec<&str>>();
            let collection = *parts.get(0).unwrap_or(&"");
            let rkey = *parts.get(1).unwrap_or(&"");
            let path = format_data_key(collection.to_string(), rkey.to_string());
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
        car_slice = read_car_bytes(&commit_data.cid, commit_data.new_blocks).await?;
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

pub async fn format_seq_identity_evt(did: String) -> Result<models::RepoSeq> {
    let evt = IdentityEvt { did: did.clone() };
    Ok(models::RepoSeq::new(
        did,
        "identity".to_string(),
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
