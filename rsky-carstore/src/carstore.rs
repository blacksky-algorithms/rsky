use crate::last_shard_cache::{LastShardCache, LastShardError, LastShardSource};
use crate::meta::{CarShard, CarStoreSurreal};
use chrono::prelude::*;
use lexicon_cid::Cid;
use rsky_common::models::Uid;
use rsky_repo::block_map::BlockMap;
use rsky_repo::cid_set::CidSet;
use rsky_repo::vendored::iroh_car;
use rsky_repo::vendored::iroh_car::{CarHeader, CarReader, CarWriter};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use slog::{o, Discard, Logger};
use std::collections::HashMap;
use std::future::Future;
use std::io::Write;
use std::pin::Pin;
use std::sync::Arc;
use surrealdb::Error as SurrealError;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::sync::{Mutex, RwLock};
use tokio::{fs, io};
use tracing::{info_span, Instrument};
use unsigned_varint::decode as varint_decode;

#[derive(Debug, Error)]
pub enum CarStoreError {
    #[error("SurrealDbError: {0}")]
    SurrealDbError(#[from] SurrealError),
    #[error("CID mismatch")]
    CidMismatch,
    #[error("CarStore: NotFound: {0}")]
    NotFound(String),
    #[error("IoError: {0}")]
    Io(#[from] io::Error),
    #[error("CID parse error: {0}")]
    Cid(#[from] lexicon_cid::Error),
    #[error("Varint decode error")]
    DecodeError,
    #[error("RepoBaseMismatch: {0}")]
    RepoBaseMismatch(String),
    #[error("CarError: {0}")]
    CarError(#[from] iroh_car::error::Error),
    #[error("CarStoreError: {0}")]
    Error(String),
}

/// A convenience alias for functions returning `CarStoreError`.
pub type CarStoreResult<T> = Result<T, CarStoreError>;

pub trait CarStore: Send + Sync {
    fn compact_user_shards<'a>(
        &'a self,
        user: &'a Uid,
        skip_big_shards: bool,
    ) -> Pin<Box<dyn Future<Output = CarStoreResult<Arc<CompactionStats>>> + Send + Sync + 'a>>;

    fn get_compaction_targets<'a>(
        &'a self,
        shard_count: i32,
    ) -> Pin<Box<dyn Future<Output = CarStoreResult<Vec<CompactionTarget>>> + Send + Sync + 'a>>;

    fn get_user_repo_head<'a>(
        &'a self,
        user: &'a Uid,
    ) -> Pin<Box<dyn Future<Output = CarStoreResult<Cid>> + Send + Sync + 'a>>;

    fn get_user_repo_rev<'a>(
        &'a self,
        user: &'a Uid,
    ) -> Pin<Box<dyn Future<Output = CarStoreResult<String>> + Send + Sync + 'a>>;

    fn import_slice<'a>(
        &'a self,
        uid: &'a Uid,
        since: Option<&'a str>,
        carslice: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = CarStoreResult<(Cid, Arc<DeltaSession>)>> + Send + Sync + 'a>>;

    fn new_delta_session<'a>(
        &'a self,
        user: &'a Uid,
        since: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = CarStoreResult<Arc<DeltaSession>>> + Send + Sync + 'a>>;

    fn read_only_session<'a>(
        &'a self,
        user: &'a Uid,
    ) -> Pin<Box<dyn Future<Output = CarStoreResult<Arc<DeltaSession>>> + Send + Sync + 'a>>;

    fn read_user_car<'a>(
        &'a self,
        user: &'a Uid,
        since_rev: &'a str,
        incremental: bool,
        w: &'a mut (dyn Write + Send),
    ) -> Pin<Box<dyn Future<Output = CarStoreResult<()>> + Send + Sync + 'a>>;

    fn stat<'a>(
        &'a self,
        usr: &'a Uid,
    ) -> Pin<Box<dyn Future<Output = CarStoreResult<Vec<UserStat>>> + Send + Sync + 'a>>;

    fn wipe_user_data<'a>(
        &'a self,
        user: &'a Uid,
    ) -> Pin<Box<dyn Future<Output = CarStoreResult<()>> + Send + Sync + 'a>>;
}

pub trait ShardWriter: Send {
    fn write_new_shard<'a>(
        &'a self,
        root: &'a Cid,
        rev: &'a String,
        user: &'a Uid,
        seq: &'a i64,
        blocks: HashMap<Cid, Vec<u8>>,
        rmcids: CidSet,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, CarStoreError>> + Send + 'a>>;
}

impl LastShardSource for CarStoreSurreal {
    fn get_last_shard<'a>(
        &'a self,
        uid: &'a Uid,
    ) -> Pin<Box<dyn Future<Output = Result<Arc<CarShard>, LastShardError>> + Send + Sync + 'a>>
    {
        Box::pin(async move {
            match self.get_last_shard(uid).await {
                // If there's an existing shard, wrap it in an Arc
                Ok(Some(shard)) => Ok(Arc::new(shard)),
                // If none found, return an error
                Ok(None) => Err(LastShardError::Error(format!(
                    "No shard found for user: {}",
                    uid
                ))),
                // Translate Surreal or CarStoreError to LastShardError
                Err(e) => Err(LastShardError::Error(format!(
                    "DB error while fetching last shard: {e:?}"
                ))),
            }
        })
    }
}

#[derive(Clone)]
pub struct FileCarStore {
    pub meta: Arc<CarStoreSurreal>,
    pub root_dirs: Vec<String>,
    pub last_shard_cache: LastShardCache,
    pub log: Logger,
}

impl FileCarStore {
    /// Equivalent to `NewCarStore` in Go:
    /// - Creates/validates the data directories
    /// - Initializes the SurrealDB store
    /// - Sets up the lastShardCache
    /// - Returns a fully constructed `FileCarStore`
    pub async fn new_car_store(roots: Vec<String>) -> Result<Self, CarStoreError> {
        // 1. Create or open the SurrealDB store
        let store = CarStoreSurreal::new().await?;
        store.init().await?;

        // 2. Ensure each root directory exists
        for root in &roots {
            match fs::metadata(root).await {
                Ok(_) => {
                    // Directory exists; do nothing
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    // If not found, create it (similar to os.Mkdir in Go)
                    fs::create_dir_all(root).await?;
                }
                Err(e) => {
                    // Some other error, bail out
                    return Err(CarStoreError::Io(e));
                }
            }
        }

        // 3. Wrap the Surreal store in an Arc, so it can be shared
        let meta_arc = Arc::new(store);

        // 4. Construct LastShardCache.
        //    Because `CarStoreSurreal` now implements `LastShardSource`,
        //    we can pass `meta_arc.clone()` as the source.
        let last_shard_cache = LastShardCache {
            source: meta_arc.clone(),
            last_shard_cache: Arc::new(Mutex::new(HashMap::new())),
        };

        last_shard_cache.init().await;

        // 5. Create a logger. Equivalent to slog.Default().With("system", "carstore")
        let log = Logger::root(Discard, o!("system" => "carstore"));

        // 6. Build and return the FileCarStore
        Ok(Self {
            meta: meta_arc,
            root_dirs: roots,
            last_shard_cache,
            log,
        })
    }

    pub async fn check_last_shard_cache(&self, user: &Uid) -> Option<Arc<CarShard>> {
        self.last_shard_cache.check(user).await
    }

    pub async fn remove_last_shard_cache(&self, user: &Uid) {
        self.last_shard_cache.remove(user).await
    }

    pub async fn put_last_shard_cache(&self, last_shard: Arc<CarShard>) {
        self.last_shard_cache.put(last_shard).await
    }

    pub async fn get_last_shard_cache(&self, user: &Uid) -> Result<Arc<CarShard>, LastShardError> {
        self.last_shard_cache.get(user).await
    }

    /// Go: func (cs *FileCarStore) NewDeltaSession(ctx context.Context, user models.Uid, since *string) (*DeltaSession, error)
    ///
    /// Rust version: Asynchronous, returns a `Result<DeltaSession, CarStoreError>`.
    /// The `ctx` parameter is omitted or replaced by instrumentation via `tracing`.
    pub async fn new_delta_session(
        &self,
        user: Uid,
        since: Option<&str>,
    ) -> Result<DeltaSession, CarStoreError> {
        // Optional: start a trace span
        let span = info_span!("NewSession");
        async move {
            // 1. getLastShard
            let last_shard = self.get_last_shard_cache(&user).await.map_err(|e| {
                CarStoreError::Error(format!("Failed to retrieve last shard: {}", e))
            })?;

            // 2. If `since` is provided, compare to lastShard.rev
            if let Some(since_rev) = since {
                if since_rev != last_shard.rev {
                    return Err(CarStoreError::RepoBaseMismatch(format!(
                        "{since_rev} != {}",
                        last_shard.rev
                    )));
                }
            }

            // 3. Build and return the DeltaSession
            Ok(DeltaSession {
                blocks: BlockMap::new(),
                rmcids: CidSet::new(None),
                base: Arc::new(UserView {
                    user: user.clone(),
                    cs: self.meta.clone(),
                    prefetch: true,
                    cache: Arc::new(RwLock::new(HashMap::new())),
                }),
                user: user.clone(),
                base_cid: last_shard.root,
                seq: last_shard.seq + 1,
                read_only: false,
                cs: Arc::new(self.clone()),
                last_rev: last_shard.rev.clone(),
            })
        }
        .instrument(span)
        .await
    }

    /// Writes the CAR bytes to disk, returning a file path for the new shard file
    /// (Equivalent to `cs.writeNewShardFile(ctx, user, seq, buf.Bytes())`).
    async fn write_new_shard_file(
        &self,
        user: &Uid,
        seq: i64,
        car_data: &[u8],
    ) -> Result<String, CarStoreError> {
        // 1. Decide where to place the file. E.g. combine user + seq
        let path = format!("/tmp/carshard_{user}_{seq}.car"); // or your real logic

        // 2. Write to disk
        tokio::fs::write(&path, car_data)
            .await
            .map_err(|e| CarStoreError::Error(format!("failed to write shard file: {e}")))?;

        Ok(path)
    }

    pub async fn put_shard(
        &self,
        shard: CarShard,
        brefs: Vec<HashMap<String, JsonValue>>,
        rmcids: &CidSet,
        nocache: bool,
    ) -> Result<(), CarStoreError> {
        // 1. Insert the shard + references in the DB
        self.meta
            .put_shard_and_refs(shard.clone(), brefs, rmcids)
            .await?;

        // 2. Update the last shard cache unless "nocache" is true
        if !nocache {
            self.put_last_shard_cache(Arc::new(shard)).await;
        }

        Ok(())
    }
}

impl ShardWriter for FileCarStore {
    fn write_new_shard<'a>(
        &'a self,
        root: &'a Cid,
        rev: &'a String,
        user: &'a Uid,
        seq: &'a i64,
        blocks: HashMap<Cid, Vec<u8>>,
        rmcids: CidSet,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, CarStoreError>> + Send + 'a>> {
        Box::pin(async move {
            let mut buffer = Vec::new();
            // Before writing any block, note the current "offset" in the buffer.
            // The header is written as soon as we call `writer.write(...)` for the first time.
            // We'll detect that offset after the header is done.
            //
            // The simplest approach: measure buffer length before each block,
            // then measure it again after the write to see how many bytes were added.
            let mut offset_before_first_block = buffer.len();

            let header = CarHeader::new_v1(vec![root.clone()]);

            let mut writer = CarWriter::new(header, &mut buffer);

            let mut brefs = Vec::with_capacity(blocks.len());

            for (cid, data) in &blocks {
                // measure offset at this point in the buffer
                let offset_start = offset_before_first_block;

                // 2a. Write the block
                writer
                    .write(cid.clone(), data)
                    .await
                    .map_err(|e| CarStoreError::Error(format!("failed to write block: {e}")))?;

                // 2b. Now measure how many bytes have been added
                let offset_end = writer.current_offset();
                offset_before_first_block = offset_end;

                // Save a block reference record
                let mut bref = HashMap::new();
                bref.insert("cid".to_string(), serde_json::json!(cid.to_string()));
                bref.insert("offset".to_string(), serde_json::json!(offset_start as i64));
                // We'll set "shard" after we actually create the shard, or in put_shard_and_refs

                brefs.push(bref);
            }

            // 3. Finish the writer to flush all data
            //    This also ensures the header is properly written if no blocks were written
            writer
                .finish()
                .await
                .map_err(|e| CarStoreError::Error(format!("failed to finish car writer: {e}")))?;

            // At this point, `buffer` contains the entire CAR (header + blocks).
            // The final offset is buffer.len().

            // 4. Write the CAR bytes to a new shard file
            let path = self
                .write_new_shard_file(&user, *seq, &buffer)
                .await
                .map_err(|e| CarStoreError::Error(format!("failed to write shard file: {e}")))?;

            // 5. Construct the CarShard
            //    In your Go code, you do: CarShard{ Root, DataStart = hnw, Seq, Path, Usr, Rev }
            //    We'll guess a few more fields exist (like created_at).
            let shard = CarShard {
                id: None,
                root: root.clone(),
                data_start: 0, // If you want the real offset of the "first block," you could store it
                seq: *seq,
                path,
                usr: user.clone(),
                rev: rev.to_string(),
                created_at: Utc::now(),
            };

            // 6. Insert the shard + block refs + stale references
            self.put_shard(shard, brefs, &rmcids, false)
                .await
                .map_err(|e| {
                    CarStoreError::Error(format!("failed to store shard metadata: {e}"))
                })?;

            // 7. Return the full in-memory CAR bytes
            Ok(buffer)
        })
    }
}

/// subset of Blockstore that we actually use here
pub trait MinBlockstore: Send + Sync {
    fn get<'a>(
        &'a self,
        bcid: &'a Cid,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, CarStoreError>> + Send + 'a>>;

    fn has<'a>(
        &'a self,
        bcid: &'a Cid,
    ) -> Pin<Box<dyn Future<Output = Result<bool, CarStoreError>> + Send + 'a>>;

    fn get_size<'a>(
        &'a self,
        bcid: &'a Cid,
    ) -> Pin<Box<dyn Future<Output = Result<usize, CarStoreError>> + Send + 'a>>;
}

// userView needs these things to get into the underlying block store
// implemented by CarStoreMeta
pub trait UserViewSource: Send + Sync {
    fn has_uid_cid<'a>(
        &'a self,
        user: &'a Uid,
        k: &'a Cid,
    ) -> Pin<Box<dyn Future<Output = Result<bool, CarStoreError>> + Send + Sync + 'a>>;

    fn lookup_block_ref<'a>(
        &'a self,
        k: &'a Cid,
    ) -> Pin<Box<dyn Future<Output = Result<(String, i64, Uid), CarStoreError>> + Send + Sync + 'a>>;
}

/// wrapper into a block store that keeps track of which user we are working on behalf of
pub struct UserView {
    pub cs: Arc<dyn UserViewSource>,
    pub user: Uid,
    pub cache: Arc<RwLock<HashMap<Cid, Vec<u8>>>>,
    pub prefetch: bool,
}

impl MinBlockstore for UserView {
    fn get<'a>(
        &'a self,
        bcid: &'a Cid,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, CarStoreError>> + Send + 'a>> {
        Box::pin(async move {
            // Check cache
            {
                let cache = self.cache.read().await;
                if let Some(data) = cache.get(bcid) {
                    return Ok(data.clone());
                }
            }

            // Lookup block reference
            let (path, offset, user) = self.cs.lookup_block_ref(bcid).await?;

            let prefetch = if user != self.user {
                false
            } else {
                self.prefetch
            };

            if prefetch {
                // Check file size to decide on prefetch
                let metadata = tokio::fs::metadata(&path).await?;
                if metadata.len() <= 512 * 1024 {
                    // 512KB threshold
                    // Read entire CAR file and cache blocks
                    let file = fs::File::open(&path).await?;
                    let mut car_reader = CarReader::new(file).await?;

                    let mut cache = self.cache.write().await;
                    while let Some(block) = car_reader.next_block().await? {
                        let cid = block.0;
                        cache.insert(cid, block.1);
                    }

                    // Retrieve from cache after prefetch
                    if let Some(data) = cache.get(bcid) {
                        Ok(data.clone())
                    } else {
                        Err(CarStoreError::NotFound("block not found".to_string()))
                    }
                } else {
                    // File too large, read single block
                    read_single_block(&path, offset, bcid).await
                }
            } else {
                // Read single block without prefetch
                read_single_block(&path, offset, bcid).await
            }
        })
    }

    fn has<'a>(
        &'a self,
        bcid: &'a Cid,
    ) -> Pin<Box<dyn Future<Output = Result<bool, CarStoreError>> + Send + 'a>> {
        Box::pin(async move {
            {
                let cache = self.cache.read().await;
                if cache.contains_key(bcid) {
                    return Ok(true);
                }
            }
            self.cs.has_uid_cid(&self.user, bcid).await
        })
    }

    fn get_size<'a>(
        &'a self,
        bcid: &'a Cid,
    ) -> Pin<Box<dyn Future<Output = Result<usize, CarStoreError>> + Send + 'a>> {
        Box::pin(async move {
            let data = self.get(bcid).await?;
            Ok(data.len())
        })
    }
}

async fn read_single_block(
    path: &str,
    offset: i64,
    expected_cid: &Cid,
) -> Result<Vec<u8>, CarStoreError> {
    let mut file = fs::File::open(path).await?;
    file.seek(io::SeekFrom::Start(offset as u64)).await?;

    // Read varint length
    let mut varint_buf = [0u8; 10];
    let mut bytes_read = 0;
    loop {
        let byte = file.read_u8().await?;
        varint_buf[bytes_read] = byte;
        bytes_read += 1;
        if byte < 0x80 {
            break;
        }
    }
    let (len, _) =
        varint_decode::u64(&varint_buf[..bytes_read]).map_err(|_| CarStoreError::DecodeError)?;

    // Read CID and data
    let mut cid_and_data = vec![0u8; len as usize];
    file.read_exact(&mut cid_and_data).await?;

    // Parse CID and data
    let (cid, data) = parse_cid_and_data(&cid_and_data)?;
    if cid != *expected_cid {
        return Err(CarStoreError::CidMismatch);
    }

    Ok(data.to_vec())
}

fn parse_cid_and_data(bytes: &[u8]) -> Result<(Cid, &[u8]), CarStoreError> {
    let mut cursor = std::io::Cursor::new(bytes);
    let cid = Cid::read_bytes(&mut cursor)?;
    let data = &bytes[cursor.position() as usize..];
    Ok((cid, data))
}

pub struct DeltaSession {
    pub blocks: BlockMap,
    pub rmcids: CidSet,
    pub base: Arc<dyn MinBlockstore>,
    pub user: Uid,
    pub base_cid: Cid,
    pub seq: i64,
    pub read_only: bool,
    pub cs: Arc<dyn ShardWriter>,
    pub last_rev: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CompactionTarget {
    pub usr: Uid,
    pub num_shards: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserStat {
    pub seq: usize,
    pub root: String,
    pub created: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CompactionStats {
    pub total_refs: usize,
    pub start_shards: usize,
    pub new_shards: usize,
    pub skipped_shards: usize,
    pub shards_deleted: usize,
    pub dupe_count: usize,
}
