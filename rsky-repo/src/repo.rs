use crate::block_map::BlockMap;
use crate::cid_set::CidSet;
use crate::data_diff::DataDiff;
use crate::error::DataStoreError;
use crate::mst::MST;
use crate::storage::types::RepoStorage;
use crate::types::{
    CollectionContents, Commit, CommitData, RecordCreateOrUpdateOp, RecordWriteEnum, RecordWriteOp,
    RepoContents, RepoRecord, UnsignedCommit,
};
use crate::util;
use anyhow::{bail, Result};
use lexicon_cid::Cid;
use rsky_common;
use rsky_common::ipld::cid_for_cbor;
use rsky_common::tid::{Ticker, TID};
use secp256k1::Keypair;
use serde_cbor::Value as CborValue;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct CommitRecord {
    collection: String,
    rkey: String,
    cid: Cid,
    record: RepoRecord,
}

pub struct Repo {
    pub storage: Arc<RwLock<dyn RepoStorage>>, // get ipld blocks from db
    pub data: MST,
    pub commit: Commit,
    pub cid: Cid,
}

impl Repo {
    // static
    pub fn new(storage: Arc<RwLock<dyn RepoStorage>>, data: MST, commit: Commit, cid: Cid) -> Self {
        Repo {
            storage,
            data,
            commit,
            cid,
        }
    }

    // static
    pub async fn load(storage: Arc<RwLock<dyn RepoStorage>>, cid: Option<Cid>) -> Result<Self> {
        let commit_cid = if let Some(cid) = cid {
            Some(cid)
        } else {
            let storage_guard = storage.read().await;
            storage_guard.get_root().await
        };
        match commit_cid {
            Some(commit_cid) => {
                let commit_bytes: Vec<u8> = {
                    let storage_guard = storage.read().await;
                    match storage_guard.get_bytes(&commit_cid).await? {
                        Some(res) => res,
                        None => bail!("Missing blocks for commit cid {commit_cid}"),
                    }
                };
                let commit: Commit = serde_ipld_dagcbor::from_slice(commit_bytes.as_slice())?;
                let data = MST::load(storage.clone(), commit.data, None)?;
                Ok(Repo::new(storage, data, commit, commit_cid))
            }
            None => bail!("No cid provided and none in storage"),
        }
    }

    pub fn did(&self) -> String {
        self.commit.did.clone()
    }

    pub fn version(self) -> u8 {
        self.commit.version
    }

    pub async fn walk_records(
        &mut self,
        from: Option<String>,
    ) -> impl Iterator<Item = CommitRecord> {
        let mut iter: Vec<CommitRecord> = Vec::new();
        for leaf in self
            .data
            .walk_leaves_from(&from.unwrap_or("".to_owned()))
            .await
        {
            let path = util::parse_data_key(&leaf.key).unwrap();
            let storage_guard = self.storage.read().await;
            let record = storage_guard.read_record(&leaf.value).await.unwrap();
            iter.push(CommitRecord {
                collection: path.collection,
                rkey: path.rkey,
                cid: leaf.value,
                record,
            })
        }
        iter.into_iter()
    }

    pub async fn get_record(
        &mut self,
        collection: String,
        rkey: String,
    ) -> Result<Option<CborValue>> {
        let data_key = format!("{}/{}", collection, rkey);
        let cid = self.data.get(&data_key).await?;
        let storage_guard = self.storage.read().await;
        match cid {
            None => Ok(None),
            Some(cid) => Ok(Some(
                storage_guard
                    .read_obj(&cid, Box::new(|obj| matches!(obj, CborValue::Map(_))))
                    .await?,
            )),
        }
    }

    pub async fn get_contents(&mut self) -> Result<RepoContents> {
        let entries = self.data.list(None, None, None).await?;
        let cids = entries
            .clone()
            .into_iter()
            .map(|entry| entry.value)
            .collect::<Vec<Cid>>();
        let storage_guard = self.storage.read().await;
        let found = storage_guard.get_blocks(cids).await?;
        if found.missing.len() > 0 {
            return Err(anyhow::Error::new(DataStoreError::MissingBlocks(
                "getContents record".to_owned(),
                found.missing,
            )));
        }
        let mut contents: RepoContents = BTreeMap::new();
        for entry in entries {
            let path = util::parse_data_key(&entry.key)?;
            if contents.get(&path.collection).is_none() {
                contents.insert(path.collection.clone(), CollectionContents::new());
            }
            let parsed = crate::parse::get_and_parse_record(&found.blocks, entry.value)?;
            if let Some(collection_contents) = contents.get_mut(&path.collection) {
                collection_contents.insert(path.rkey, parsed.record);
            }
        }
        Ok(contents.to_owned())
    }

    // static
    pub async fn format_init_commit(
        storage: Arc<RwLock<dyn RepoStorage>>,
        did: String,
        keypair: Keypair,
        initial_writes: Option<Vec<RecordCreateOrUpdateOp>>,
    ) -> Result<CommitData> {
        let mut new_blocks = BlockMap::new();
        let mut data = MST::create(storage, None, None).await?;
        for record in initial_writes.unwrap_or(Vec::new()) {
            let cid = new_blocks.add(record.record)?;
            let data_key = util::format_data_key(record.collection, record.rkey);
            data = data.add(&data_key, cid, None).await?;
        }
        let data_cid: Cid = data.get_pointer().await?;
        let diff = DataDiff::of(&mut data, None).await?;
        new_blocks.add_map(diff.new_mst_blocks)?;
        let rev = Ticker::new().next(None);
        let commit = util::sign_commit(
            UnsignedCommit {
                did,
                version: 3,
                rev: rev.0.clone(),
                prev: None, // added for backwards compatibility with v2
                data: data_cid,
            },
            keypair,
        )?;
        let commit_cid = new_blocks.add(commit)?;
        Ok(CommitData {
            cid: commit_cid,
            rev: rev.0,
            since: None,
            prev: None,
            new_blocks: new_blocks.clone(),
            relevant_blocks: new_blocks,
            removed_cids: diff.removed_cids,
        })
    }

    // static
    pub async fn create_from_commit(
        storage: Arc<RwLock<dyn RepoStorage>>,
        commit: CommitData,
    ) -> Result<Self> {
        {
            let storage_guard = storage.read().await;
            storage_guard.apply_commit(commit.clone(), None).await?;
        }
        Repo::load(storage, Some(commit.cid)).await
    }

    // static
    pub async fn create(
        storage: Arc<RwLock<dyn RepoStorage>>,
        did: String,
        keypair: Keypair,
        initial_writes: Option<Vec<RecordCreateOrUpdateOp>>,
    ) -> Result<Self> {
        let commit =
            Self::format_init_commit(storage.clone(), did, keypair, initial_writes).await?;
        Self::create_from_commit(storage, commit).await
    }

    pub async fn format_commit(
        &mut self,
        to_write: RecordWriteEnum,
        keypair: Keypair,
    ) -> Result<CommitData> {
        let writes = match to_write {
            RecordWriteEnum::List(to_write) => to_write,
            RecordWriteEnum::Single(to_write) => vec![to_write],
        };
        let mut leaves = BlockMap::new();

        let mut data = self.data.clone(); // @TODO: Confirm if this should be clone
        for write in writes.clone() {
            match write {
                RecordWriteOp::Create(write) => {
                    let cid = leaves.add(write.record)?;
                    let data_key = util::format_data_key(write.collection, write.rkey);
                    data = data.add(&data_key, cid, None).await?;
                }
                RecordWriteOp::Update(write) => {
                    let cid = leaves.add(write.record)?;
                    let data_key = util::format_data_key(write.collection, write.rkey);
                    data = data.update(&data_key, cid).await?;
                }
                RecordWriteOp::Delete(write) => {
                    let data_key = util::format_data_key(write.collection, write.rkey);
                    data = data.delete(&data_key).await?;
                }
            }
        }

        let data_cid = data.get_pointer().await?;
        let diff = DataDiff::of(&mut data, Some(&mut self.data.clone())).await?;

        let mut new_blocks = diff.new_mst_blocks;
        let mut removed_cids = diff.removed_cids;

        let mut relevant_blocks = BlockMap::new();
        for op in writes {
            data.add_blocks_for_path(
                util::format_data_key(op.collection(), op.rkey()),
                &mut relevant_blocks,
            )
            .await?;
        }

        let added_leaves = leaves.get_many(diff.new_leaf_cids.to_list())?;
        if added_leaves.missing.len() > 0 {
            bail!("Missing leaf blocks: {:?}", added_leaves.missing);
        }
        new_blocks.add_map(added_leaves.blocks.clone())?;
        relevant_blocks.add_map(added_leaves.blocks)?;

        let rev = Ticker::new().next(Some(TID(self.commit.rev.clone())));

        let commit = util::sign_commit(
            UnsignedCommit {
                did: self.did(),
                version: 3,
                rev: rev.0.clone(),
                prev: None, // added for backwards compatibility with v2
                data: data_cid,
            },
            keypair,
        )?;
        let commit_block_bytes = rsky_common::struct_to_cbor(&commit)?;
        let commit_cid = cid_for_cbor(&commit)?;

        if !commit_cid.eq(&self.cid) {
            new_blocks.set(commit_cid, commit_block_bytes.clone());
            relevant_blocks.set(commit_cid, commit_block_bytes.clone());
            removed_cids.add(self.cid);
        }

        Ok(CommitData {
            cid: commit_cid,
            rev: rev.0,
            since: Some(self.commit.rev.clone()),
            prev: Some(self.cid),
            new_blocks,
            relevant_blocks,
            removed_cids,
        })
    }

    pub async fn apply_commit(&self, commit_data: CommitData) -> Result<Self> {
        let commit_data_cid = commit_data.cid.clone();
        {
            let storage_guard = self.storage.read().await;
            storage_guard.apply_commit(commit_data, None).await?;
        }
        Repo::load(self.storage.clone(), Some(commit_data_cid)).await
    }

    pub async fn apply_writes(
        &mut self,
        to_write: RecordWriteEnum,
        keypair: Keypair,
    ) -> Result<Self> {
        let commit = self.format_commit(to_write, keypair).await?;
        self.apply_commit(commit).await
    }

    pub fn format_resign_commit(&self, rev: String, keypair: Keypair) -> Result<CommitData> {
        let commit = util::sign_commit(
            UnsignedCommit {
                did: self.did(),
                version: 3,
                rev: rev.clone(),
                prev: None, // added for backwards compatibility with v2
                data: self.commit.data,
            },
            keypair,
        )?;
        let mut new_blocks = BlockMap::new();
        let commit_cid = new_blocks.add(commit)?;
        Ok(CommitData {
            cid: commit_cid,
            rev,
            since: None,
            prev: None,
            new_blocks: new_blocks.clone(),
            relevant_blocks: new_blocks,
            removed_cids: CidSet::new(Some(vec![self.cid])),
        })
    }

    pub async fn resign_commit(&mut self, rev: String, keypair: Keypair) -> Result<Self> {
        let formatted = self.format_resign_commit(rev, keypair)?;
        self.apply_commit(formatted).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::car::{blocks_to_car_file, read_car_with_root};
    use crate::mst::util::{random_cid, random_str};
    use crate::parse::get_and_parse_record;
    use crate::storage::memory_blockstore::MemoryBlockstore;
    use crate::sync::consumer::{verify_proofs, verify_records, verify_repo, ConsumerError};
    use crate::sync::provider::{get_full_repo, get_records};
    use crate::types::{RecordCidClaim, RecordDeleteOp, RecordPath, WriteOpAction};
    use crate::util::{stream_to_buffer, verify_commit_sig};
    use anyhow::Result;
    use futures::pin_mut;
    use rand::prelude::SliceRandom;
    use rand::thread_rng;
    use rsky_common::sign::sign_without_indexmap;
    use rsky_crypto::utils::{encode_did_key, random_bytes};
    use secp256k1::Secp256k1;
    use serde_json::json;

    const TEST_COLLECTIONS: [&'static str; 2] = ["com.example.posts", "com.example.likes"];
    const COLL_NAME: &str = "com.example.posts";

    pub struct FillRepoOutput {
        pub repo: Repo,
        pub data: RepoContents,
    }

    pub fn generate_object() -> RepoRecord {
        serde_json::from_value(json!({ "name": random_str(100) }))
            .expect("Simple object failed to serialize")
    }

    pub async fn fill_repo(
        mut repo: Repo,
        keypair: Keypair,
        items_per_collection: usize,
    ) -> Result<FillRepoOutput> {
        let mut repo_data: RepoContents = Default::default();
        let mut writes: Vec<RecordWriteOp> = Default::default();
        for coll_name in TEST_COLLECTIONS {
            let mut coll_data: CollectionContents = Default::default();
            for _ in 0..items_per_collection {
                let object = generate_object();
                let rkey = Ticker::new().next(None).to_string();
                coll_data.insert(rkey.clone(), object.clone());
                writes.push(RecordWriteOp::Create(RecordCreateOrUpdateOp {
                    action: WriteOpAction::Create,
                    collection: coll_name.to_string(),
                    rkey,
                    record: object,
                }));
            }
            repo_data.insert(coll_name.to_string(), coll_data);
        }
        let writes = RecordWriteEnum::List(writes);
        let updated = repo.apply_writes(writes, keypair).await?;
        Ok(FillRepoOutput {
            repo: updated,
            data: repo_data,
        })
    }

    pub async fn add_bad_commit(mut repo: Repo, keypair: Keypair) -> Result<Repo> {
        let object = generate_object();
        let mut new_blocks = BlockMap::new();
        let cid = new_blocks.add(object)?;
        let mut updated_data = repo
            .data
            .add(
                &format!("com.example.test/{}", Ticker::new().next(None)),
                cid,
                None,
            )
            .await?;
        let data_cid = updated_data.get_pointer().await?;
        let diff = DataDiff::of(&mut updated_data, Some(&mut repo.data)).await?;
        new_blocks.add_map(diff.new_mst_blocks)?;
        // we generate a bad sig by signing some other data
        let rev = TID::next_str(Some(repo.commit.rev))?;
        let commit = Commit {
            did: repo.commit.did,
            rev: rev.clone(),
            data: data_cid,
            prev: repo.commit.prev,
            version: repo.commit.version,
            sig: sign_without_indexmap(&random_bytes(256), &keypair.secret_key())?.to_vec(),
        };
        let commit_cid = new_blocks.add(commit)?;
        {
            let storage_guard = repo.storage.read().await;
            storage_guard
                .apply_commit(
                    CommitData {
                        cid: commit_cid,
                        rev,
                        since: None,
                        prev: Some(repo.cid),
                        new_blocks,
                        relevant_blocks: BlockMap::new(),
                        removed_cids: diff.removed_cids,
                    },
                    None,
                )
                .await?;
        }
        Repo::load(repo.storage.clone(), Some(commit_cid)).await
    }

    pub async fn contents_to_claims(contents: RepoContents) -> Result<Vec<RecordCidClaim>> {
        let mut claims: Vec<RecordCidClaim> = Default::default();
        for coll in contents.keys() {
            if let Some(coll_content) = contents.get(coll) {
                for rkey in coll_content.keys() {
                    claims.push(RecordCidClaim {
                        collection: coll.to_string(),
                        rkey: rkey.to_string(),
                        cid: Some(cid_for_cbor(coll_content.get(rkey).unwrap())?),
                    });
                }
            }
        }
        Ok(claims)
    }

    #[derive(Debug)]
    pub struct FormatEditOpts {
        pub adds: Option<usize>,
        pub updates: Option<usize>,
        pub deletes: Option<usize>,
    }

    #[derive(Debug)]
    pub struct FormatEditOutput {
        pub commit: CommitData,
        pub data: RepoContents,
    }

    pub async fn format_edit(
        repo: &mut Repo,
        prev_data: RepoContents,
        keypair: Keypair,
        params: FormatEditOpts,
    ) -> Result<FormatEditOutput> {
        let (adds, updates, deletes) = (
            params.adds.unwrap_or(0),
            params.updates.unwrap_or(0),
            params.deletes.unwrap_or(0),
        );
        let mut repo_data: RepoContents = Default::default();
        let mut writes: Vec<RecordWriteOp> = Default::default();
        let mut rng = thread_rng();
        for coll_name in TEST_COLLECTIONS {
            let mut coll_data: CollectionContents = match prev_data.get(coll_name) {
                Some(prev) => prev.clone(),
                None => Default::default(),
            };

            let mut entries: Vec<(String, RepoRecord)> = coll_data
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            entries.shuffle(&mut rng);

            for _ in 0..adds {
                let object = generate_object();
                let rkey = TID::next_str(None)?;
                coll_data.insert(rkey.clone(), object.clone());
                writes.push(RecordWriteOp::Create(RecordCreateOrUpdateOp {
                    action: WriteOpAction::Create,
                    collection: coll_name.to_string(),
                    rkey,
                    record: object,
                }));
            }

            let to_update = entries[0..updates].to_vec();
            for i in 0..to_update.len() {
                let object = generate_object();
                let rkey = to_update[i].0.clone();
                coll_data.insert(rkey.clone(), object.clone());
                writes.push(RecordWriteOp::Update(RecordCreateOrUpdateOp {
                    action: WriteOpAction::Update,
                    collection: coll_name.to_string(),
                    rkey: rkey.to_string(),
                    record: object,
                }));
            }

            let to_delete = entries[0..deletes].to_vec();
            for i in 0..to_delete.len() {
                let rkey = to_delete[i].0.clone();
                coll_data.remove(&rkey);
                writes.push(RecordWriteOp::Delete(RecordDeleteOp {
                    action: WriteOpAction::Delete,
                    collection: coll_name.to_string(),
                    rkey: rkey.to_string(),
                }));
            }
            repo_data.insert(coll_name.to_string(), coll_data);
        }
        let commit = repo
            .format_commit(RecordWriteEnum::List(writes), keypair)
            .await?;
        Ok(FormatEditOutput {
            commit,
            data: repo_data,
        })
    }

    #[tokio::test]
    async fn creates_repo() -> Result<()> {
        let storage = MemoryBlockstore::default();
        let secp = Secp256k1::new();
        let keypair = Keypair::new(&secp, &mut thread_rng());
        let did_key = encode_did_key(&keypair.public_key());
        let _ = Repo::create(Arc::new(RwLock::new(storage)), did_key, keypair, None).await?;
        Ok(())
    }

    #[tokio::test]
    async fn has_proper_metadata() -> Result<()> {
        let storage = MemoryBlockstore::default();
        let secp = Secp256k1::new();
        let keypair = Keypair::new(&secp, &mut thread_rng());
        let did_key = encode_did_key(&keypair.public_key());
        let repo = Repo::create(
            Arc::new(RwLock::new(storage)),
            did_key.clone(),
            keypair,
            None,
        )
        .await?;
        assert_eq!(repo.did(), did_key);
        assert_eq!(repo.version(), 3);
        Ok(())
    }

    #[tokio::test]
    async fn does_basic_operations() -> Result<()> {
        let storage = MemoryBlockstore::default();
        let secp = Secp256k1::new();
        let keypair = Keypair::new(&secp, &mut thread_rng());
        let did_key = encode_did_key(&keypair.public_key());
        let mut repo = Repo::create(
            Arc::new(RwLock::new(storage)),
            did_key.clone(),
            keypair,
            None,
        )
        .await?;
        let rkey = TID::next_str(None)?;
        let record = generate_object();
        repo = repo
            .apply_writes(
                RecordWriteEnum::Single(RecordWriteOp::Create(RecordCreateOrUpdateOp {
                    action: WriteOpAction::Create,
                    collection: COLL_NAME.to_string(),
                    rkey: rkey.clone(),
                    record: record.clone(),
                })),
                keypair,
            )
            .await?;

        let got = repo.get_record(COLL_NAME.to_string(), rkey.clone()).await?;
        assert!(got.is_some());
        let got: RepoRecord = serde_cbor::value::from_value(got.unwrap())?;
        assert_eq!(got, record);

        let updated_record = generate_object();
        repo = repo
            .apply_writes(
                RecordWriteEnum::Single(RecordWriteOp::Update(RecordCreateOrUpdateOp {
                    action: WriteOpAction::Update,
                    collection: COLL_NAME.to_string(),
                    rkey: rkey.clone(),
                    record: updated_record.clone(),
                })),
                keypair,
            )
            .await?;
        let got = repo.get_record(COLL_NAME.to_string(), rkey.clone()).await?;
        assert!(got.is_some());
        let got: RepoRecord = serde_cbor::value::from_value(got.unwrap())?;
        assert_eq!(got, updated_record);

        repo = repo
            .apply_writes(
                RecordWriteEnum::Single(RecordWriteOp::Delete(RecordDeleteOp {
                    action: WriteOpAction::Delete,
                    collection: COLL_NAME.to_string(),
                    rkey: rkey.clone(),
                })),
                keypair,
            )
            .await?;
        let got = repo.get_record(COLL_NAME.to_string(), rkey.clone()).await?;
        assert!(got.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn adds_content_collection() -> Result<()> {
        let storage = MemoryBlockstore::default();
        let secp = Secp256k1::new();
        let keypair = Keypair::new(&secp, &mut thread_rng());
        let did_key = encode_did_key(&keypair.public_key());
        let mut repo = Repo::create(
            Arc::new(RwLock::new(storage)),
            did_key.clone(),
            keypair,
            None,
        )
        .await?;
        let filled = fill_repo(repo, keypair, 100).await?;
        repo = filled.repo;
        let repo_data = filled.data;
        let contents = repo.get_contents().await?;
        assert_eq!(contents, repo_data);
        Ok(())
    }

    #[tokio::test]
    async fn edits_and_deletes_content() -> Result<()> {
        let storage = MemoryBlockstore::default();
        let secp = Secp256k1::new();
        let keypair = Keypair::new(&secp, &mut thread_rng());
        let did_key = encode_did_key(&keypair.public_key());
        let mut repo = Repo::create(
            Arc::new(RwLock::new(storage)),
            did_key.clone(),
            keypair,
            None,
        )
        .await?;
        let filled = fill_repo(repo, keypair, 100).await?;
        repo = filled.repo;
        let mut repo_data = filled.data;
        let edit = format_edit(
            &mut repo,
            repo_data,
            keypair,
            FormatEditOpts {
                adds: Some(20),
                updates: Some(20),
                deletes: Some(20),
            },
        )
        .await?;
        repo = repo.apply_commit(edit.commit).await?;
        repo_data = edit.data;
        let contents = repo.get_contents().await?;
        assert_eq!(contents, repo_data);
        Ok(())
    }

    #[tokio::test]
    async fn has_a_valid_signature_to_commit() -> Result<()> {
        let storage = MemoryBlockstore::default();
        let secp = Secp256k1::new();
        let keypair = Keypair::new(&secp, &mut thread_rng());
        let did_key = encode_did_key(&keypair.public_key());
        let mut repo = Repo::create(
            Arc::new(RwLock::new(storage)),
            did_key.clone(),
            keypair,
            None,
        )
        .await?;
        let filled = fill_repo(repo, keypair, 100).await?;
        repo = filled.repo;
        let repo_data = filled.data;
        let edit = format_edit(
            &mut repo,
            repo_data,
            keypair,
            FormatEditOpts {
                adds: Some(20),
                updates: Some(20),
                deletes: Some(20),
            },
        )
        .await?;
        repo = repo.apply_commit(edit.commit).await?;
        let _ = repo.get_contents().await?;
        let verified = verify_commit_sig(repo.commit, &did_key)?;
        assert!(verified);
        Ok(())
    }

    #[tokio::test]
    async fn sets_correct_did() -> Result<()> {
        let storage = MemoryBlockstore::default();
        let secp = Secp256k1::new();
        let keypair = Keypair::new(&secp, &mut thread_rng());
        let did_key = encode_did_key(&keypair.public_key());
        let mut repo = Repo::create(
            Arc::new(RwLock::new(storage)),
            did_key.clone(),
            keypair,
            None,
        )
        .await?;
        let filled = fill_repo(repo, keypair, 100).await?;
        repo = filled.repo;
        let repo_data = filled.data;
        let edit = format_edit(
            &mut repo,
            repo_data,
            keypair,
            FormatEditOpts {
                adds: Some(20),
                updates: Some(20),
                deletes: Some(20),
            },
        )
        .await?;
        repo = repo.apply_commit(edit.commit).await?;
        let _ = repo.get_contents().await?;
        let _ = verify_commit_sig(repo.commit.clone(), &did_key)?;
        assert_eq!(repo.did(), did_key);
        Ok(())
    }

    #[tokio::test]
    async fn loads_from_blockstore() -> Result<()> {
        let storage = MemoryBlockstore::default();
        let secp = Secp256k1::new();
        let keypair = Keypair::new(&secp, &mut thread_rng());
        let did_key = encode_did_key(&keypair.public_key());
        let mut repo = Repo::create(
            Arc::new(RwLock::new(storage)),
            did_key.clone(),
            keypair,
            None,
        )
        .await?;
        let filled = fill_repo(repo, keypair, 100).await?;
        repo = filled.repo;
        let repo_data = filled.data;
        let edit = format_edit(
            &mut repo,
            repo_data.clone(),
            keypair,
            FormatEditOpts {
                adds: Some(20),
                updates: Some(20),
                deletes: Some(20),
            },
        )
        .await?;
        repo = repo.apply_commit(edit.commit).await?;
        let old_contents = repo.get_contents().await?;
        let _ = verify_commit_sig(repo.commit, &did_key)?;

        let mut reloaded_repo = Repo::load(repo.storage.clone(), Some(repo.cid)).await?;
        let contents = reloaded_repo.get_contents().await?;
        assert_eq!(contents, old_contents);
        assert_eq!(reloaded_repo.did(), did_key);
        assert_eq!(reloaded_repo.version(), 3);
        Ok(())
    }

    // @NOTE this test uses a fully deterministic tree structure
    #[tokio::test]
    async fn includes_all_relevant_blocks_for_proof_commit_data() -> Result<()> {
        let did = "did:example:test";
        let collection = "com.atproto.test";
        let record = json!({ "test": 123 });

        let blockstore = MemoryBlockstore::default();
        let secp = Secp256k1::new();
        let keypair = Keypair::new(&secp, &mut thread_rng());
        let mut repo = Repo::create(
            Arc::new(RwLock::new(blockstore)),
            did.to_string(),
            keypair,
            None,
        )
        .await?;

        let mut keys: Vec<String> = Default::default();
        for i in 0..50 {
            let rkey = format!("key-{i}");
            keys.push(rkey.clone());
            repo = repo
                .apply_writes(
                    RecordWriteEnum::Single(RecordWriteOp::Create(RecordCreateOrUpdateOp {
                        action: WriteOpAction::Create,
                        collection: collection.to_string(),
                        rkey,
                        record: serde_json::from_value(record.clone())?,
                    })),
                    keypair,
                )
                .await?;
        }

        // this test demonstrates the test case:
        // specifically in the case of deleting the first key, there is a "rearranged block" that is necessary
        // in the proof path but _is not_ in newBlocks (as it already existed in the repository)
        {
            let commit = repo
                .format_commit(
                    RecordWriteEnum::Single(RecordWriteOp::Delete(RecordDeleteOp {
                        action: WriteOpAction::Delete,
                        collection: collection.to_string(),
                        rkey: keys[0].clone(),
                    })),
                    keypair,
                )
                .await?;
            let car = blocks_to_car_file(Some(&commit.cid), commit.new_blocks).await?;
            let did_key = encode_did_key(&keypair.public_key());
            let result = verify_proofs(
                car,
                vec![RecordCidClaim {
                    collection: collection.to_string(),
                    rkey: keys[0].clone(),
                    cid: None,
                }],
                did,
                &did_key,
            )
            .await;
            assert!(matches!(
                result
                    .unwrap_err()
                    .downcast_ref::<DataStoreError>()
                    .unwrap(),
                DataStoreError::MissingBlock(_)
            ));
        }

        for rkey in keys {
            let commit = repo
                .format_commit(
                    RecordWriteEnum::Single(RecordWriteOp::Delete(RecordDeleteOp {
                        action: WriteOpAction::Delete,
                        collection: collection.to_string(),
                        rkey: rkey.clone(),
                    })),
                    keypair,
                )
                .await?;
            let car = blocks_to_car_file(Some(&commit.cid), commit.relevant_blocks.clone()).await?;
            let did_key = encode_did_key(&keypair.public_key());
            let proof_res = verify_proofs(
                car,
                vec![RecordCidClaim {
                    collection: collection.to_string(),
                    rkey,
                    cid: None,
                }],
                did,
                &did_key,
            )
            .await?;
            assert_eq!(proof_res.unverified.len(), 0);
            repo = repo.apply_commit(commit).await?;
        }
        Ok(())
    }

    #[tokio::test]
    async fn verifies_valid_records() -> Result<()> {
        let storage = MemoryBlockstore::default();
        let secp = Secp256k1::new();
        let keypair = Keypair::new(&secp, &mut thread_rng());
        let repo_did = "did:example:test";
        let mut repo = Repo::create(
            Arc::new(RwLock::new(storage)),
            repo_did.to_string(),
            keypair,
            None,
        )
        .await?;
        let did_key = encode_did_key(&keypair.public_key());
        let filled = fill_repo(repo, keypair, 5).await?;
        repo = filled.repo;
        let repo_data = filled.data;
        let claims = contents_to_claims(repo_data).await?;
        let claims_as_record_paths: Vec<RecordPath> = claims
            .clone()
            .into_iter()
            .map(|c| RecordPath {
                collection: c.collection,
                rkey: c.rkey,
            })
            .collect();
        let proofs = get_records(repo.storage.clone(), repo.cid, claims_as_record_paths).await?;
        let results = verify_proofs(proofs, claims.clone(), repo_did, &did_key).await?;

        assert!(results.verified.len() > 0);
        assert_eq!(results.verified, claims);
        assert_eq!(results.unverified.len(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn verifies_record_nonexistence() -> Result<()> {
        let storage = MemoryBlockstore::default();
        let secp = Secp256k1::new();
        let keypair = Keypair::new(&secp, &mut thread_rng());
        let repo_did = "did:example:test";
        let mut repo = Repo::create(
            Arc::new(RwLock::new(storage)),
            repo_did.to_string(),
            keypair,
            None,
        )
        .await?;
        let did_key = encode_did_key(&keypair.public_key());
        let filled = fill_repo(repo, keypair, 5).await?;
        repo = filled.repo;
        let _repo_data = filled.data;
        let claims: Vec<RecordCidClaim> = vec![RecordCidClaim {
            collection: TEST_COLLECTIONS[0].to_string(),
            rkey: Ticker::new().next(None).to_string(),
            cid: None,
        }];
        let claims_as_record_paths: Vec<RecordPath> = claims
            .clone()
            .into_iter()
            .map(|c| RecordPath {
                collection: c.collection,
                rkey: c.rkey,
            })
            .collect();
        let proofs = get_records(repo.storage.clone(), repo.cid, claims_as_record_paths).await?;
        let results = verify_proofs(proofs, claims.clone(), repo_did, &did_key).await?;

        assert!(results.verified.len() > 0);
        assert_eq!(results.verified, claims);
        assert_eq!(results.unverified.len(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn does_not_verify_a_record_that_does_not_exist() -> Result<()> {
        let storage = MemoryBlockstore::default();
        let secp = Secp256k1::new();
        let keypair = Keypair::new(&secp, &mut thread_rng());
        let repo_did = "did:example:test";
        let mut repo = Repo::create(
            Arc::new(RwLock::new(storage)),
            repo_did.to_string(),
            keypair,
            None,
        )
        .await?;
        let did_key = encode_did_key(&keypair.public_key());
        let filled = fill_repo(repo, keypair, 5).await?;
        repo = filled.repo;
        let repo_data = filled.data;
        let real_claims = contents_to_claims(repo_data).await?;
        let claims: Vec<RecordCidClaim> = vec![RecordCidClaim {
            collection: real_claims[0].collection.clone(),
            rkey: Ticker::new().next(None).to_string(),
            cid: real_claims[0].cid.clone(),
        }];
        let claims_as_record_paths: Vec<RecordPath> = claims
            .clone()
            .into_iter()
            .map(|c| RecordPath {
                collection: c.collection,
                rkey: c.rkey,
            })
            .collect();
        let proofs = get_records(repo.storage.clone(), repo.cid, claims_as_record_paths).await?;
        let results = verify_proofs(proofs, claims.clone(), repo_did, &did_key).await?;

        assert_eq!(results.verified.len(), 0);
        assert!(results.unverified.len() > 0);
        assert_eq!(results.unverified, claims);
        Ok(())
    }

    #[tokio::test]
    async fn does_not_verify_an_invalid_record_at_a_real_path() -> Result<()> {
        let storage = MemoryBlockstore::default();
        let secp = Secp256k1::new();
        let keypair = Keypair::new(&secp, &mut thread_rng());
        let repo_did = "did:example:test";
        let mut repo = Repo::create(
            Arc::new(RwLock::new(storage)),
            repo_did.to_string(),
            keypair,
            None,
        )
        .await?;
        let did_key = encode_did_key(&keypair.public_key());
        let filled = fill_repo(repo, keypair, 5).await?;
        repo = filled.repo;
        let repo_data = filled.data;
        let real_claims = contents_to_claims(repo_data).await?;
        let mut no_storage: Option<&mut dyn RepoStorage> = None;
        let claims: Vec<RecordCidClaim> = vec![RecordCidClaim {
            collection: real_claims[0].collection.clone(),
            rkey: real_claims[0].rkey.clone(),
            cid: Some(random_cid(&mut no_storage, None).await?),
        }];
        let claims_as_record_paths: Vec<RecordPath> = claims
            .clone()
            .into_iter()
            .map(|c| RecordPath {
                collection: c.collection,
                rkey: c.rkey,
            })
            .collect();
        let proofs = get_records(repo.storage.clone(), repo.cid, claims_as_record_paths).await?;
        let results = verify_proofs(proofs, claims.clone(), repo_did, &did_key).await?;

        assert_eq!(results.verified.len(), 0);
        assert!(results.unverified.len() > 0);
        assert_eq!(results.unverified, claims);
        Ok(())
    }

    #[tokio::test]
    async fn does_not_verify_a_delete_where_a_record_does_exist() -> Result<()> {
        let storage = MemoryBlockstore::default();
        let secp = Secp256k1::new();
        let keypair = Keypair::new(&secp, &mut thread_rng());
        let repo_did = "did:example:test";
        let mut repo = Repo::create(
            Arc::new(RwLock::new(storage)),
            repo_did.to_string(),
            keypair,
            None,
        )
        .await?;
        let did_key = encode_did_key(&keypair.public_key());
        let filled = fill_repo(repo, keypair, 5).await?;
        repo = filled.repo;
        let repo_data = filled.data;
        let real_claims = contents_to_claims(repo_data).await?;
        let claims: Vec<RecordCidClaim> = vec![RecordCidClaim {
            collection: real_claims[0].collection.clone(),
            rkey: real_claims[0].rkey.clone(),
            cid: None,
        }];
        let claims_as_record_paths: Vec<RecordPath> = claims
            .clone()
            .into_iter()
            .map(|c| RecordPath {
                collection: c.collection,
                rkey: c.rkey,
            })
            .collect();
        let proofs = get_records(repo.storage.clone(), repo.cid, claims_as_record_paths).await?;
        let results = verify_proofs(proofs, claims.clone(), repo_did, &did_key).await?;

        assert_eq!(results.verified.len(), 0);
        assert!(results.unverified.len() > 0);
        assert_eq!(results.unverified, claims);
        Ok(())
    }

    #[tokio::test]
    async fn can_determine_record_proofs_from_car_file() -> Result<()> {
        let storage = MemoryBlockstore::default();
        let secp = Secp256k1::new();
        let keypair = Keypair::new(&secp, &mut thread_rng());
        let repo_did = "did:example:test";
        let mut repo = Repo::create(
            Arc::new(RwLock::new(storage)),
            repo_did.to_string(),
            keypair,
            None,
        )
        .await?;
        let did_key = encode_did_key(&keypair.public_key());
        let filled = fill_repo(repo, keypair, 5).await?;
        repo = filled.repo;
        let repo_data = filled.data;
        let possible = contents_to_claims(repo_data.clone()).await?;
        let claims = vec![
            // random sampling of records
            possible[0].clone(),
            possible[4].clone(),
            possible[5].clone(),
            possible[8].clone(),
        ];
        let claims_as_record_paths: Vec<RecordPath> = claims
            .clone()
            .into_iter()
            .map(|c| RecordPath {
                collection: c.collection,
                rkey: c.rkey,
            })
            .collect();
        let proofs = get_records(repo.storage.clone(), repo.cid, claims_as_record_paths).await?;
        let records = verify_records(proofs, repo_did, &did_key).await?;
        for record in records {
            let found_claim = claims
                .iter()
                .find(|claim| claim.collection == record.collection && claim.rkey == record.rkey);
            match found_claim {
                None => bail!("Could not find record for claim"),
                Some(found_claim) => assert_eq!(
                    found_claim.cid,
                    Some(cid_for_cbor(
                        repo_data
                            .get(&record.collection)
                            .unwrap()
                            .get(&record.rkey)
                            .unwrap()
                    )?)
                ),
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn verify_proofs_throws_on_a_bad_signature() -> Result<()> {
        let storage = MemoryBlockstore::default();
        let secp = Secp256k1::new();
        let keypair = Keypair::new(&secp, &mut thread_rng());
        let repo_did = "did:example:test";
        let mut repo = Repo::create(
            Arc::new(RwLock::new(storage)),
            repo_did.to_string(),
            keypair,
            None,
        )
        .await?;
        let did_key = encode_did_key(&keypair.public_key());
        let filled = fill_repo(repo, keypair, 5).await?;
        repo = filled.repo;
        let repo_data = filled.data;
        let bad_repo = add_bad_commit(repo, keypair).await?;
        let claims = contents_to_claims(repo_data).await?;
        let claims_as_record_paths: Vec<RecordPath> = claims
            .clone()
            .into_iter()
            .map(|c| RecordPath {
                collection: c.collection,
                rkey: c.rkey,
            })
            .collect();
        let proofs = get_records(
            bad_repo.storage.clone(),
            bad_repo.cid,
            claims_as_record_paths,
        )
        .await?;
        let result = verify_proofs(proofs, claims.clone(), repo_did, &did_key).await;
        assert!(matches!(
            result.unwrap_err().downcast_ref::<ConsumerError>().unwrap(),
            ConsumerError::RepoVerificationError(_)
        ));
        Ok(())
    }

    #[tokio::test]
    async fn sync_a_full_repo() -> Result<()> {
        let storage = MemoryBlockstore::default();
        let secp = Secp256k1::new();
        let keypair = Keypair::new(&secp, &mut thread_rng());
        let repo_did = "did:example:test";
        let mut repo = Repo::create(
            Arc::new(RwLock::new(storage)),
            repo_did.to_string(),
            keypair,
            None,
        )
        .await?;
        let did_key = encode_did_key(&keypair.public_key());
        let filled = fill_repo(repo, keypair, 5).await?;
        repo = filled.repo;
        let repo_data = filled.data;
        let repo_stream = get_full_repo(repo.storage.clone(), repo.cid).await?;
        pin_mut!(repo_stream);
        let car_bytes = stream_to_buffer(repo_stream).await?;
        let mut car = read_car_with_root(car_bytes).await?;
        let verifed = verify_repo(
            &mut car.blocks,
            car.root,
            Some(&repo_did.to_string()),
            Some(&did_key),
            None,
        )
        .await?;
        let sync_storage = MemoryBlockstore::default();
        sync_storage.apply_commit(verifed.commit, None).await?;
        let mut loaded_repo =
            Repo::load(Arc::new(RwLock::new(sync_storage)), Some(car.root)).await?;
        let contents = loaded_repo.get_contents().await?;
        assert_eq!(contents, repo_data);
        let mut contents_from_ops = RepoContents::default();
        for write in verifed.creates {
            match contents_from_ops.get(&write.collection) {
                Some(_) => (),
                None => {
                    contents_from_ops
                        .insert(write.collection.clone(), CollectionContents::default());
                    ()
                }
            }
            let parsed = get_and_parse_record(&car.blocks, write.cid)?;
            contents_from_ops
                .get_mut(&write.collection)
                .unwrap()
                .insert(write.rkey, parsed.record);
        }
        assert_eq!(contents_from_ops, repo_data);
        Ok(())
    }
}
