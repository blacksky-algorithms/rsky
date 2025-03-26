// based on https://github.com/bluesky-social/atproto/blob/main/packages/repo/src/repo.ts
// also adds components from https://github.com/bluesky-social/atproto/blob/main/packages/pds/src/actor-store/repo/transactor.ts

use crate::actor_store::aws::s3::S3BlobStore;
use crate::actor_store::blob::BlobReader;
use crate::actor_store::preference::PreferenceReader;
use crate::actor_store::record::RecordReader;
use crate::actor_store::repo::sql_repo::SqlRepoReader;
use crate::db::DbConn;
use anyhow::{bail, Result};
use diesel::*;
use futures::stream::{self, StreamExt};
use lexicon_cid::Cid;
use rsky_common;
use rsky_repo::repo::Repo;
use rsky_repo::storage::readable_blockstore::ReadableBlockstore;
use rsky_repo::storage::types::RepoStorage;
use rsky_repo::types::{
    write_to_op, CommitData, PreparedCreateOrUpdate, PreparedWrite, RecordCreateOrUpdateOp,
    RecordWriteEnum, RecordWriteOp, WriteOpAction,
};
use rsky_syntax::aturi::AtUri;
use secp256k1::{Keypair, Secp256k1, SecretKey};
use std::env;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;

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
    pub fn new(did: String, blobstore: S3BlobStore, db: DbConn) -> Self {
        let db = Arc::new(db);
        ActorStore {
            storage: Arc::new(RwLock::new(SqlRepoReader::new(
                did.clone(),
                None,
                db.clone(),
            ))),
            record: RecordReader::new(did.clone(), db.clone()),
            pref: PreferenceReader::new(did.clone(), db.clone()),
            did,
            blob: BlobReader::new(blobstore, db.clone()), // Unlike TS impl, just use blob reader vs generator
        }
    }

    pub async fn get_repo_root(&self) -> Option<Cid> {
        let storage_guard = self.storage.read().await;
        storage_guard.get_root().await
    }

    // Transactors
    // -------------------

    #[deprecated]
    pub async fn create_repo_legacy(
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
            .map(PreparedWrite::Create)
            .collect::<Vec<PreparedWrite>>();
        self.blob.process_write_blobs(writes).await?;
        Ok(commit)
    }

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
            .map(PreparedWrite::Create)
            .collect::<Vec<PreparedWrite>>();
        self.blob.process_write_blobs(writes).await?;
        Ok(commit)
    }

    pub async fn process_import_repo(
        &mut self,
        commit: CommitData,
        writes: Vec<PreparedWrite>,
    ) -> Result<()> {
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
        Ok(())
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
            if !new_record_blocks.missing.is_empty() {
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

    pub async fn index_writes(&self, writes: Vec<PreparedWrite>, rev: &str) -> Result<()> {
        let now: &str = &rsky_common::now();

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
                                rev.to_owned(),
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
                                rev.to_owned(),
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
            .then(|chunk| async { self.blob.blobstore.delete_many(chunk.to_vec()).await })
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
        if touched_uris.is_empty() || cids.is_empty() {
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
        res.into_iter()
            .map(|row| Cid::from_str(&row).map_err(|error| anyhow::Error::new(error)))
            .collect::<Result<Vec<Cid>>>()
    }
}

pub mod aws;
pub mod blob;
pub mod preference;
pub mod record;
pub mod repo;
