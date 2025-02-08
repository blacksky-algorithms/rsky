use crate::mst::MST;
use crate::storage::types::RepoStorage;
use crate::types::{Commit, VersionedCommit};
use crate::util::ensure_v3_commit;
use anyhow::Result;
use lexicon_cid::Cid;
use serde_cbor::Value as CborValue;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct ReadableRepo {
    pub storage: Arc<RwLock<dyn RepoStorage>>,
    pub data: MST,
    pub commit: Commit,
    pub cid: Cid,
}

impl ReadableRepo {
    // static
    pub fn new(storage: Arc<RwLock<dyn RepoStorage>>, data: MST, commit: Commit, cid: Cid) -> Self {
        Self {
            storage,
            data,
            commit,
            cid,
        }
    }

    pub async fn load(storage: Arc<RwLock<dyn RepoStorage>>, commit_cid: Cid) -> Result<Self> {
        let commit: CborValue = {
            let storage_guard = storage.read().await;
            storage_guard
                .read_obj(
                    &commit_cid,
                    Box::new(|obj: CborValue| {
                        match serde_cbor::value::from_value::<VersionedCommit>(obj.clone()) {
                            Ok(_) => true,
                            Err(_) => false,
                        }
                    }),
                )
                .await?
        };
        let commit: VersionedCommit = serde_cbor::value::from_value(commit)?;
        let data = MST::load(storage.clone(), commit.data(), None)?;
        Ok(Self::new(
            storage,
            data,
            ensure_v3_commit(commit),
            commit_cid,
        ))
    }

    pub fn did(&self) -> &String {
        &self.commit.did
    }
}
