use crate::common::tid::Ticker;
use crate::repo::block_map::BlockMap;
use crate::repo::data_diff::DataDiff;
use crate::repo::error::DataStoreError;
use crate::repo::mst::MST;
use crate::repo::types::{
    CollectionContents, Commit, CommitData, RecordCreateOrUpdateOp, RepoContents, RepoRecord,
    VersionedCommit,
};
use crate::storage::{Ipld, SqlRepoReader};
use anyhow::Result;
use libipld::Cid;
use secp256k1::Keypair;
use std::collections::BTreeMap;

pub struct CommitRecord {
    collection: String,
    rkey: String,
    cid: Cid,
    record: RepoRecord,
}

pub struct Repo<'a> {
    storage: SqlRepoReader<'a>,
    data: MST<'a>,
    commit: Commit,
    cid: Cid,
}

impl<'a> Repo<'a> {
    pub fn new(storage: SqlRepoReader, data: MST, commit: Commit, cid: Cid) -> Self {
        Repo {
            storage,
            data,
            commit,
            cid,
        }
    }

    pub fn load(storage: &mut SqlRepoReader, commit_cid: Cid) -> Result<Self> {
        let commit: VersionedCommit = storage
            .read_obj(&commit_cid, |obj: &'a Ipld| match obj {
                Ipld::VersionedCommit(VersionedCommit::Commit(_)) => true,
                Ipld::VersionedCommit(VersionedCommit::LegacyV2Commit(_)) => true,
                _ => false,
            })?
            .commit();
        let data = MST::load(storage.clone(), commit.data(), None)?;
        println!("Loaded repo for did: `{:?}`", commit.did());
        Ok(Repo {
            storage: storage.clone(),
            data,
            commit: util::ensure_v3_commit(commit),
            cid: commit_cid,
        })
    }

    pub fn did(self) -> String {
        self.commit.did
    }

    pub fn version(self) -> u8 {
        self.commit.version
    }

    pub fn walk_records(&mut self, from: Option<String>) -> impl Iterator<Item = CommitRecord> {
        let mut iter: Vec<CommitRecord> = Vec::new();
        for leaf in self.data.walk_leaves_from(&from.unwrap_or("".to_owned())) {
            let path = util::parse_data_key(&leaf.key).unwrap();
            let record = self.storage.read_record(&leaf.value).unwrap();
            iter.push(CommitRecord {
                collection: path.collection,
                rkey: path.rkey,
                cid: leaf.value,
                record,
            })
        }
        iter.into_iter()
    }

    pub fn get_record(&mut self, collection: String, rkey: String) -> Result<Option<Ipld>> {
        let data_key = format!("{}/{}", collection, rkey);
        let cid = self.data.get(&data_key)?;
        match cid {
            None => Ok(None),
            Some(cid) => Ok(Some(
                self.storage
                    .read_obj(&cid, |obj| matches!(obj, Ipld::Map(_)))?,
            )),
        }
    }

    pub fn get_content(&mut self) -> Result<RepoContents> {
        let entries = self.data.list(None, None, None)?;
        let cids = entries
            .into_iter()
            .map(|entry| entry.value)
            .collect::<Vec<Cid>>();
        let found = self.storage.get_blocks(&mut self.storage.conn, cids)?;
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
                contents.insert(path.collection, CollectionContents::new());
            }
            let parsed = parse::get_and_parse_record(&found.blocks, entry.value)?;
            contents
                .get(&path.collection)
                .unwrap()
                .insert(path.rkey, parsed.record);
        }
        Ok(contents)
    }

    pub fn format_init_commit(
        storage: SqlRepoReader,
        did: String,
        keypair: Keypair,
        initial_writes: Option<Vec<RecordCreateOrUpdateOp>>,
    ) -> Result<CommitData> {
        let mut new_blocks = BlockMap::new();
        let mut data = MST::create(storage, None, None)?;
        for record in initial_writes.unwrap_or(Vec::new()) {
            let cid = new_blocks.add(record.record)?;
            let data_key = util::format_data_key(record.collection, record.rkey);
            data = data.add(&data_key, cid, None)?;
        }
        let data_cid = data.get_pointer()?;
        let diff = DataDiff::of(data, None)?;
        new_blocks.add_map(diff.new_mst_blocks)?;

        let rev = Ticker::new().next(None);
        //let commit =
        todo!()
    }
}

pub mod blob_refs;
pub mod block_map;
pub mod cid_set;
pub mod data_diff;
pub mod error;
pub mod mst;
pub mod parse;
pub mod types;
pub mod util;
