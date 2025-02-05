use crate::car;
use crate::car::blocks_to_car_file;
use crate::repo::block_map::BlockMap;
use crate::repo::cid_set::CidSet;
use crate::repo::error::DataStoreError;
use crate::repo::mst::MST;
use crate::repo::types::{Commit, RecordPath};
use crate::repo::util;
use crate::storage::types::RepoStorage;
use crate::vendored::iroh_car::CarWriter;
use anyhow::Result;
use futures::{stream, Stream, StreamExt};
use lexicon_cid::Cid;
use serde_cbor::Value as CborValue;
use std::sync::Arc;
use tokio::io::DuplexStream;
use tokio::sync::RwLock;

pub async fn get_full_repo(
    storage: Arc<RwLock<dyn RepoStorage>>,
    commit_cid: Cid,
) -> Result<impl Stream<Item = Result<Vec<u8>>> + Send + 'static> {
    Ok(car::write_car(
        Some(&commit_cid),
        move |mut car: CarWriter<DuplexStream>| {
            async move {
                // Get the commit:
                let commit = {
                    let storage_guard = storage.read().await;
                    storage_guard
                        .read_obj_and_bytes(
                            &commit_cid,
                            Box::new(|obj: CborValue| {
                                serde_cbor::value::from_value::<Commit>(obj.clone()).is_ok()
                            }),
                        )
                        .await?
                };

                let data: Commit = serde_cbor::value::from_value(commit.obj)?;

                // Write the commit block:
                car.write(commit_cid, commit.bytes).await?;

                // Load the MST and write it to the CAR stream:
                let mut mst = MST::load(storage.clone(), data.data, None)?;
                mst.write_to_car_stream(car).await
            }
        },
    )
    .await)
}

pub async fn get_records(
    storage: Arc<RwLock<dyn RepoStorage>>,
    commit_cid: Cid,
    paths: Vec<RecordPath>,
) -> Result<Vec<u8>> {
    let mut car = BlockMap::new();
    let commit = {
        let storage_guard = storage.read().await;
        storage_guard
            .read_obj_and_bytes(
                &commit_cid,
                Box::new(|obj: CborValue| {
                    match serde_cbor::value::from_value::<Commit>(obj.clone()) {
                        Ok(_) => true,
                        Err(_) => false,
                    }
                }),
            )
            .await?
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
    let storage_guard = storage.read().await;
    let found = storage_guard.get_blocks(all_cids.to_list()).await?;
    if found.missing.len() > 0 {
        return Err(anyhow::Error::new(DataStoreError::MissingBlocks(
            "writeRecordsToCarStream".to_owned(),
            found.missing,
        )));
    }
    for block in found.blocks.entries()? {
        car.set(block.cid, block.bytes)
    }
    blocks_to_car_file(Some(&commit_cid), car).await
}
