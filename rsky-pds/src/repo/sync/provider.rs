use crate::car::read_car_bytes;
use crate::repo::block_map::BlockMap;
use crate::repo::cid_set::CidSet;
use crate::repo::error::DataStoreError;
use crate::repo::mst::MST;
use crate::repo::types::{Commit, RecordPath};
use crate::repo::util;
use crate::storage::types::RepoStorage;
use anyhow::Result;
use futures::{stream, StreamExt};
use lexicon_cid::Cid;
use serde_cbor::Value as CborValue;
use std::sync::Arc;
use tokio::sync::RwLock;

pub async fn get_records(
    storage: Arc<RwLock<dyn RepoStorage>>,
    commit_cid: Cid,
    paths: Vec<RecordPath>,
) -> Result<Vec<u8>> {
    let mut car = BlockMap::new();
    let commit = {
        let mut storage_guard = storage.write().await;
        storage_guard.read_obj_and_bytes(
            &commit_cid,
            Box::new(|obj: &CborValue| {
                match serde_cbor::value::from_value::<Commit>(obj.clone()) {
                    Ok(_) => true,
                    Err(_) => false,
                }
            }),
        )?
    };
    let data: Commit = serde_cbor::value::from_value(commit.obj)?;
    car.set(commit_cid, commit.bytes);
    let mst = MST::load(storage.clone(), data.data, None)?;
    let cids_for_paths = stream::iter(paths)
        .then(|p| {
            let mut mst_clone = mst.clone();
            async move {
                Ok::<Vec<Cid>, anyhow::Error>(
                    mst_clone
                        .cids_for_path(util::format_data_key(p.collection, p.rkey))
                        .await?,
                )
            }
        })
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    let all_cids: CidSet =
        cids_for_paths
            .into_iter()
            .fold(CidSet::new(None), |mut acc: CidSet, cur| {
                acc.add_set(CidSet::new(Some(cur)));
                acc
            });
    let mut storage_guard = storage.write().await;
    let found = storage_guard.get_blocks(all_cids.to_list())?;
    if found.missing.len() > 0 {
        return Err(anyhow::Error::new(DataStoreError::MissingBlocks(
            "writeRecordsToCarStream".to_owned(),
            found.missing,
        )));
    }
    for block in found.blocks.entries()? {
        car.set(block.cid, block.bytes)
    }
    read_car_bytes(Some(&commit_cid), car).await
}
