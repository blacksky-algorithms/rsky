// based on https://github.com/bluesky-social/atproto/blob/main/packages/repo/src/repo.ts
// also adds components from https://github.com/bluesky-social/atproto/blob/main/packages/pds/src/actor-store/repo/transactor.ts

use crate::common;
use crate::common::ipld::cid_for_cbor;
use crate::common::tid::{Ticker, TID};
use crate::db::DbConn;
use crate::lexicon::LEXICONS;
use crate::repo::aws::s3::S3BlobStore;
use crate::repo::blob::BlobReader;
use crate::repo::blob_refs::{BlobRef, JsonBlobRef};
use crate::repo::block_map::BlockMap;
use crate::repo::cid_set::CidSet;
use crate::repo::data_diff::DataDiff;
use crate::repo::error::DataStoreError;
use crate::repo::mst::MST;
use crate::repo::preference::PreferenceReader;
use crate::repo::record::RecordReader;
use crate::repo::types::{
    write_to_op, BlobConstraint, CollectionContents, Commit, CommitData, Ids, Lex, PreparedBlobRef,
    PreparedCreateOrUpdate, PreparedDelete, PreparedWrite, RecordCreateOrUpdateOp, RecordWriteEnum,
    RecordWriteOp, RepoContents, RepoRecord, UnsignedCommit, WriteOpAction,
};
use crate::repo::util::{cbor_to_lex, lex_to_ipld};
use crate::storage::readable_blockstore::ReadableBlockstore;
use crate::storage::types::RepoStorage;
use crate::storage::{sql_repo::SqlRepoReader, Ipld};
use anyhow::{bail, Result};
use diesel::*;
use futures::stream::{self, StreamExt};
use lazy_static::lazy_static;
use lexicon_cid::Cid;
use rsky_syntax::aturi::AtUri;
use secp256k1::{Keypair, Secp256k1, SecretKey};
use serde::Serialize;
use serde_cbor::Value as CborValue;
use serde_json::{json, Value as JsonValue};
use std::collections::BTreeMap;
use std::env;
use std::fmt::Debug;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct FoundBlobRef {
    pub r#ref: BlobRef,
    pub path: Vec<String>,
}

pub struct PrepareCreateOpts {
    pub did: String,
    pub collection: String,
    pub rkey: Option<String>,
    pub swap_cid: Option<Cid>,
    pub record: RepoRecord,
    pub validate: Option<bool>,
}

pub struct PrepareUpdateOpts {
    pub did: String,
    pub collection: String,
    pub rkey: String,
    pub swap_cid: Option<Cid>,
    pub record: RepoRecord,
    pub validate: Option<bool>,
}

pub struct PrepareDeleteOpts {
    pub did: String,
    pub collection: String,
    pub rkey: String,
    pub swap_cid: Option<Cid>,
}

pub struct CommitRecord {
    collection: String,
    rkey: String,
    cid: Cid,
    record: RepoRecord,
}

pub struct Repo {
    storage: Arc<RwLock<dyn RepoStorage>>, // get ipld blocks from db
    data: MST,
    commit: Commit,
    cid: Cid,
}

pub struct ActorStore {
    pub did: String,
    pub storage: Arc<RwLock<SqlRepoReader>>, // get ipld blocks from db
    pub record: RecordReader,                // get lexicon records from db
    pub blob: BlobReader,                    // get blobs
    pub pref: PreferenceReader,              // get preferences
}

// Combination of RepoReader/Transactor, BlobReader/Transactor, SqlRepoReader/Transactor
impl ActorStore {
    /// Concrete reader of an individual repo (hence S3BlobStore which takes `did` param)
    pub fn new(did: String, blobstore: S3BlobStore, conn: DbConn) -> Self {
        ActorStore {
            storage: Arc::new(RwLock::new(SqlRepoReader::new(did.clone(), None, conn))),
            record: RecordReader::new(did.clone()),
            pref: PreferenceReader::new(did.clone()),
            did,
            blob: BlobReader::new(blobstore), // Unlike TS impl, just use blob reader vs generator
        }
    }

    // Transactors
    // -------------------

    pub async fn create_repo(
        &self,
        keypair: Keypair,
        writes: Vec<PreparedCreateOrUpdate>,
    ) -> Result<CommitData> {
        let write_ops = writes
            .clone()
            .into_iter()
            .map(|prepare| {
                let at_uri: AtUri = prepare.uri.try_into()?;
                Ok(RecordCreateOrUpdateOp {
                    action: WriteOpAction::Create,
                    collection: at_uri.get_collection(),
                    rkey: at_uri.get_rkey(),
                    record: prepare.record,
                })
            })
            .collect::<Result<Vec<RecordCreateOrUpdateOp>>>()?;
        let commit = Repo::format_init_commit(
            self.storage.clone(),
            self.did.clone(),
            keypair,
            Some(write_ops),
        )
        .await?;
        let storage_guard = self.storage.read().await;
        storage_guard.apply_commit(commit.clone(), None).await?;
        let writes = writes
            .into_iter()
            .map(|w| PreparedWrite::Create(w))
            .collect::<Vec<PreparedWrite>>();
        self.blob.process_write_blobs(writes).await?;
        Ok(commit)
    }

    pub async fn process_writes(
        &mut self,
        writes: Vec<PreparedWrite>,
        swap_commit_cid: Option<Cid>,
    ) -> Result<CommitData> {
        let commit = self.format_commit(writes.clone(), swap_commit_cid).await?;
        {
            let immutable_borrow = &self;
            // & send to indexing
            immutable_borrow
                .index_writes(writes.clone(), &commit.rev)
                .await?;
        }
        // persist the commit to repo storage
        let storage_guard = self.storage.read().await;
        storage_guard.apply_commit(commit.clone(), None).await?;
        // process blobs
        self.blob.process_write_blobs(writes).await?;
        Ok(commit)
    }

    pub async fn format_commit(
        &mut self,
        writes: Vec<PreparedWrite>,
        swap_commit: Option<Cid>,
    ) -> Result<CommitData> {
        let current_root = {
            let storage_guard = self.storage.read().await;
            storage_guard.get_root_detailed().await
        };
        if let Ok(current_root) = current_root {
            if let Some(swap_commit) = swap_commit {
                if !current_root.cid.eq(&swap_commit) {
                    bail!("BadCommitSwapError: {0}", current_root.cid)
                }
            }
            {
                let mut storage_guard = self.storage.write().await;
                storage_guard.cache_rev(current_root.rev).await?;
            }
            let mut new_record_cids: Vec<Cid> = vec![];
            let mut delete_and_update_uris = vec![];
            for write in &writes {
                match write.clone() {
                    PreparedWrite::Create(c) => new_record_cids.push(c.cid),
                    PreparedWrite::Update(u) => {
                        new_record_cids.push(u.cid);
                        let u_at_uri: AtUri = u.uri.try_into()?;
                        delete_and_update_uris.push(u_at_uri);
                    }
                    PreparedWrite::Delete(d) => {
                        let d_at_uri: AtUri = d.uri.try_into()?;
                        delete_and_update_uris.push(d_at_uri)
                    }
                }
                if write.swap_cid().is_none() {
                    continue;
                }
                let write_at_uri: &AtUri = &write.uri().try_into()?;
                let record = self
                    .record
                    .get_record(write_at_uri, None, Some(true))
                    .await?;
                let current_record = match record {
                    Some(record) => Some(Cid::from_str(&record.cid)?),
                    None => None,
                };
                match write {
                    // There should be no current record for a create
                    PreparedWrite::Create(_) if write.swap_cid().is_some() => {
                        bail!("BadRecordSwapError: `{0:?}`", current_record)
                    }
                    // There should be a current record for an update
                    PreparedWrite::Update(_) if write.swap_cid().is_none() => {
                        bail!("BadRecordSwapError: `{0:?}`", current_record)
                    }
                    // There should be a current record for a delete
                    PreparedWrite::Delete(_) if write.swap_cid().is_none() => {
                        bail!("BadRecordSwapError: `{0:?}`", current_record)
                    }
                    _ => Ok::<(), anyhow::Error>(()),
                }?;
                match (current_record, write.swap_cid()) {
                    (Some(current_record), Some(swap_cid)) if current_record.eq(swap_cid) => {
                        Ok::<(), anyhow::Error>(())
                    }
                    _ => bail!(
                        "BadRecordSwapError: current record is `{0:?}`",
                        current_record
                    ),
                }?;
            }
            let mut repo = Repo::load(self.storage.clone(), Some(current_root.cid)).await?;
            let write_ops: Vec<RecordWriteOp> = writes
                .into_iter()
                .map(write_to_op)
                .collect::<Result<Vec<RecordWriteOp>>>()?;
            // @TODO: Use repo signing key global config
            let secp = Secp256k1::new();
            let repo_private_key = env::var("PDS_REPO_SIGNING_KEY_K256_PRIVATE_KEY_HEX").unwrap();
            let repo_secret_key =
                SecretKey::from_slice(&hex::decode(repo_private_key.as_bytes()).unwrap()).unwrap();
            let repo_signing_key = Keypair::from_secret_key(&secp, &repo_secret_key);

            let mut commit = repo
                .format_commit(RecordWriteEnum::List(write_ops), repo_signing_key)
                .await?;

            // find blocks that would be deleted but are referenced by another record
            let duplicate_record_cids = self
                .get_duplicate_record_cids(commit.removed_cids.to_list(), delete_and_update_uris)
                .await?;
            for cid in duplicate_record_cids {
                commit.removed_cids.delete(cid)
            }

            // find blocks that are relevant to ops but not included in diff
            // (for instance a record that was moved but cid stayed the same)
            let new_record_blocks = commit.relevant_blocks.get_many(new_record_cids)?;
            if new_record_blocks.missing.len() > 0 {
                let missing_blocks = {
                    let storage_guard = self.storage.read().await;
                    storage_guard.get_blocks(new_record_blocks.missing).await?
                };
                commit.relevant_blocks.add_map(missing_blocks.blocks)?;
            }
            Ok(commit)
        } else {
            bail!("No repo root found for `{0}`", self.did)
        }
    }

    pub async fn index_writes(&self, writes: Vec<PreparedWrite>, rev: &String) -> Result<()> {
        let now: &str = &common::now();

        let _ = stream::iter(writes)
            .then(|write| async move {
                Ok::<(), anyhow::Error>(match write {
                    PreparedWrite::Create(write) => {
                        let write_at_uri: AtUri = write.uri.try_into()?;
                        self.record
                            .index_record(
                                write_at_uri.clone(),
                                write.cid,
                                Some(write.record),
                                Some(write.action),
                                rev.clone(),
                                Some(now.to_string()),
                            )
                            .await?
                    }
                    PreparedWrite::Update(write) => {
                        let write_at_uri: AtUri = write.uri.try_into()?;
                        self.record
                            .index_record(
                                write_at_uri.clone(),
                                write.cid,
                                Some(write.record),
                                Some(write.action),
                                rev.clone(),
                                Some(now.to_string()),
                            )
                            .await?
                    }
                    PreparedWrite::Delete(write) => {
                        let write_at_uri: AtUri = write.uri.try_into()?;
                        self.record.delete_record(&write_at_uri).await?
                    }
                })
            })
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
        Ok(())
    }

    pub async fn destroy(&mut self) -> Result<()> {
        let did: String = self.did.clone();
        let storage_guard = self.storage.read().await;
        let db: Arc<DbConn> = storage_guard.db.clone();
        use crate::schema::pds::blob::dsl as BlobSchema;

        let blob_rows: Vec<String> = db
            .run(move |conn| {
                BlobSchema::blob
                    .filter(BlobSchema::did.eq(did))
                    .select(BlobSchema::cid)
                    .get_results(conn)
            })
            .await?;
        let cids = blob_rows
            .into_iter()
            .map(|row| Ok(Cid::from_str(&row)?))
            .collect::<Result<Vec<Cid>>>()?;
        let _ = stream::iter(cids.chunks(500))
            .then(|chunk| async {
                Ok::<(), anyhow::Error>(self.blob.blobstore.delete_many(chunk.to_vec()).await?)
            })
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
        Ok(())
    }

    pub async fn get_duplicate_record_cids(
        &self,
        cids: Vec<Cid>,
        touched_uris: Vec<AtUri>,
    ) -> Result<Vec<Cid>> {
        if touched_uris.len() == 0 || cids.len() == 0 {
            return Ok(vec![]);
        }
        let did: String = self.did.clone();
        let storage_guard = self.storage.read().await;
        let db: Arc<DbConn> = storage_guard.db.clone();
        use crate::schema::pds::record::dsl as RecordSchema;

        let cid_strs: Vec<String> = cids.into_iter().map(|c| c.to_string()).collect();
        let touched_uri_strs: Vec<String> = touched_uris.iter().map(|t| t.to_string()).collect();
        let res: Vec<String> = db
            .run(move |conn| {
                RecordSchema::record
                    .filter(RecordSchema::did.eq(did))
                    .filter(RecordSchema::cid.eq_any(cid_strs))
                    .filter(RecordSchema::uri.ne_all(touched_uri_strs))
                    .select(RecordSchema::cid)
                    .get_results(conn)
            })
            .await?;
        Ok(res
            .into_iter()
            .map(|row| Cid::from_str(&row).map_err(|error| anyhow::Error::new(error)))
            .collect::<Result<Vec<Cid>>>()?)
    }
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
            let parsed = parse::get_and_parse_record(&found.blocks, entry.value)?;
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
        let commit_block_bytes = common::struct_to_cbor(commit.clone())?;
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

pub fn blobs_for_write(record: RepoRecord, validate: bool) -> Result<Vec<PreparedBlobRef>> {
    let refs = find_blob_refs(Lex::Map(record.clone()), None, None);
    let record_type = match record.get("$type") {
        Some(Lex::Ipld(Ipld::String(t))) => Some(t),
        _ => None,
    };
    for r#ref in refs.clone() {
        if matches!(r#ref.r#ref.original, JsonBlobRef::Untyped(_)) {
            bail!("Legacy blob ref at `{}`", r#ref.path.join("/"))
        }
    }
    refs.into_iter()
        .map(|FoundBlobRef { r#ref, path }| {
            let constraints: BlobConstraint = match (validate, record_type) {
                (true, Some(record_type)) => {
                    let properties: crate::lexicon::lexicons::Image2 = serde_json::from_value(
                        CONSTRAINTS[record_type.as_str()][path.join("/")].clone(),
                    )?;
                    BlobConstraint {
                        max_size: Some(properties.max_size as usize),
                        accept: Some(properties.accept),
                    }
                }
                (_, _) => BlobConstraint {
                    max_size: None,
                    accept: None,
                },
            };

            Ok(PreparedBlobRef {
                cid: r#ref.get_cid()?,
                mime_type: r#ref.get_mime_type().to_string(),
                constraints,
            })
        })
        .collect::<Result<Vec<PreparedBlobRef>>>()
}

pub fn find_blob_refs(val: Lex, path: Option<Vec<String>>, layer: Option<u8>) -> Vec<FoundBlobRef> {
    let layer = layer.unwrap_or_else(|| 0);
    let path = path.unwrap_or_else(|| vec![]);
    if layer > 32 {
        return vec![];
    }
    // walk arrays
    match val {
        Lex::List(list) => list
            .into_iter()
            .flat_map(|item| find_blob_refs(item, Some(path.clone()), Some(layer + 1)))
            .collect::<Vec<FoundBlobRef>>(),
        Lex::Blob(blob) => vec![FoundBlobRef { r#ref: blob, path }],
        Lex::Ipld(Ipld::Json(JsonValue::Array(list))) => list
            .into_iter()
            .flat_map(|item| match serde_json::from_value::<RepoRecord>(item) {
                Ok(item) => find_blob_refs(Lex::Map(item), Some(path.clone()), Some(layer + 1)),
                Err(_) => vec![],
            })
            .collect::<Vec<FoundBlobRef>>(),
        Lex::Ipld(Ipld::Json(json)) => match serde_json::from_value::<JsonBlobRef>(json.clone()) {
            Ok(blob) => vec![FoundBlobRef {
                r#ref: BlobRef { original: blob },
                path,
            }],
            Err(_) => match serde_json::from_value::<RepoRecord>(json) {
                Ok(record) => record
                    .into_iter()
                    .flat_map(|(key, item)| {
                        find_blob_refs(
                            item,
                            Some([path.as_slice(), [key].as_slice()].concat()),
                            Some(layer + 1),
                        )
                    })
                    .collect::<Vec<FoundBlobRef>>(),
                Err(_) => vec![],
            },
        },
        Lex::Ipld(_) => vec![],
        Lex::Map(map) => map
            .into_iter()
            .flat_map(|(key, item)| {
                find_blob_refs(
                    item,
                    Some([path.as_slice(), [key].as_slice()].concat()),
                    Some(layer + 1),
                )
            })
            .collect::<Vec<FoundBlobRef>>(),
    }
}

pub fn assert_valid_record(record: &RepoRecord) -> Result<()> {
    match record.get("$type") {
        Some(Lex::Ipld(Ipld::String(_))) => Ok(()),
        _ => bail!("No $type provided"),
    }
}

pub fn set_collection_name(
    collection: &String,
    mut record: RepoRecord,
    validate: bool,
) -> Result<RepoRecord> {
    if record.get("$type").is_none() {
        record.insert(
            "$type".to_string(),
            Lex::Ipld(Ipld::Json(JsonValue::String(collection.clone()))),
        );
    }
    if let Some(Lex::Ipld(Ipld::Json(JsonValue::String(record_type)))) = record.get("$type") {
        if validate && record_type.to_string() != *collection {
            bail!("Invalid $type: expected {collection}, got {record_type}")
        }
    }
    Ok(record)
}

pub async fn cid_for_safe_record(record: RepoRecord) -> Result<Cid> {
    let lex = lex_to_ipld(Lex::Map(record));
    let block = serde_ipld_dagcbor::to_vec(&lex)?;
    // Confirm whether Block properly transforms between lex and cbor
    let _ = cbor_to_lex(block)?;
    cid_for_cbor(&lex)
}

pub async fn prepare_create(opts: PrepareCreateOpts) -> Result<PreparedCreateOrUpdate> {
    let PrepareCreateOpts {
        did,
        collection,
        rkey,
        swap_cid,
        validate,
        ..
    } = opts;
    let validate = validate.unwrap_or_else(|| true);

    let record = set_collection_name(&collection, opts.record, validate)?;
    if validate {
        assert_valid_record(&record)?;
    }

    // assert_no_explicit_slurs(rkey, record).await?;
    let next_rkey = Ticker::new().next(None);
    let rkey = rkey.unwrap_or(next_rkey.to_string());
    let uri = AtUri::make(did, Some(collection), Some(rkey))?;
    Ok(PreparedCreateOrUpdate {
        action: WriteOpAction::Create,
        uri: uri.to_string(),
        cid: cid_for_safe_record(record.clone()).await?,
        swap_cid,
        record: record.clone(),
        blobs: blobs_for_write(record, validate)?,
    })
}

pub async fn prepare_update(opts: PrepareUpdateOpts) -> Result<PreparedCreateOrUpdate> {
    let PrepareUpdateOpts {
        did,
        collection,
        rkey,
        swap_cid,
        validate,
        ..
    } = opts;
    let validate = validate.unwrap_or_else(|| true);

    let record = set_collection_name(&collection, opts.record, validate)?;
    if validate {
        assert_valid_record(&record)?;
    }
    // assert_no_explicit_slurs(rkey, record).await?;
    let uri = AtUri::make(did, Some(collection), Some(rkey))?;
    Ok(PreparedCreateOrUpdate {
        action: WriteOpAction::Update,
        uri: uri.to_string(),
        cid: cid_for_safe_record(record.clone()).await?,
        swap_cid,
        record: record.clone(),
        blobs: blobs_for_write(record, validate)?,
    })
}

pub fn prepare_delete(opts: PrepareDeleteOpts) -> Result<PreparedDelete> {
    let PrepareDeleteOpts {
        did,
        collection,
        rkey,
        swap_cid,
    } = opts;
    let uri = AtUri::make(did, Some(collection), Some(rkey))?;
    Ok(PreparedDelete {
        action: WriteOpAction::Delete,
        uri: uri.to_string(),
        swap_cid,
    })
}

lazy_static! {
    static ref CONSTRAINTS: JsonValue = {
        json!({
            Ids::AppBskyActorProfile.as_str(): {
                "avatar": LEXICONS.app_bsky_actor_profile.defs.main.record.properties.avatar,
                "banner": LEXICONS.app_bsky_actor_profile.defs.main.record.properties.banner
            },
            Ids::AppBskyFeedGenerator.as_str(): {
                "avatar": LEXICONS.app_bsky_feed_generator.defs.main.record.properties.avatar
            },
            Ids::AppBskyGraphList.as_str(): {
                "avatar": LEXICONS.app_bsky_graph_list.defs.main.record.properties.avatar
            },
            Ids::AppBskyFeedPost.as_str(): {
                "embed/images/image": LEXICONS.app_bsky_embed_images.defs.image.properties.image,
                "embed/external/thumb": LEXICONS.app_bsky_embed_external.defs.external.properties.thumb,
                "embed/media/images/image": LEXICONS.app_bsky_embed_images.defs.image.properties.image,
                "embed/media/external/thumb": LEXICONS.app_bsky_embed_external.defs.external.properties.thumb
            }
        })
    };
}

pub mod aws;
pub mod blob;
pub mod blob_refs;
pub mod block_map;
pub mod cid_set;
pub mod data_diff;
pub mod error;
pub mod mst;
pub mod parse;
pub mod preference;
mod readable_repo;
pub mod record;
pub mod sync;
pub mod types;
pub mod util;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apis::com::atproto::server::encode_did_key;
    use crate::car::{blocks_to_car_file, read_car_with_root};
    use crate::common::sign::sign_without_indexmap;
    use crate::repo::mst::util::{random_cid, random_str};
    use crate::repo::parse::get_and_parse_record;
    use crate::repo::sync::consumer::{verify_proofs, verify_records, verify_repo, ConsumerError};
    use crate::repo::sync::provider::{get_full_repo, get_records};
    use crate::repo::types::{RecordCidClaim, RecordDeleteOp, RecordPath};
    use crate::repo::util::{stream_to_buffer, verify_commit_sig};
    use crate::storage::memory_blockstore::MemoryBlockstore;
    use anyhow::Result;
    use futures::pin_mut;
    use rand::prelude::SliceRandom;
    use rand::thread_rng;
    use rsky_crypto::utils::random_bytes;

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
