// based on https://github.com/bluesky-social/atproto/blob/main/packages/pds/src/actor-store/actor-store.ts
// per-actor sqlite storage with a reader/transactor split

use crate::actor_store::blob::{apply_write_blobs_in, BlobReader, PromotedBlob};
use crate::actor_store::blobstore::BlobStore;
use crate::actor_store::db::{get_db, get_migrated_db, ActorDb};
use crate::actor_store::preference::PreferenceReader;
use crate::actor_store::record::{delete_record_in, index_record_in, RecordReader};
use crate::actor_store::repo::sql_repo::{apply_commit_in, SqlRepoReader};
use crate::actor_store::repo::types::SyncEvtData;
use crate::actor_store::space::SpaceStore;
use crate::background::BackgroundQueue;
use crate::config::ActorStoreConfig;
use crate::lifecycle::LifecycleStore;
use crate::publication::settle_pending_work;
use crate::sequencer::events::{format_seq_commit, format_seq_sync_evt, sync_evt_data_from_commit};
use anyhow::{anyhow, bail, Result};
use lexicon_cid::Cid;
use lru::LruCache;
use rsky_crypto::utils::encode_did_key;
use rsky_repo::repo::Repo;
use rsky_repo::storage::readable_blockstore::ReadableBlockstore;
use rsky_repo::storage::types::RepoStorage;
use rsky_repo::storage::CidAndRev;
use rsky_repo::types::{
    write_to_op, CommitAction, CommitData, CommitDataWithOps, CommitOp, PreparedCreateOrUpdate,
    PreparedWrite, RecordCreateOrUpdateOp, RecordWriteEnum, RecordWriteOp, WriteOpAction,
};
use rsky_repo::util::format_data_key;
use rsky_syntax::aturi::AtUri;
use rusqlite::OptionalExtension;
use secp256k1::{Keypair, Secp256k1, SecretKey};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt;
use std::num::NonZeroUsize;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use tokio::io::AsyncWriteExt;
use tokio::sync::{OwnedMutexGuard, RwLock};

pub mod aws;
pub mod blob;
pub mod blobstore;
pub mod db;
pub mod disk_blobstore;
pub mod preference;
pub mod record;
pub mod repo;
pub mod space;

#[derive(Debug)]
enum FormatCommitError {
    BadRecordSwap(String),
    RecordSwapMismatch(String),
    BadCommitSwap(String),
    MissingRepoRoot(String),
}

impl fmt::Display for FormatCommitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadRecordSwap(record) => write!(f, "BadRecordSwapError: `{:?}`", record),
            Self::RecordSwapMismatch(record) => {
                write!(f, "BadRecordSwapError: current record is `{:?}`", record)
            }
            Self::BadCommitSwap(cid) => write!(f, "BadCommitSwapError: {}", cid),
            Self::MissingRepoRoot(did) => write!(f, "No repo root found for `{}`", did),
        }
    }
}

impl std::error::Error for FormatCommitError {}

/// The reference PDS's pre-publication size gates, applied before a commit
/// is formatted or persisted.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WriteLimitError {
    #[error("Too many writes. Max: 200")]
    TooManyWrites,
    #[error("Too many writes. Max event size: 2MB")]
    EventTooLarge,
}

const MAX_WRITES_PER_COMMIT: usize = 200;
const MAX_COMMIT_EVENT_BYTES: usize = 2_000_000;

/// A publication intent, committed with the write it publishes and
/// delivered to the sequencer afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishIntent {
    pub rev: String,
    pub cid: String,
    pub event_type: String,
    pub event: Vec<u8>,
}

/// A publication intent as stored, with its delivery progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredIntent {
    pub id: i64,
    pub rev: String,
    pub cid: String,
    pub event_type: String,
    pub event: Vec<u8>,
    pub state: String,
    pub seq_floor: Option<i64>,
    pub seq: Option<i64>,
}

/// The events a commit publishes: the commit itself and, for a repository's
/// first commit, the sync event the reference PDS sequences with it.
async fn intents_for(
    did: &str,
    commit: &CommitDataWithOps,
    with_sync: bool,
) -> Result<Vec<PublishIntent>> {
    let cid = commit.commit_data.cid.to_string();
    let rev = commit.commit_data.rev.clone();
    let mut intents = Vec::new();
    let append = format_seq_commit(did.to_owned(), commit.clone()).await?;
    intents.push(PublishIntent {
        rev: rev.clone(),
        cid: cid.clone(),
        event_type: append.event_type,
        event: append.event,
    });
    if with_sync {
        let sync = format_seq_sync_evt(
            did.to_owned(),
            sync_evt_data_from_commit(commit.clone()).await?,
        )
        .await?;
        intents.push(PublishIntent {
            rev,
            cid,
            event_type: sync.event_type,
            event: sync.event,
        });
    }
    Ok(intents)
}

fn stored_intent_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredIntent> {
    Ok(StoredIntent {
        id: row.get(0)?,
        rev: row.get(1)?,
        cid: row.get(2)?,
        event_type: row.get(3)?,
        event: row.get(4)?,
        state: row.get(5)?,
        seq_floor: row.get(6)?,
        seq: row.get(7)?,
    })
}

const SELECT_INTENT: &str = "SELECT id, rev, cid, \"eventType\", event, state, \"seqFloor\", seq \
                             FROM publish_intent";

/// The undelivered intents of a store, oldest first.
pub fn pending_intents_in(conn: &rusqlite::Connection) -> Result<Vec<StoredIntent>> {
    let mut stmt = conn.prepare(&format!(
        "{SELECT_INTENT} WHERE state = 'pending' ORDER BY id"
    ))?;
    let rows = stmt
        .query_map([], stored_intent_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn all_intents_in(conn: &rusqlite::Connection) -> Result<Vec<StoredIntent>> {
    let mut stmt = conn.prepare(&format!("{SELECT_INTENT} ORDER BY id"))?;
    let rows = stmt
        .query_map([], stored_intent_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn check_record_swap(
    action: &WriteOpAction,
    current_record: &Option<Cid>,
    swap_cid: &Option<Cid>,
) -> Result<(), FormatCommitError> {
    let Some(swap_cid) = swap_cid else {
        return Ok(());
    };
    // There should be no current record for a create
    if matches!(action, WriteOpAction::Create) {
        return Err(FormatCommitError::BadRecordSwap(format!(
            "{current_record:?}"
        )));
    }
    match current_record {
        Some(current_record) if current_record.eq(swap_cid) => Ok(()),
        _ => Err(FormatCommitError::RecordSwapMismatch(format!(
            "{current_record:?}"
        ))),
    }
}

fn assert_safe_path_part(part: &str) -> Result<()> {
    if part.is_empty()
        || part.starts_with('.')
        || part.contains('/')
        || part.contains('\\')
        || part.contains("..")
    {
        bail!("unsafe path part: {part}")
    }
    Ok(())
}

fn import_keypair(secret_bytes: &[u8]) -> Result<Keypair> {
    let secp = Secp256k1::new();
    let secret_key = SecretKey::from_slice(secret_bytes)?;
    Ok(Keypair::from_secret_key(&secp, &secret_key))
}

async fn load_key(location: &Path) -> Result<Option<Keypair>> {
    match tokio::fs::read(location).await {
        Ok(bytes) => Ok(Some(import_keypair(&bytes)?)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}

/// A truncating in-place rewrite of a key file destroys an unrecoverable
/// secret if the process dies mid-write, so stage and rename instead.
pub(crate) async fn atomic_write_key(location: &Path, bytes: &[u8]) -> Result<()> {
    let directory = location
        .parent()
        .ok_or_else(|| anyhow!("key location has no parent directory"))?;
    let file_name = location
        .file_name()
        .ok_or_else(|| anyhow!("key location has no file name"))?;
    let mut tmp_name = file_name.to_os_string();
    tmp_name.push(".tmp");
    let tmp_location = directory.join(tmp_name);
    {
        let mut file = tokio::fs::File::create(&tmp_location).await?;
        file.write_all(bytes).await?;
        file.sync_all().await?;
    }
    tokio::fs::rename(&tmp_location, location).await?;
    tokio::fs::File::open(directory).await?.sync_all().await?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct ActorLocation {
    pub directory: PathBuf,
    pub db_location: PathBuf,
    pub key_location: PathBuf,
}

/// Service managing per-actor sqlite stores under a root directory.
pub struct ActorStore {
    pub directory: PathBuf,
    pub reserved_key_dir: PathBuf,
    pub background_queue: BackgroundQueue,
    pub lifecycle: LifecycleStore,
    /// Another implementation shares the stores and may still serve their
    /// objects, so nothing is deleted from object storage.
    pub coexistence: bool,
    cache: Mutex<LruCache<String, CachedDb>>,
    locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    publish_locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// DIDs whose deletion is in progress; no write is admitted for them.
    tombstones: crate::lifecycle::Tombstones,
}

/// How an actor store is being opened. Reads never modify the store's
/// schema, so a store shared with another PDS implementation stays exactly
/// as that implementation left it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenMode {
    Read,
    Write,
}

#[derive(Debug, Clone)]
struct CachedDb {
    db: ActorDb,
    migrated: bool,
}

impl ActorStore {
    pub fn new(
        cfg: &ActorStoreConfig,
        background_queue: BackgroundQueue,
        lifecycle: LifecycleStore,
    ) -> Self {
        let directory = PathBuf::from(&cfg.directory);
        let reserved_key_dir = directory.join("reserved_keys");
        let cache_size = NonZeroUsize::new(cfg.cache_size.max(1)).expect("non-zero cache size");
        ActorStore {
            directory,
            reserved_key_dir,
            background_queue,
            tombstones: lifecycle.tombstones(),
            lifecycle,
            coexistence: false,
            cache: Mutex::new(LruCache::new(cache_size)),
            locks: Mutex::new(HashMap::new()),
            publish_locks: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_coexistence(mut self, coexistence: bool) -> Self {
        self.coexistence = coexistence;
        self
    }

    /// The CIDs of every blob the store registers, from the store alone.
    pub async fn blob_cids(&self, did: &str) -> Result<Vec<String>> {
        let db = self.open_db(did, OpenMode::Read).await?;
        db.run(|conn| {
            let mut stmt = conn.prepare("SELECT cid FROM blob ORDER BY cid")?;
            let cids = stmt
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(cids)
        })
        .await
    }

    pub fn get_location(&self, did: &str) -> Result<ActorLocation> {
        assert_safe_path_part(did)?;
        let did_hash = hex::encode(Sha256::digest(did.as_bytes()));
        let directory = self.directory.join(&did_hash[0..2]).join(did);
        let db_location = directory.join("store.sqlite");
        let key_location = directory.join("key");
        Ok(ActorLocation {
            directory,
            db_location,
            key_location,
        })
    }

    pub async fn exists(&self, did: &str) -> Result<bool> {
        let location = self.get_location(did)?;
        Ok(tokio::fs::try_exists(&location.db_location).await?)
    }

    pub async fn keypair(&self, did: &str) -> Result<Keypair> {
        let location = self.get_location(did)?;
        match load_key(&location.key_location).await? {
            Some(keypair) => Ok(keypair),
            None => bail!("Keypair not found for {did}"),
        }
    }

    fn did_lock(&self, did: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.locks.lock().expect("actor store locks poisoned");
        locks
            .entry(did.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    /// Serializes publication for one actor. Distinct from the write lock,
    /// which a handler still holds while it publishes the write it made.
    pub(crate) fn publish_lock(&self, did: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self
            .publish_locks
            .lock()
            .expect("actor store publish locks poisoned");
        locks
            .entry(did.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    /// The store's write connection, for the publisher and workers.
    pub(crate) async fn write_db(&self, did: &str) -> Result<ActorDb> {
        self.open_db(did, OpenMode::Write).await
    }

    /// Runs the store's journaled blob work in the background and clears
    /// the actor's pending mark once nothing is outstanding.
    pub fn queue_blob_work(&self, did: &str, blobstore: Arc<dyn BlobStore>) {
        let did = did.to_owned();
        let db = self.write_db_cached(&did);
        let lifecycle = self.lifecycle.clone();
        let background_queue = self.background_queue.clone();
        let coexistence = self.coexistence;
        self.background_queue.add(async move {
            let Some(db) = db else { return Ok(()) };
            let blob = BlobReader::new(blobstore, db.clone(), background_queue, coexistence);
            blob.run_blob_work().await?;
            settle_pending_work(&lifecycle, &db, &blob, &did).await
        });
    }

    fn write_db_cached(&self, did: &str) -> Option<ActorDb> {
        let mut cache = self.cache.lock().expect("actor store cache poisoned");
        cache
            .get(did)
            .filter(|entry| entry.migrated)
            .map(|entry| entry.db.clone())
    }

    async fn open_db(&self, did: &str, mode: OpenMode) -> Result<ActorDb> {
        {
            let mut cache = self.cache.lock().expect("actor store cache poisoned");
            if let Some(entry) = cache.get_mut(did) {
                if mode == OpenMode::Read || entry.migrated {
                    return Ok(entry.db.clone());
                }
            }
        }
        let location = self.get_location(did)?;
        if !tokio::fs::try_exists(&location.db_location).await? {
            bail!("Repo not found: {did}")
        }
        // A store may be shared with another PDS implementation, so reads
        // never touch its schema. Writes migrate on open rather than only on
        // create, so an account created before a local migration existed
        // still receives it before its first write needs the new table.
        let db = match mode {
            OpenMode::Read => get_db(&location.db_location)?,
            OpenMode::Write => get_migrated_db(&location.db_location).await?,
        };
        // ensure the db is ready (not in wal recovery mode)
        db.run(|conn| {
            let _: Option<String> = conn
                .query_row("SELECT did FROM repo_root LIMIT 1", [], |row| row.get(0))
                .optional()?;
            Ok(())
        })
        .await?;
        let mut cache = self.cache.lock().expect("actor store cache poisoned");
        cache.put(
            did.to_string(),
            CachedDb {
                db: db.clone(),
                migrated: mode == OpenMode::Write,
            },
        );
        Ok(db)
    }

    pub async fn read(
        &self,
        did: String,
        blobstore: Arc<dyn BlobStore>,
    ) -> Result<ActorStoreReader> {
        let db = self.open_db(&did, OpenMode::Read).await?;
        let key_location = self.get_location(&did)?.key_location;
        Ok(ActorStoreReader::new(
            did,
            db,
            blobstore,
            self.background_queue.clone(),
            key_location,
            self.coexistence,
        ))
    }

    pub async fn transact(
        &self,
        did: String,
        blobstore: Arc<dyn BlobStore>,
    ) -> Result<ActorStoreTransactor> {
        crate::lifecycle::assert_not_deleting(&self.tombstones, &did)?;
        let guard = self.did_lock(&did).lock_owned().await;
        let db = self.open_db(&did, OpenMode::Write).await?;
        let key_location = self.get_location(&did)?.key_location;
        let reader = ActorStoreReader::new(
            did,
            db,
            blobstore,
            self.background_queue.clone(),
            key_location,
            self.coexistence,
        );
        let keypair = reader.keypair().await?;
        Ok(ActorStoreTransactor {
            reader,
            keypair,
            lifecycle: self.lifecycle.clone(),
            _guard: guard,
        })
    }

    pub async fn create(&self, did: &str, keypair: &Keypair) -> Result<()> {
        crate::lifecycle::assert_not_deleting(&self.tombstones, did)?;
        let location = self.get_location(did)?;
        tokio::fs::create_dir_all(&location.directory).await?;
        if tokio::fs::try_exists(&location.db_location).await? {
            bail!("Repo already exists: {did}")
        }
        tokio::fs::write(&location.key_location, keypair.secret_bytes()).await?;
        let db = get_migrated_db(&location.db_location).await?;
        let mut cache = self.cache.lock().expect("actor store cache poisoned");
        cache.put(did.to_string(), CachedDb { db, migrated: true });
        Ok(())
    }

    /// Replace an actor's signing key on disk. Holds the per-DID write lock so
    /// no commit is signed with the outgoing key part-way through the swap.
    pub async fn set_keypair(&self, did: &str, keypair: &Keypair) -> Result<()> {
        let location = self.get_location(did)?;
        let _guard = self.did_lock(did).lock_owned().await;
        tokio::fs::create_dir_all(&location.directory).await?;
        atomic_write_key(&location.key_location, &keypair.secret_bytes()).await
    }

    pub async fn destroy(&self, did: &str, blobstore: Arc<dyn BlobStore>) -> Result<()> {
        self.delete_blobs(did, blobstore).await?;
        self.unlink(did).await
    }

    /// Deletes the account's objects from blob storage; failures are logged
    /// because the store is going away regardless.
    pub async fn delete_blobs(&self, did: &str, blobstore: Arc<dyn BlobStore>) -> Result<()> {
        if let Some(delete_all) = blobstore.delete_all() {
            if let Err(err) = delete_all.await {
                tracing::error!(?err, did, "failed to delete blobs from blobstore");
            }
        } else if self.exists(did).await? {
            let reader = self.read(did.to_string(), blobstore.clone()).await?;
            let blob_cids = reader.blob.get_blob_cids().await?;
            for chunk in blob_cids.chunks(500) {
                if let Err(err) = blobstore.delete_many(chunk.to_vec()).await {
                    tracing::error!(?err, did, "failed to delete blobs from blobstore");
                }
            }
        }
        Ok(())
    }

    /// Removes the actor directory and forgets the store, touching nothing
    /// in blob storage.
    pub async fn unlink(&self, did: &str) -> Result<()> {
        {
            let mut cache = self.cache.lock().expect("actor store cache poisoned");
            cache.pop(did);
        }
        {
            let mut locks = self.locks.lock().expect("actor store locks poisoned");
            locks.remove(did);
        }
        {
            let mut locks = self
                .publish_locks
                .lock()
                .expect("actor store publish locks poisoned");
            locks.remove(did);
        }
        let location = self.get_location(did)?;
        if tokio::fs::try_exists(&location.directory).await? {
            tokio::fs::remove_dir_all(&location.directory).await?;
        }
        Ok(())
    }

    pub async fn reserve_keypair(&self, did: Option<&str>) -> Result<String> {
        let mut key_location: Option<PathBuf> = None;
        if let Some(did) = did {
            assert_safe_path_part(did)?;
            let loc = self.reserved_key_dir.join(did);
            if let Some(keypair) = load_key(&loc).await? {
                return Ok(encode_did_key(&keypair.public_key()));
            }
            key_location = Some(loc);
        }
        let secp = Secp256k1::new();
        let (secret_key, public_key) = secp.generate_keypair(&mut rand::thread_rng());
        let key_did = encode_did_key(&public_key);
        let key_location = key_location.unwrap_or_else(|| self.reserved_key_dir.join(&key_did));
        tokio::fs::create_dir_all(&self.reserved_key_dir).await?;
        tokio::fs::write(&key_location, secret_key.secret_bytes()).await?;
        Ok(key_did)
    }

    pub async fn get_reserved_keypair(&self, signing_key_or_did: &str) -> Result<Option<Keypair>> {
        assert_safe_path_part(signing_key_or_did)?;
        load_key(&self.reserved_key_dir.join(signing_key_or_did)).await
    }

    pub async fn clear_reserved_keypair(&self, key_did: &str, did: Option<&str>) -> Result<()> {
        assert_safe_path_part(key_did)?;
        let key_loc = self.reserved_key_dir.join(key_did);
        if tokio::fs::try_exists(&key_loc).await? {
            tokio::fs::remove_file(&key_loc).await?;
        }
        if let Some(did) = did {
            assert_safe_path_part(did)?;
            let did_loc = self.reserved_key_dir.join(did);
            if tokio::fs::try_exists(&did_loc).await? {
                tokio::fs::remove_file(&did_loc).await?;
            }
        }
        Ok(())
    }
}

/// Read access to a single actor's store.
pub struct ActorStoreReader {
    pub did: String,
    pub storage: Arc<RwLock<SqlRepoReader>>, // get ipld blocks from db
    pub record: RecordReader,                // get lexicon records from db
    pub blob: BlobReader,                    // get blobs
    pub pref: PreferenceReader,              // get preferences
    pub space: SpaceStore,                   // permissioned repos + space definitions
    key_location: PathBuf,
}

impl ActorStoreReader {
    fn new(
        did: String,
        db: ActorDb,
        blobstore: Arc<dyn BlobStore>,
        background_queue: BackgroundQueue,
        key_location: PathBuf,
        coexistence: bool,
    ) -> Self {
        ActorStoreReader {
            storage: Arc::new(RwLock::new(SqlRepoReader::new(
                did.clone(),
                None,
                db.clone(),
            ))),
            record: RecordReader::new(did.clone(), db.clone()),
            pref: PreferenceReader::new(did.clone(), db.clone()),
            space: SpaceStore::new(did.clone(), db.clone()),
            blob: BlobReader::new(blobstore, db, background_queue, coexistence),
            did,
            key_location,
        }
    }

    /// The store's undelivered publication intents, oldest first.
    pub async fn pending_intents(&self) -> Result<Vec<StoredIntent>> {
        self.record.db.run(|conn| pending_intents_in(conn)).await
    }

    pub async fn all_intents(&self) -> Result<Vec<StoredIntent>> {
        self.record.db.run(|conn| all_intents_in(conn)).await
    }

    pub async fn keypair(&self) -> Result<Keypair> {
        match load_key(&self.key_location).await? {
            Some(keypair) => Ok(keypair),
            None => bail!("Keypair not found for {}", self.did),
        }
    }

    pub async fn get_repo_root(&self) -> Option<Cid> {
        let storage_guard = self.storage.read().await;
        storage_guard.get_root().await
    }

    pub async fn get_sync_event_data(&self) -> Result<SyncEvtData> {
        let storage_guard = self.storage.read().await;
        let current_root = storage_guard.get_root_detailed().await?;
        let blocks_and_missing = storage_guard.get_blocks(vec![current_root.cid]).await?;
        Ok(SyncEvtData {
            cid: current_root.cid,
            rev: current_root.rev,
            blocks: blocks_and_missing.blocks,
        })
    }
}

/// Write access to a single actor's store. Holds the per-DID write lock
/// for its lifetime so writers for the same actor serialize.
pub struct ActorStoreTransactor {
    reader: ActorStoreReader,
    pub keypair: Keypair,
    lifecycle: LifecycleStore,
    _guard: OwnedMutexGuard<()>,
}

impl Deref for ActorStoreTransactor {
    type Target = ActorStoreReader;

    fn deref(&self) -> &Self::Target {
        &self.reader
    }
}

impl DerefMut for ActorStoreTransactor {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.reader
    }
}

impl ActorStoreTransactor {
    /// Creates the repository. The reference PDS sequences a creation batch
    /// only for an active account, so `publish` is false for an account
    /// created deactivated, whose repository is announced by its activation.
    pub async fn create_repo(
        &self,
        writes: Vec<PreparedCreateOrUpdate>,
        publish: bool,
    ) -> Result<CommitDataWithOps> {
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
            &self.keypair,
            Some(write_ops),
        )
        .await?;
        let write_commit_ops = writes.iter().try_fold(
            Vec::with_capacity(writes.len()),
            |mut acc, w| -> Result<Vec<CommitOp>> {
                let aturi: AtUri = w.uri.clone().try_into()?;
                acc.push(CommitOp {
                    action: CommitAction::Create,
                    path: format_data_key(aturi.get_collection(), aturi.get_rkey()),
                    cid: Some(w.cid),
                    prev: None,
                });
                Ok(acc)
            },
        )?;
        let writes = writes
            .into_iter()
            .map(PreparedWrite::Create)
            .collect::<Vec<PreparedWrite>>();
        let commit = CommitDataWithOps {
            commit_data: commit,
            ops: write_commit_ops,
            prev_data: None,
        };
        let intents = match publish {
            true => intents_for(&self.did, &commit, true).await?,
            false => vec![],
        };
        let promoted = self.blob.promote_write_blobs(&writes, false).await?;
        self.commit_write(
            &commit.commit_data,
            None,
            &writes,
            &promoted,
            &intents,
            false,
        )
        .await?;
        Ok(commit)
    }

    /// An import replaces the repository without publishing it, as the
    /// reference PDS does; a blob it references may arrive afterwards.
    pub async fn process_import_repo(
        &mut self,
        commit: CommitData,
        writes: Vec<PreparedWrite>,
    ) -> Result<()> {
        let current_root = {
            let storage_guard = self.storage.read().await;
            storage_guard.get_root().await
        };
        let promoted = self.blob.promote_write_blobs(&writes, true).await?;
        // an intent from before the import names a root the import
        // replaced; publishing it now would be a commit behind the head
        self.commit_write(&commit, current_root, &writes, &promoted, &[], true)
            .await
    }

    pub async fn process_writes(
        &mut self,
        writes: Vec<PreparedWrite>,
        swap_commit_cid: Option<Cid>,
    ) -> Result<CommitDataWithOps> {
        if writes.len() > MAX_WRITES_PER_COMMIT {
            return Err(WriteLimitError::TooManyWrites.into());
        }
        let (commit, current_root) = self.format_commit(writes.clone(), swap_commit_cid).await?;
        if commit.commit_data.relevant_blocks.byte_size()? > MAX_COMMIT_EVENT_BYTES {
            return Err(WriteLimitError::EventTooLarge.into());
        }
        let intents = intents_for(&self.did, &commit, false).await?;
        let promoted = self.blob.promote_write_blobs(&writes, false).await?;
        self.commit_write(
            &commit.commit_data,
            Some(current_root.cid),
            &writes,
            &promoted,
            &intents,
            false,
        )
        .await?;
        Ok(commit)
    }

    /// Persists a prepared commit as one transaction: the root is swapped
    /// only if it still equals `expected_root`, and the blocks, record
    /// index, blob bookkeeping, and publication intents land with it or not
    /// at all. The actor is marked in the lifecycle journal first, so a
    /// restart knows to look here for undelivered intents.
    async fn commit_write(
        &self,
        commit: &CommitData,
        expected_root: Option<Cid>,
        writes: &[PreparedWrite],
        promoted: &[PromotedBlob],
        intents: &[PublishIntent],
        supersede_pending: bool,
    ) -> Result<()> {
        self.lifecycle.mark_pending_work(&self.did).await?;
        let did = self.did.clone();
        let now = self.storage.read().await.now.clone();
        let commit = commit.clone();
        let writes = writes.to_vec();
        let promoted = promoted.to_vec();
        let intents = intents.to_vec();
        let coexistence = self.blob.coexistence;
        let new_blocks = commit.new_blocks.clone();
        self.record
            .db
            .tx(move |tx| {
                apply_commit_in(tx, &did, &now, &commit, expected_root.as_ref())?;
                for write in &writes {
                    match write {
                        PreparedWrite::Create(w) | PreparedWrite::Update(w) => {
                            let uri: AtUri = w.uri.clone().try_into()?;
                            index_record_in(
                                tx,
                                &uri,
                                &w.cid,
                                Some(&w.record),
                                w.action.clone(),
                                &commit.rev,
                                &now,
                            )?;
                        }
                        PreparedWrite::Delete(w) => {
                            let uri: AtUri = w.uri.clone().try_into()?;
                            delete_record_in(tx, &uri)?;
                        }
                    }
                }
                apply_write_blobs_in(tx, &writes, &promoted, coexistence, &now)?;
                if supersede_pending {
                    tx.execute(
                        "UPDATE publish_intent SET state = 'superseded' WHERE state = 'pending'",
                        [],
                    )?;
                }
                let mut insert = tx.prepare_cached(
                    "INSERT INTO publish_intent (rev, cid, \"eventType\", event, \"createdAt\") \
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                )?;
                for intent in &intents {
                    insert.execute(rusqlite::params![
                        intent.rev,
                        intent.cid,
                        intent.event_type,
                        intent.event,
                        now
                    ])?;
                }
                Ok(())
            })
            .await?;
        let storage_guard = self.storage.read().await;
        let mut cache_guard = storage_guard.cache.write().await;
        cache_guard.add_map(new_blocks)?;
        Ok(())
    }

    /// Formats a commit and returns the root it was formatted against, which
    /// is the root the commit may replace.
    pub async fn format_commit(
        &mut self,
        writes: Vec<PreparedWrite>,
        swap_commit: Option<Cid>,
    ) -> Result<(CommitDataWithOps, CidAndRev)> {
        let current_root = {
            let storage_guard = self.storage.read().await;
            storage_guard.get_root_detailed().await
        };
        let Ok(current_root) = current_root else {
            return Err(FormatCommitError::MissingRepoRoot(self.did.clone()).into());
        };
        if let Some(swap_commit) = swap_commit {
            if !current_root.cid.eq(&swap_commit) {
                return Err(FormatCommitError::BadCommitSwap(current_root.cid.to_string()).into());
            }
        }
        {
            let mut storage_guard = self.storage.write().await;
            storage_guard.cache_rev(current_root.rev.clone()).await?;
        }
        let mut new_record_cids: Vec<Cid> = vec![];
        let mut delete_and_update_uris = vec![];
        let mut commit_ops = vec![];
        for write in &writes {
            let commit_action: CommitAction = write.action().into();
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
            let write_at_uri: &AtUri = &write.uri().try_into()?;
            // op.prev must be populated for every update/delete,
            // not only swap-checked writes
            let record = self
                .reader
                .record
                .get_record(write_at_uri, None, Some(true))
                .await?;
            let current_record = match record {
                Some(record) => Some(Cid::from_str(&record.cid)?),
                None => None,
            };
            let cid = match &write {
                &PreparedWrite::Delete(_) => None,
                &PreparedWrite::Create(w) | &PreparedWrite::Update(w) => Some(w.cid),
            };
            commit_ops.push(CommitOp {
                action: commit_action,
                path: format_data_key(write_at_uri.get_collection(), write_at_uri.get_rkey()),
                cid,
                prev: current_record,
            });
            check_record_swap(write.action(), &current_record, write.swap_cid())?;
        }
        let mut repo = Repo::load(self.storage.clone(), Some(current_root.cid)).await?;
        let previous_data = repo.commit.data;
        let formatted_against = CidAndRev {
            cid: current_root.cid,
            rev: current_root.rev.clone(),
        };
        let write_ops: Vec<RecordWriteOp> = writes
            .into_iter()
            .map(write_to_op)
            .collect::<Result<Vec<RecordWriteOp>>>()?;

        let mut commit = repo
            .format_commit(RecordWriteEnum::List(write_ops), &self.keypair)
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
        Ok((
            CommitDataWithOps {
                ops: commit_ops,
                commit_data: commit,
                prev_data: Some(previous_data),
            },
            formatted_against,
        ))
    }

    pub async fn get_duplicate_record_cids(
        &self,
        cids: Vec<Cid>,
        touched_uris: Vec<AtUri>,
    ) -> Result<Vec<Cid>> {
        if touched_uris.is_empty() || cids.is_empty() {
            return Ok(vec![]);
        }
        let cid_strs: Vec<String> = cids.into_iter().map(|c| c.to_string()).collect();
        let touched_uri_strs: Vec<String> = touched_uris.iter().map(|t| t.to_string()).collect();
        let res: Vec<String> = self
            .record
            .db
            .run(move |conn| {
                let sql = format!(
                    "SELECT cid FROM record WHERE cid IN ({}) AND uri NOT IN ({})",
                    crate::actor_store::repo::sql_repo::placeholders(cid_strs.len()),
                    crate::actor_store::repo::sql_repo::placeholders(touched_uri_strs.len())
                );
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt
                    .query_map(
                        rusqlite::params_from_iter(cid_strs.iter().chain(touched_uri_strs.iter())),
                        |row| row.get::<_, String>(0),
                    )?
                    .collect::<Result<Vec<String>, rusqlite::Error>>()?;
                Ok(rows)
            })
            .await?;
        res.into_iter()
            .map(|row| Cid::from_str(&row).map_err(anyhow::Error::new))
            .collect::<Result<Vec<Cid>>>()
    }
}

#[cfg(test)]
mod tests;
