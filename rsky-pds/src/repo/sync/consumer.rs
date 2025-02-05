use crate::car::read_car_with_root;
use crate::repo::block_map::BlockMap;
use crate::repo::data_diff::DataDiff;
use crate::repo::mst::MST;
use crate::repo::readable_repo::ReadableRepo;
use crate::repo::types::{
    Commit, CommitData, RecordCidClaim, RecordClaim, RecordPath, VerifiedDiff, VerifiedRepo,
};
use crate::repo::util::{ensure_creates, parse_data_key, verify_commit_sig};
use crate::repo::{util, Repo};
use crate::storage::memory_blockstore::MemoryBlockstore;
use crate::storage::readable_blockstore::ReadableBlockstore;
use crate::storage::sync_storage::SyncStorage;
use crate::storage::types::RepoStorage;
use anyhow::{bail, Result};
use lexicon_cid::Cid;
use serde_cbor::Value as CborValue;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

#[derive(Debug)]
pub struct VerifyProofsOutput {
    pub verified: Vec<RecordCidClaim>,
    pub unverified: Vec<RecordCidClaim>,
}
#[derive(Debug)]
pub struct VerifyRepoInput {
    pub ensure_leaves: Option<bool>,
}

#[derive(Error, Debug)]
pub enum ConsumerError {
    #[error("RepoVerificationError: {0}")]
    RepoVerificationError(String),
}

pub async fn verify_repo(
    blocks: &mut BlockMap,
    head: Cid,
    did: Option<&String>,
    signing_key: Option<&String>,
    opts: Option<VerifyRepoInput>,
) -> Result<VerifiedRepo> {
    let diff = verify_diff(None, blocks, head, did, signing_key, opts).await?;
    let creates = ensure_creates(diff.writes)?;
    Ok(VerifiedRepo {
        creates,
        commit: diff.commit,
    })
}

pub async fn verify_diff(
    mut repo: Option<Repo>,
    update_blocks: &mut BlockMap,
    update_root: Cid,
    did: Option<&String>,
    signing_key: Option<&String>,
    opts: Option<VerifyRepoInput>,
) -> Result<VerifiedDiff> {
    let ensure_leaves = match opts {
        None => true,
        Some(opts) => opts.ensure_leaves.unwrap_or(true),
    };
    let staged_storage = MemoryBlockstore::new(Some(update_blocks.clone())).await?;
    let update_storage: Arc<RwLock<dyn RepoStorage>> = match repo {
        Some(ref repo) => Arc::new(RwLock::new(SyncStorage::new(
            Arc::new(RwLock::new(staged_storage)),
            repo.storage.clone(),
        ))),
        None => Arc::new(RwLock::new(staged_storage)),
    };
    let mut updated = verify_repo_root(update_storage, update_root, did, signing_key).await?;
    let repo_mst: Option<&mut MST> = match repo {
        None => None,
        Some(ref mut repo) => Some(&mut repo.data),
    };
    let diff = DataDiff::of(&mut updated.data, repo_mst).await?;
    let writes = util::diff_to_write_descripts(&diff).await?;
    let mut new_blocks = diff.new_mst_blocks;
    let leaves = update_blocks.get_many(diff.new_leaf_cids.to_list())?;
    if leaves.missing.len() > 0 && ensure_leaves {
        bail!("missing leaf blocks: {:?}", leaves.missing);
    }
    new_blocks.add_map(leaves.blocks)?;
    let mut removed_cids = diff.removed_cids;
    let commit_cid = new_blocks.add(updated.commit.clone())?;
    // ensure the commit cid actually changed
    if let Some(ref repo) = repo {
        if commit_cid == repo.cid {
            new_blocks.delete(commit_cid)?;
        } else {
            removed_cids.add(repo.cid);
        }
    }
    Ok(VerifiedDiff {
        writes,
        commit: CommitData {
            cid: updated.cid,
            rev: updated.commit.rev.clone(),
            since: match repo {
                None => None,
                Some(ref repo) => Some(repo.commit.rev.clone()),
            },
            prev: match repo {
                None => None,
                Some(ref repo) => Some(repo.cid),
            },
            relevant_blocks: new_blocks.clone(),
            new_blocks,
            removed_cids,
        },
    })
}

pub async fn verify_repo_root(
    storage: Arc<RwLock<dyn RepoStorage>>,
    head: Cid,
    did: Option<&String>,
    signing_key: Option<&String>,
) -> Result<ReadableRepo> {
    let repo = ReadableRepo::load(storage, head).await?;
    if let Some(did) = did {
        if repo.did() != did {
            return Err(ConsumerError::RepoVerificationError(format!(
                "Invalid repo did: {}",
                repo.did()
            ))
            .into());
        }
    }
    if let Some(signing_key) = signing_key {
        let valid_sig = verify_commit_sig(repo.commit.clone(), signing_key)?;
        if !valid_sig {
            return Err(ConsumerError::RepoVerificationError(format!(
                "Invalid signature on commit: {}",
                repo.cid.to_string()
            ))
            .into());
        }
    }
    Ok(repo)
}

pub async fn verify_proofs(
    proofs: Vec<u8>,
    claims: Vec<RecordCidClaim>,
    did: &str,
    did_key: &String,
) -> Result<VerifyProofsOutput> {
    let car = read_car_with_root(proofs).await?;
    let blockstore = MemoryBlockstore::new(Some(car.blocks)).await?;
    let data: CborValue = blockstore
        .read_obj(
            &car.root,
            Box::new(
                |obj: CborValue| match serde_cbor::value::from_value::<Commit>(obj.clone()) {
                    Ok(_) => true,
                    Err(_) => false,
                },
            ),
        )
        .await?;
    let commit: Commit = serde_cbor::value::from_value(data)?;
    if commit.did != did {
        return Err(ConsumerError::RepoVerificationError(format!(
            "Invalid repo did: {}",
            commit.did
        ))
        .into());
    }
    match verify_commit_sig(commit.clone(), did_key)? {
        false => {
            return Err(ConsumerError::RepoVerificationError(format!(
                "Invalid signature on commit: {}",
                car.root.to_string()
            ))
            .into());
        }
        true => {
            let mut mst = MST::load(Arc::new(RwLock::new(blockstore)), commit.data, None)?;
            let mut verified: Vec<RecordCidClaim> = Default::default();
            let mut unverified: Vec<RecordCidClaim> = Default::default();
            for claim in claims {
                let found = mst
                    .get(&util::format_data_key(
                        claim.collection.clone(),
                        claim.rkey.clone(),
                    ))
                    .await?;
                let record = match found {
                    Some(found) => {
                        let storage_guard = mst.storage.read().await;
                        Some(
                            storage_guard
                                .read_obj(&found, Box::new(|obj| matches!(obj, CborValue::Map(_))))
                                .await?,
                        )
                    }
                    None => None,
                };
                match &claim.cid {
                    None => match record {
                        None => verified.push(claim),
                        Some(_) => unverified.push(claim),
                    },
                    Some(_) => match claim.cid == found {
                        true => verified.push(claim),
                        false => unverified.push(claim),
                    },
                }
            }
            Ok(VerifyProofsOutput {
                verified,
                unverified,
            })
        }
    }
}

pub async fn verify_records(
    proofs: Vec<u8>,
    did: &str,
    signing_key: &String,
) -> Result<Vec<RecordClaim>> {
    let car = read_car_with_root(proofs).await?;
    let blockstore = MemoryBlockstore::new(Some(car.blocks)).await?;
    let data: CborValue = blockstore
        .read_obj(
            &car.root,
            Box::new(
                |obj: CborValue| match serde_cbor::value::from_value::<Commit>(obj.clone()) {
                    Ok(_) => true,
                    Err(_) => false,
                },
            ),
        )
        .await?;
    let commit: Commit = serde_cbor::value::from_value(data)?;
    if commit.did != did {
        return Err(ConsumerError::RepoVerificationError(format!(
            "Invalid repo did: {}",
            commit.did
        ))
        .into());
    }
    match verify_commit_sig(commit.clone(), signing_key)? {
        false => {
            return Err(ConsumerError::RepoVerificationError(format!(
                "Invalid signature on commit: {}",
                car.root.to_string()
            ))
            .into());
        }
        true => {
            let mst = MST::load(Arc::new(RwLock::new(blockstore)), commit.data, None)?;

            let mut records: Vec<RecordClaim> = Default::default();
            let leaves = mst.clone().reachable_leaves().await?;
            for leaf in leaves {
                let RecordPath { collection, rkey } = parse_data_key(&leaf.key)?;
                let storage_guard = mst.storage.read().await;

                if let Some(record) = storage_guard.attempt_read_record(&leaf.value).await {
                    records.push(RecordClaim {
                        collection,
                        rkey,
                        record: Some(record),
                    });
                }
            }
            Ok(records)
        }
    }
}
