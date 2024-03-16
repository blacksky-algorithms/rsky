use crate::common::tid::{Ticker, TID};
use crate::repo::block_map::BlockMap;
use crate::repo::data_diff::DataDiff;
use crate::repo::error::DataStoreError;
use crate::repo::mst::MST;
use crate::repo::types::{CollectionContents, Commit, CommitData, RecordCreateOrUpdateOp, RepoContents, RepoRecord, UnsignedCommit, VersionedCommit, RecordWriteEnum, RecordWriteOp};
use crate::storage::{Ipld, SqlRepoReader};
use anyhow::{bail, Result};
use libipld::Cid;
use secp256k1::Keypair;
use std::collections::BTreeMap;
use crate::repo::cid_set::CidSet;

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

    pub fn load(storage: &mut SqlRepoReader, cid: Option<Cid>) -> Result<Self> {
        let commit_cid = if let Some(cid) = cid {
            Some(cid)
        } else {
            storage.get_root()
        };
        match commit_cid { 
            Some(commit_cid) => {
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
            },
            None => bail!("No cid provided and none in storage")
        }

    }

    pub fn did(&self) -> String {
        self.commit.did.clone()
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
        let commit = util::sign_commit(
            UnsignedCommit {
                did,
                version: 3,
                rev: rev.0.clone(),
                prev: None, // added for backwards compatibility with v2
                data: data_cid
            },
            keypair
        )?;
        let commit_cid = new_blocks.add(commit)?;
        Ok(CommitData {
            cid: commit_cid,
            rev: rev.0,
            since: None,
            prev: None,
            new_blocks,
            removed_cids: diff.removed_cids
        })
    }

    pub fn create_from_commit(storage: &mut SqlRepoReader, commit: CommitData) -> Result<Self> {
        storage.apply_commit(commit.clone())?;
        Repo::load(storage, Some(commit.cid))
    }

    pub fn create(
        mut storage: SqlRepoReader,
        did: String,
        keypair: Keypair,
        initial_writes: Option<Vec<RecordCreateOrUpdateOp>>
    ) -> Result<Self> {
        let commit = Repo::format_init_commit(
            storage,
            did,
            keypair,
            initial_writes
        )?;
        Repo::create_from_commit(&mut storage, commit)
    }
    
    pub fn format_commit(
        &mut self,
        to_write: RecordWriteEnum,
        keypair: Keypair
    ) -> Result<CommitData> {
        let writes = match to_write {
            RecordWriteEnum::List(to_write) => to_write,
            RecordWriteEnum::Single(to_write) => vec![to_write]
        };
        let mut leaves = BlockMap::new();
        
        let mut data = self.data.clone();
        for write in writes {
            match write {
                RecordWriteOp::Create(write) => {
                    let cid = leaves.add(write.record)?;
                    let data_key = util::format_data_key(write.collection, write.rkey);
                    data = data.add(&data_key, cid, None)?;
                },
                RecordWriteOp::Update(write) => {
                    let cid = leaves.add(write.record)?;
                    let data_key = util::format_data_key(write.collection, write.rkey);
                    data = data.update(&data_key, cid)?;
                },
                RecordWriteOp::Delete(write) => {
                    let data_key = util::format_data_key(write.collection, write.rkey);
                    data = data.delete(&data_key)?;
                },
            }
        }
        
        let data_cid = data.get_pointer()?;
        let diff = DataDiff::of(data, Some(self.data.clone()))?;
        let mut new_blocks = diff.new_mst_blocks;
        let mut removed_cids = diff.removed_cids;
        
        let added_leaves = leaves.get_many(diff.new_leaf_cids.to_list())?;
        if added_leaves.missing.len() > 0 {
            bail!("Missing leaf blocks: {:?}",added_leaves.missing);
        }
        new_blocks.add_map(added_leaves.blocks)?;
        
        let rev = Ticker::new().next(Some(TID(self.commit.rev.clone())));
        let commit = util::sign_commit(
            UnsignedCommit {
                did: self.did(),
                version: 3,
                rev: rev.0.clone(),
                prev: None, // added for backwards compatibility with v2
                data: data_cid
            },
            keypair
        )?;
        let commit_cid = new_blocks.add(commit)?;

        // ensure the commit cid actually changed
        if commit_cid.eq(&self.cid) {
            new_blocks.delete(commit_cid)?;
        } else {
            removed_cids.add(self.cid);
        }
        
       Ok(CommitData{
           cid: commit_cid,
           rev: rev.0,
           since: Some(self.commit.rev.clone()),
           prev: Some(self.cid),
           new_blocks,
           removed_cids
       })
    }
    
    pub fn apply_commit(&mut self, commit_data: CommitData) -> Result<Self> {
        let commit_data_cid = commit_data.cid.clone();
        self.storage.apply_commit(commit_data)?;
        Repo::load(&mut self.storage, Some(commit_data_cid))
    }
    
    pub fn apply_writes(
        &mut self,
        to_write: RecordWriteEnum,
        keypair: Keypair
    ) -> Result<Self> {
        let commit = self.format_commit(to_write, keypair)?;
        self.apply_commit(commit)
    }
    
    pub fn format_resign_commit(
        &self,
        rev: String,
        keypair: Keypair
    ) -> Result<CommitData> {
        let commit = util::sign_commit(
            UnsignedCommit {
                did: self.did(),
                version: 3,
                rev: rev.clone(),
                prev: None, // added for backwards compatibility with v2
                data: self.commit.data
            },
            keypair
        )?;
        let mut new_blocks = BlockMap::new();
        let commit_cid = new_blocks.add(commit)?;
        Ok(CommitData {
            cid: commit_cid,
            rev,
            since: None,
            prev: None,
            new_blocks,
            removed_cids: CidSet::new(Some(vec![self.cid]))
        })
    }
    
    pub fn resign_commit(&mut self, rev: String, keypair: Keypair) -> Result<Self> {
        let formatted = self.format_resign_commit(rev, keypair)?;
        self.apply_commit(formatted)
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
mod reader;
mod record;
