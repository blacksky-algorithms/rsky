use crate::car::read_car_bytes;
use crate::repo::block_map::BlockMap;
use crate::repo::cid_set::CidSet;
use crate::repo::error::DataStoreError;
use crate::repo::mst::MST;
use crate::repo::types::{Commit, RecordPath};
use crate::repo::util;
use crate::storage::SqlRepoReader;
use anyhow::Result;
use lexicon_cid::Cid;
use serde_cbor::Value as CborValue;

pub async fn get_records(
    storage: &mut SqlRepoReader,
    commit_cid: Cid,
    paths: Vec<RecordPath>,
) -> Result<Vec<u8>> {
    let mut car = BlockMap::new();
    let commit = storage.read_obj_and_bytes(&commit_cid, |obj: &CborValue| {
        match serde_cbor::value::from_value::<Commit>(obj.clone()) {
            Ok(_) => true,
            Err(_) => false,
        }
    })?;
    let data: Commit = serde_cbor::value::from_value(commit.obj)?;
    car.set(commit_cid, commit.bytes);
    let mut mst = MST::load(storage.clone(), data.data, None)?;
    let cids_for_paths = paths
        .into_iter()
        .map(|p| mst.cids_for_path(util::format_data_key(p.collection, p.rkey)))
        .collect::<Result<Vec<Vec<Cid>>>>()?;
    let all_cids: CidSet =
        cids_for_paths
            .into_iter()
            .fold(CidSet::new(None), |mut acc: CidSet, cur| {
                acc.add_set(CidSet::new(Some(cur)));
                acc
            });
    let found = storage.get_blocks(all_cids.to_list()).await?;
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
