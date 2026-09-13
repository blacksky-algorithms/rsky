use crate::actor_store::aws::s3::S3BlobStore;
use crate::actor_store::disk_blobstore::DiskBlobStore;
use crate::config::{BlobstoreConfig, S3Config};
use anyhow::{bail, Result};
use aws_config::SdkConfig;
use aws_sdk_s3::primitives::ByteStream;
use futures::future::BoxFuture;
use lexicon_cid::Cid;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
#[derive(Debug, thiserror::Error)]
#[error("Blob not found")]
pub struct BlobNotFoundError;

/// The content's object cannot be served or reused until an outstanding
/// delete against its key has a confirmed outcome.
#[derive(Debug, thiserror::Error)]
#[error("BlobUnavailable")]
pub struct BlobUnavailableError;

/// Why a delete by physical key did not complete: a refusal the service
/// confirmed, or no confirmation at all.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum DeleteError {
    #[error("delete refused: {0}")]
    Definitive(String),
    #[error("delete unconfirmed: {0}")]
    Ambiguous(String),
}

/// Which of an actor's object families a physical key belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectKind {
    Permanent,
    Temp,
    Quarantine,
}

/// Object storage for blob bytes, keyed by actor.
/// Mirrors the BlobStore interface from the reference implementation.
pub trait BlobStore: Send + Sync {
    fn put_temp(&self, bytes: Vec<u8>) -> BoxFuture<'_, Result<String>>;
    /// Stores a spooled upload as a temporary object without holding it
    /// in memory where the store allows it.
    fn put_temp_from_path(&self, path: std::path::PathBuf) -> BoxFuture<'_, Result<String>> {
        Box::pin(async move {
            let bytes = tokio::fs::read(path).await?;
            self.put_temp(bytes).await
        })
    }
    fn make_permanent(&self, key: String, cid: Cid) -> BoxFuture<'_, Result<()>>;
    fn put_permanent(&self, cid: Cid, bytes: Vec<u8>) -> BoxFuture<'_, Result<()>>;
    fn quarantine(&self, cid: Cid) -> BoxFuture<'_, Result<()>>;
    fn unquarantine(&self, cid: Cid) -> BoxFuture<'_, Result<()>>;
    fn get_bytes(&self, cid: Cid) -> BoxFuture<'_, Result<Vec<u8>>>;
    fn get_stream(&self, cid: Cid) -> BoxFuture<'_, Result<ByteStream>>;
    fn has_temp(&self, key: String) -> BoxFuture<'_, Result<bool>>;
    fn has_stored(&self, cid: Cid) -> BoxFuture<'_, Result<bool>>;
    fn delete(&self, cid: Cid) -> BoxFuture<'_, Result<()>>;
    fn delete_many(&self, cids: Vec<Cid>) -> BoxFuture<'_, Result<()>>;
    /// Stores that can wipe an actor's blobs wholesale return a future;
    /// others return None and callers fall back to per-cid deletion.
    fn delete_all(&self) -> Option<BoxFuture<'_, Result<()>>> {
        None
    }
    /// Copies a temporary object to its permanent key and leaves the
    /// temporary object in place, for a store another implementation may
    /// still read.
    fn make_permanent_copy_only(&self, key: String, cid: Cid) -> BoxFuture<'_, Result<()>>;
    fn has_quarantined(&self, cid: Cid) -> BoxFuture<'_, Result<bool>>;
    /// Copies a quarantined object back to its permanent key and leaves the
    /// quarantined object in place.
    fn restore_copy_only(&self, cid: Cid) -> BoxFuture<'_, Result<()>>;

    /// The prefixes under which every object of this actor lives.
    fn namespace_prefixes(&self) -> Vec<String> {
        Vec::new()
    }
    /// The physical key of an object: a permanent object at a generation,
    /// a temporary object by its key, or a quarantined object.
    fn object_key(&self, kind: ObjectKind, name: &str, generation: u32) -> String {
        let name = crate::blob_generations::generation_name(name, generation);
        match kind {
            ObjectKind::Permanent => format!("permanent/{name}"),
            ObjectKind::Temp => format!("temp/{name}"),
            ObjectKind::Quarantine => format!("quarantine/{name}"),
        }
    }
    /// Every physical key under `prefix`.
    fn list_objects(&self, prefix: String) -> BoxFuture<'_, Result<Vec<String>>> {
        Box::pin(async move { bail!("listing {prefix} is not supported by this store") })
    }
    /// Deletes one object by physical key, distinguishing a refusal from a
    /// request whose outcome is unknown.
    fn delete_object(&self, key: String) -> BoxFuture<'_, std::result::Result<(), DeleteError>> {
        Box::pin(async move {
            Err(DeleteError::Definitive(format!(
                "deleting {key} is not supported by this store"
            )))
        })
    }
    fn object_exists(&self, key: String) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async move { bail!("checking {key} is not supported by this store") })
    }
    fn get_object(&self, key: String) -> BoxFuture<'_, Result<Vec<u8>>> {
        Box::pin(async move { bail!("reading {key} is not supported by this store") })
    }
    fn put_object(&self, key: String, bytes: Vec<u8>) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let _ = bytes;
            bail!("writing {key} is not supported by this store")
        })
    }
}

/// A blob store for code paths that only read a store's journal and never
/// touch an object; every operation fails.
pub fn unavailable() -> Arc<dyn BlobStore> {
    Arc::new(UnavailableBlobStore)
}

struct UnavailableBlobStore;

impl BlobStore for UnavailableBlobStore {
    fn put_temp(&self, _bytes: Vec<u8>) -> BoxFuture<'_, Result<String>> {
        Box::pin(async { bail!("blob store unavailable") })
    }
    fn make_permanent(&self, _key: String, _cid: Cid) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { bail!("blob store unavailable") })
    }
    fn put_permanent(&self, _cid: Cid, _bytes: Vec<u8>) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { bail!("blob store unavailable") })
    }
    fn quarantine(&self, _cid: Cid) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { bail!("blob store unavailable") })
    }
    fn unquarantine(&self, _cid: Cid) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { bail!("blob store unavailable") })
    }
    fn get_bytes(&self, _cid: Cid) -> BoxFuture<'_, Result<Vec<u8>>> {
        Box::pin(async { bail!("blob store unavailable") })
    }
    fn get_stream(&self, _cid: Cid) -> BoxFuture<'_, Result<ByteStream>> {
        Box::pin(async { bail!("blob store unavailable") })
    }
    fn has_temp(&self, _key: String) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async { bail!("blob store unavailable") })
    }
    fn has_stored(&self, _cid: Cid) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async { bail!("blob store unavailable") })
    }
    fn delete(&self, _cid: Cid) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { bail!("blob store unavailable") })
    }
    fn delete_many(&self, _cids: Vec<Cid>) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { bail!("blob store unavailable") })
    }
    fn make_permanent_copy_only(&self, _key: String, _cid: Cid) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { bail!("blob store unavailable") })
    }
    fn has_quarantined(&self, _cid: Cid) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async { bail!("blob store unavailable") })
    }
    fn restore_copy_only(&self, _cid: Cid) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { bail!("blob store unavailable") })
    }
}

/// Builds the configured blobstore implementation for a given actor.
pub struct BlobstoreFactory {
    cfg: BlobstoreConfig,
    aws_cfg: SdkConfig,
    attempts: Option<crate::blob_attempts::AttemptJournal>,
    generations: Option<crate::blob_generations::Generations>,
}

impl BlobstoreFactory {
    pub fn new(cfg: BlobstoreConfig, aws_cfg: SdkConfig) -> Self {
        BlobstoreFactory {
            cfg,
            aws_cfg,
            attempts: None,
            generations: None,
        }
    }

    /// Builds the factory for the configured store, deriving the S3 client
    /// settings from the `PDS_BLOBSTORE_S3_*` configuration over whatever
    /// the AWS environment provides.
    pub async fn from_config(cfg: BlobstoreConfig) -> Self {
        let s3 = match &cfg {
            BlobstoreConfig::S3(s3) => s3.clone(),
            BlobstoreConfig::Disk { .. } => S3Config::default(),
        };
        Self::new(cfg, sdk_config_for(&s3).await)
    }
    /// Journals every physical write the S3 stores make.
    pub fn with_attempts(mut self, attempts: crate::blob_attempts::AttemptJournal) -> Self {
        self.attempts = Some(attempts);
        self
    }

    /// Resolves permanent objects through the generation registry, so a
    /// retired key is never read or written again.
    pub fn with_generations(mut self, generations: crate::blob_generations::Generations) -> Self {
        self.generations = Some(generations);
        self
    }
    pub fn generations(&self) -> Option<&crate::blob_generations::Generations> {
        self.generations.as_ref()
    }

    pub fn clone_config(&self) -> (BlobstoreConfig, SdkConfig) {
        (self.cfg.clone(), self.aws_cfg.clone())
    }

    /// A factory over the same store and journal.
    pub fn from_attempts(
        (cfg, aws_cfg): (BlobstoreConfig, SdkConfig),
        attempts: Option<crate::blob_attempts::AttemptJournal>,
    ) -> Self {
        BlobstoreFactory {
            cfg,
            aws_cfg,
            attempts,
            generations: None,
        }
    }

    pub fn attempts(&self) -> Option<&crate::blob_attempts::AttemptJournal> {
        self.attempts.as_ref()
    }

    /// Wraps a store so permanent objects follow the registry.
    pub fn generation_aware(&self, did: String, inner: Arc<dyn BlobStore>) -> Arc<dyn BlobStore> {
        match &self.generations {
            Some(generations) => Arc::new(GenerationAware {
                did,
                inner,
                generations: generations.clone(),
                attempts: self.attempts.clone(),
            }),
            None => inner,
        }
    }
    pub fn blobstore(&self, did: String) -> Arc<dyn BlobStore> {
        let inner: Arc<dyn BlobStore> = match &self.cfg {
            BlobstoreConfig::Disk {
                location,
                tmp_location,
            } => Arc::new(DiskBlobStore::new(
                did.clone(),
                Path::new(location),
                tmp_location.as_deref().map(Path::new),
                None,
            )),
            BlobstoreConfig::S3(s3) => {
                let store = S3BlobStore::new(
                    did.clone(),
                    &self.aws_cfg,
                    s3.bucket.clone(),
                    s3.force_path_style,
                );
                Arc::new(match &self.attempts {
                    Some(attempts) => store.with_attempts(attempts.clone()),
                    None => store,
                })
            }
        };
        self.generation_aware(did, inner)
    }
}

/// The SDK configuration for an S3 store: the environment's defaults with
/// the reference PDS settings layered on top.
pub async fn sdk_config_for(s3: &S3Config) -> SdkConfig {
    let mut loader = aws_config::from_env();
    if let Some(region) = &s3.region {
        loader = loader.region(aws_sdk_s3::config::Region::new(region.clone()));
    }
    if let Some(endpoint) = &s3.endpoint {
        loader = loader.endpoint_url(endpoint.clone());
    }
    if let (Some(key), Some(secret)) = (&s3.access_key_id, &s3.secret_access_key) {
        loader = loader.credentials_provider(aws_sdk_s3::config::Credentials::new(
            key.clone(),
            secret.clone(),
            None,
            None,
            "pds-blobstore",
        ));
    }
    loader.load().await
}
/// A store whose permanent objects live at the generation the registry
/// names: generation zero is the inner store's own layout, later ones are
/// generation keys the inner store reads and writes by physical key. A
/// content whose current key has an unconfirmed delete is unavailable.
pub struct GenerationAware {
    did: String,
    inner: Arc<dyn BlobStore>,
    generations: crate::blob_generations::Generations,
    attempts: Option<crate::blob_attempts::AttemptJournal>,
}

impl GenerationAware {
    async fn generation(&self, cid: Cid) -> Result<u32> {
        self.generations.current(&self.did, &cid.to_string()).await
    }

    async fn ensure_available(&self, key: &str) -> Result<()> {
        if let Some(attempts) = &self.attempts {
            if let Some(latest) = attempts.latest(&self.did, key).await? {
                if latest.operation == "delete" && latest.is_unresolved() {
                    return Err(BlobUnavailableError.into());
                }
            }
        }
        Ok(())
    }

    fn key(&self, cid: Cid, generation: u32) -> String {
        self.inner
            .object_key(ObjectKind::Permanent, &cid.to_string(), generation)
    }
}

impl BlobStore for GenerationAware {
    fn put_temp(&self, bytes: Vec<u8>) -> BoxFuture<'_, Result<String>> {
        self.inner.put_temp(bytes)
    }
    fn put_temp_from_path(&self, path: std::path::PathBuf) -> BoxFuture<'_, Result<String>> {
        self.inner.put_temp_from_path(path)
    }
    fn make_permanent(&self, key: String, cid: Cid) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let generation = self.generation(cid).await?;
            if generation == 0 {
                return self.inner.make_permanent(key, cid).await;
            }
            let target = self.key(cid, generation);
            if !self.inner.object_exists(target.clone()).await? {
                let bytes = self
                    .inner
                    .get_object(self.inner.object_key(ObjectKind::Temp, &key, 0))
                    .await?;
                self.inner.put_object(target, bytes).await?;
            }
            self.inner
                .delete_object(self.inner.object_key(ObjectKind::Temp, &key, 0))
                .await
                .map_err(anyhow::Error::new)
        })
    }
    fn put_permanent(&self, cid: Cid, bytes: Vec<u8>) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let generation = self.generation(cid).await?;
            if generation == 0 {
                return self.inner.put_permanent(cid, bytes).await;
            }
            self.inner
                .put_object(self.key(cid, generation), bytes)
                .await
        })
    }
    fn quarantine(&self, cid: Cid) -> BoxFuture<'_, Result<()>> {
        self.inner.quarantine(cid)
    }
    fn unquarantine(&self, cid: Cid) -> BoxFuture<'_, Result<()>> {
        self.inner.unquarantine(cid)
    }
    fn get_bytes(&self, cid: Cid) -> BoxFuture<'_, Result<Vec<u8>>> {
        Box::pin(async move {
            let generation = self.generation(cid).await?;
            let key = self.key(cid, generation);
            self.ensure_available(&key).await?;
            if generation == 0 {
                return self.inner.get_bytes(cid).await;
            }
            self.inner.get_object(key).await
        })
    }
    fn get_stream(&self, cid: Cid) -> BoxFuture<'_, Result<ByteStream>> {
        Box::pin(async move {
            let generation = self.generation(cid).await?;
            let key = self.key(cid, generation);
            self.ensure_available(&key).await?;
            if generation == 0 {
                return self.inner.get_stream(cid).await;
            }
            Ok(ByteStream::from(self.inner.get_object(key).await?))
        })
    }
    fn has_temp(&self, key: String) -> BoxFuture<'_, Result<bool>> {
        self.inner.has_temp(key)
    }
    fn has_stored(&self, cid: Cid) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async move {
            let generation = self.generation(cid).await?;
            if generation == 0 {
                return self.inner.has_stored(cid).await;
            }
            self.inner.object_exists(self.key(cid, generation)).await
        })
    }
    fn delete(&self, cid: Cid) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let generation = self.generation(cid).await?;
            if generation == 0 {
                return self.inner.delete(cid).await;
            }
            self.inner
                .delete_object(self.key(cid, generation))
                .await
                .map_err(anyhow::Error::new)
        })
    }
    fn delete_many(&self, cids: Vec<Cid>) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            for cid in cids {
                BlobStore::delete(self, cid).await?;
            }
            Ok(())
        })
    }
    fn delete_all(&self) -> Option<BoxFuture<'_, Result<()>>> {
        self.inner.delete_all()
    }
    fn make_permanent_copy_only(&self, key: String, cid: Cid) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let generation = self.generation(cid).await?;
            if generation == 0 {
                return self.inner.make_permanent_copy_only(key, cid).await;
            }
            let target = self.key(cid, generation);
            if !self.inner.object_exists(target.clone()).await? {
                let bytes = self
                    .inner
                    .get_object(self.inner.object_key(ObjectKind::Temp, &key, 0))
                    .await?;
                self.inner.put_object(target, bytes).await?;
            }
            Ok(())
        })
    }
    fn has_quarantined(&self, cid: Cid) -> BoxFuture<'_, Result<bool>> {
        self.inner.has_quarantined(cid)
    }
    fn restore_copy_only(&self, cid: Cid) -> BoxFuture<'_, Result<()>> {
        self.inner.restore_copy_only(cid)
    }
    fn namespace_prefixes(&self) -> Vec<String> {
        self.inner.namespace_prefixes()
    }
    fn object_key(&self, kind: ObjectKind, name: &str, generation: u32) -> String {
        self.inner.object_key(kind, name, generation)
    }
    fn list_objects(&self, prefix: String) -> BoxFuture<'_, Result<Vec<String>>> {
        self.inner.list_objects(prefix)
    }
    fn delete_object(&self, key: String) -> BoxFuture<'_, std::result::Result<(), DeleteError>> {
        self.inner.delete_object(key)
    }
    fn object_exists(&self, key: String) -> BoxFuture<'_, Result<bool>> {
        self.inner.object_exists(key)
    }
    fn get_object(&self, key: String) -> BoxFuture<'_, Result<Vec<u8>>> {
        self.inner.get_object(key)
    }
    fn put_object(&self, key: String, bytes: Vec<u8>) -> BoxFuture<'_, Result<()>> {
        self.inner.put_object(key, bytes)
    }
}

/// In-memory blobstore used by deterministic tests. It counts every call
/// that deletes or moves an object, so a test can prove that a code path
/// never issues one.
#[derive(Debug, Default)]
pub struct MemoryBlobStore {
    state: Mutex<MemoryBlobStoreState>,
    next_key: AtomicU64,
    destructive_calls: AtomicU64,
}

#[derive(Debug, Default)]
struct MemoryBlobStoreState {
    temp: HashMap<String, Vec<u8>>,
    stored: HashMap<String, Vec<u8>>,
    quarantined: HashMap<String, Vec<u8>>,
    /// Physical keys whose next delete answers with the given error
    /// (an unconfirmed delete leaves the object in place).
    failing_deletes: HashMap<String, DeleteError>,
    /// Objects reachable only by physical key: generation keys and objects
    /// a legacy writer left behind.
    named: HashMap<String, Vec<u8>>,
}

impl MemoryBlobStore {
    fn lock(&self) -> std::sync::MutexGuard<'_, MemoryBlobStoreState> {
        self.state.lock().expect("memory blobstore mutex poisoned")
    }

    pub fn stored_cids(&self) -> Vec<String> {
        let mut cids: Vec<String> = self.lock().stored.keys().cloned().collect();
        cids.sort();
        cids
    }

    pub fn has_temp(&self, key: &str) -> bool {
        self.lock().temp.contains_key(key)
    }

    pub fn has_quarantined(&self, cid: &Cid) -> bool {
        self.lock().quarantined.contains_key(&cid.to_string())
    }

    /// Places an object in quarantine with no permanent copy, as a takedown
    /// by the reference implementation leaves it.
    pub fn put_quarantined(&self, cid: Cid, bytes: Vec<u8>) {
        self.lock().quarantined.insert(cid.to_string(), bytes);
    }
    /// How many calls deleted or moved an object.
    pub fn destructive_calls(&self) -> u64 {
        self.destructive_calls.load(Ordering::SeqCst)
    }

    /// Makes the next delete of `key` answer with `error`; an ambiguous
    /// answer leaves the object in place, as a lost response would.
    pub fn fail_next_delete(&self, key: &str, error: DeleteError) {
        self.lock().failing_deletes.insert(key.to_owned(), error);
    }

    /// Places an object at a physical key, as a writer outside this
    /// process's journal would.
    pub fn put_named(&self, key: &str, bytes: Vec<u8>) {
        self.lock().named.insert(key.to_owned(), bytes);
    }

    pub fn physical_keys(&self) -> Vec<String> {
        let state = self.lock();
        let mut keys: Vec<String> = state
            .stored
            .keys()
            .map(|cid| format!("permanent/{cid}"))
            .chain(state.temp.keys().map(|key| format!("temp/{key}")))
            .chain(
                state
                    .quarantined
                    .keys()
                    .map(|cid| format!("quarantine/{cid}")),
            )
            .chain(state.named.keys().cloned())
            .collect();
        keys.sort();
        keys
    }

    fn remove_physical(state: &mut MemoryBlobStoreState, key: &str) -> bool {
        if let Some(name) = key.strip_prefix("permanent/") {
            if state.stored.remove(name).is_some() {
                return true;
            }
        } else if let Some(name) = key.strip_prefix("temp/") {
            if state.temp.remove(name).is_some() {
                return true;
            }
        } else if let Some(name) = key.strip_prefix("quarantine/") {
            if state.quarantined.remove(name).is_some() {
                return true;
            }
        }
        state.named.remove(key).is_some()
    }

    fn read_physical(state: &MemoryBlobStoreState, key: &str) -> Option<Vec<u8>> {
        if let Some(name) = key.strip_prefix("permanent/") {
            if let Some(bytes) = state.stored.get(name) {
                return Some(bytes.clone());
            }
        } else if let Some(name) = key.strip_prefix("temp/") {
            if let Some(bytes) = state.temp.get(name) {
                return Some(bytes.clone());
            }
        } else if let Some(name) = key.strip_prefix("quarantine/") {
            if let Some(bytes) = state.quarantined.get(name) {
                return Some(bytes.clone());
            }
        }
        state.named.get(key).cloned()
    }

    fn destructive(&self) {
        self.destructive_calls.fetch_add(1, Ordering::SeqCst);
    }
}

impl BlobStore for MemoryBlobStore {
    fn namespace_prefixes(&self) -> Vec<String> {
        vec!["permanent/".into(), "temp/".into(), "quarantine/".into()]
    }
    fn list_objects(&self, prefix: String) -> BoxFuture<'_, Result<Vec<String>>> {
        Box::pin(async move {
            Ok(self
                .physical_keys()
                .into_iter()
                .filter(|key| key.starts_with(&prefix))
                .collect())
        })
    }
    fn delete_object(&self, key: String) -> BoxFuture<'_, std::result::Result<(), DeleteError>> {
        Box::pin(async move {
            self.destructive();
            let mut state = self.lock();
            if let Some(error) = state.failing_deletes.remove(&key) {
                return Err(error);
            }
            MemoryBlobStore::remove_physical(&mut state, &key);
            Ok(())
        })
    }
    fn object_exists(&self, key: String) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async move { Ok(MemoryBlobStore::read_physical(&self.lock(), &key).is_some()) })
    }
    fn get_object(&self, key: String) -> BoxFuture<'_, Result<Vec<u8>>> {
        Box::pin(async move {
            match MemoryBlobStore::read_physical(&self.lock(), &key) {
                Some(bytes) => Ok(bytes),
                None => Err(BlobNotFoundError.into()),
            }
        })
    }
    fn put_object(&self, key: String, bytes: Vec<u8>) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.lock().named.insert(key, bytes);
            Ok(())
        })
    }
    fn put_temp(&self, bytes: Vec<u8>) -> BoxFuture<'_, Result<String>> {
        Box::pin(async move {
            let key = format!("temp-{}", self.next_key.fetch_add(1, Ordering::SeqCst));
            self.lock().temp.insert(key.clone(), bytes);
            Ok(key)
        })
    }

    fn make_permanent(&self, key: String, cid: Cid) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.destructive();
            let mut state = self.lock();
            // like the disk and S3 stores, an already-stored object only
            // needs its temporary copy removed
            if state.stored.contains_key(&cid.to_string()) {
                state.temp.remove(&key);
                return Ok(());
            }
            let Some(bytes) = state.temp.remove(&key) else {
                bail!("temp blob not found: {key}")
            };
            state.stored.insert(cid.to_string(), bytes);
            Ok(())
        })
    }

    fn make_permanent_copy_only(&self, key: String, cid: Cid) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let mut state = self.lock();
            let Some(bytes) = state.temp.get(&key).cloned() else {
                bail!("temp blob not found: {key}")
            };
            state.stored.entry(cid.to_string()).or_insert(bytes);
            Ok(())
        })
    }

    fn has_quarantined(&self, cid: Cid) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async move { Ok(MemoryBlobStore::has_quarantined(self, &cid)) })
    }

    fn restore_copy_only(&self, cid: Cid) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let mut state = self.lock();
            let Some(bytes) = state.quarantined.get(&cid.to_string()).cloned() else {
                bail!("quarantined blob not found: {cid}")
            };
            state.stored.entry(cid.to_string()).or_insert(bytes);
            Ok(())
        })
    }

    fn put_permanent(&self, cid: Cid, bytes: Vec<u8>) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.lock().stored.insert(cid.to_string(), bytes);
            Ok(())
        })
    }

    fn quarantine(&self, cid: Cid) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.destructive();
            let mut state = self.lock();
            let Some(bytes) = state.stored.remove(&cid.to_string()) else {
                bail!("stored blob not found: {cid}")
            };
            state.quarantined.insert(cid.to_string(), bytes);
            Ok(())
        })
    }

    fn unquarantine(&self, cid: Cid) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.destructive();
            let mut state = self.lock();
            let Some(bytes) = state.quarantined.remove(&cid.to_string()) else {
                bail!("quarantined blob not found: {cid}")
            };
            state.stored.insert(cid.to_string(), bytes);
            Ok(())
        })
    }

    fn get_bytes(&self, cid: Cid) -> BoxFuture<'_, Result<Vec<u8>>> {
        Box::pin(async move {
            match self.lock().stored.get(&cid.to_string()) {
                Some(bytes) => Ok(bytes.clone()),
                None => bail!("stored blob not found: {cid}"),
            }
        })
    }

    fn get_stream(&self, cid: Cid) -> BoxFuture<'_, Result<ByteStream>> {
        Box::pin(async move {
            let bytes = BlobStore::get_bytes(self, cid).await?;
            Ok(ByteStream::from(bytes))
        })
    }

    fn has_temp(&self, key: String) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async move { Ok(self.lock().temp.contains_key(&key)) })
    }

    fn has_stored(&self, cid: Cid) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async move { Ok(self.lock().stored.contains_key(&cid.to_string())) })
    }

    fn delete(&self, cid: Cid) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.destructive();
            self.lock().stored.remove(&cid.to_string());
            Ok(())
        })
    }

    fn delete_many(&self, cids: Vec<Cid>) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.destructive();
            let mut state = self.lock();
            for cid in cids {
                state.stored.remove(&cid.to_string());
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsky_common::ipld::sha256_to_cid;
    use sha2::{Digest, Sha256};

    fn cid_for(bytes: &[u8]) -> Cid {
        sha256_to_cid(Sha256::digest(bytes).to_vec())
    }

    #[tokio::test]
    async fn a_spooled_file_becomes_a_temp_object() {
        let store = MemoryBlobStore::default();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spooled");
        std::fs::write(&path, b"from disk").unwrap();
        let key = BlobStore::put_temp_from_path(&store, path).await.unwrap();
        assert!(store.has_temp(&key));
        assert!(
            BlobStore::put_temp_from_path(&store, dir.path().join("missing"))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn temp_to_permanent_lifecycle() {
        let store = MemoryBlobStore::default();
        let bytes = b"hello blob".to_vec();
        let cid = cid_for(&bytes);
        let key = store.put_temp(bytes.clone()).await.unwrap();
        assert!(store.has_temp(&key));
        assert!(BlobStore::has_temp(&store, key.clone()).await.unwrap());
        assert!(!store.has_stored(cid).await.unwrap());
        assert!(store.delete_all().is_none());

        store.make_permanent(key.clone(), cid).await.unwrap();
        assert!(!store.has_temp(&key));
        assert!(store.has_stored(cid).await.unwrap());
        assert_eq!(BlobStore::get_bytes(&store, cid).await.unwrap(), bytes);
        let streamed = BlobStore::get_stream(&store, cid)
            .await
            .unwrap()
            .collect()
            .await
            .unwrap()
            .to_vec();
        assert_eq!(streamed, bytes);
        // promoting again finds the object stored and nothing left to move
        store.make_permanent(key, cid).await.unwrap();
        let other = cid_for(b"never uploaded");
        assert!(store
            .make_permanent("missing".to_owned(), other)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn copy_only_operations_leave_the_source_and_count_nothing() {
        let store = MemoryBlobStore::default();
        let bytes = b"copy me".to_vec();
        let cid = cid_for(&bytes);
        assert!(store
            .make_permanent_copy_only("nope".to_owned(), cid)
            .await
            .is_err());
        let key = store.put_temp(bytes.clone()).await.unwrap();
        store
            .make_permanent_copy_only(key.clone(), cid)
            .await
            .unwrap();
        assert!(store.has_temp(&key));
        assert!(store.has_stored(cid).await.unwrap());
        // copying again is a no-op
        store.make_permanent_copy_only(key, cid).await.unwrap();

        assert!(store.restore_copy_only(cid).await.is_err());
        store.delete(cid).await.unwrap();
        store.put_quarantined(cid, bytes.clone());
        assert!(BlobStore::has_quarantined(&store, cid).await.unwrap());
        store.restore_copy_only(cid).await.unwrap();
        assert!(store.has_stored(cid).await.unwrap());
        assert!(store.has_quarantined(&cid));
        assert_eq!(store.destructive_calls(), 1);
        assert!(store.unavailable_never_counts().await);
    }

    #[tokio::test]
    async fn quarantine_round_trip() {
        let store = MemoryBlobStore::default();
        let bytes = b"quarantine me".to_vec();
        let cid = cid_for(&bytes);
        assert!(store.quarantine(cid).await.is_err());
        store.put_permanent(cid, bytes).await.unwrap();
        store.quarantine(cid).await.unwrap();
        assert!(store.has_quarantined(&cid));
        assert!(!store.has_stored(cid).await.unwrap());
        assert!(BlobStore::get_bytes(&store, cid).await.is_err());
        assert!(BlobStore::get_stream(&store, cid).await.is_err());
        store.unquarantine(cid).await.unwrap();
        assert!(store.has_stored(cid).await.unwrap());
        assert!(store.unquarantine(cid).await.is_err());
    }

    #[tokio::test]
    async fn deletes_single_and_many() {
        let store = MemoryBlobStore::default();
        let one = b"one".to_vec();
        let two = b"two".to_vec();
        let (cid_one, cid_two) = (cid_for(&one), cid_for(&two));
        store.put_permanent(cid_one, one).await.unwrap();
        store.put_permanent(cid_two, two).await.unwrap();
        assert_eq!(store.stored_cids().len(), 2);
        store.delete(cid_one).await.unwrap();
        assert_eq!(store.stored_cids(), [cid_two.to_string()]);
        store.delete_many(vec![cid_one, cid_two]).await.unwrap();
        assert!(store.stored_cids().is_empty());
    }

    impl MemoryBlobStore {
        async fn unavailable_never_counts(&self) -> bool {
            let store = unavailable();
            let cid = cid_for(b"nothing");
            store
                .make_permanent_copy_only("k".to_owned(), cid)
                .await
                .is_err()
                && store.has_quarantined(cid).await.is_err()
                && store.restore_copy_only(cid).await.is_err()
        }
    }

    #[tokio::test]
    async fn unavailable_store_fails_every_call() {
        let store = unavailable();
        let cid = cid_for(b"nothing");
        assert!(store.put_temp(vec![]).await.is_err());
        assert!(store.make_permanent("k".to_owned(), cid).await.is_err());
        assert!(store.put_permanent(cid, vec![]).await.is_err());
        assert!(store.quarantine(cid).await.is_err());
        assert!(store.unquarantine(cid).await.is_err());
        assert!(store.get_bytes(cid).await.is_err());
        assert!(store.get_stream(cid).await.is_err());
        assert!(store.has_temp("k".to_owned()).await.is_err());
        assert!(store.has_stored(cid).await.is_err());
        assert!(store.delete(cid).await.is_err());
        assert!(store.delete_many(vec![cid]).await.is_err());
        assert!(store.delete_all().is_none());
        // the physical-key operations a store does not implement are refused
        assert!(store.namespace_prefixes().is_empty());
        assert_eq!(
            store.object_key(ObjectKind::Permanent, "c", 0),
            "permanent/c"
        );
        assert_eq!(store.object_key(ObjectKind::Temp, "t", 1), "temp/t.g1");
        assert_eq!(
            store.object_key(ObjectKind::Quarantine, "q", 2),
            "quarantine/q.g2"
        );
        assert!(store.list_objects("permanent/".to_owned()).await.is_err());
        assert!(matches!(
            store.delete_object("k".to_owned()).await,
            Err(DeleteError::Definitive(_))
        ));
        assert!(store.object_exists("k".to_owned()).await.is_err());
        assert!(store.get_object("k".to_owned()).await.is_err());
        assert!(store.put_object("k".to_owned(), vec![]).await.is_err());
    }

    #[tokio::test]
    async fn physical_keys_reach_every_namespace_of_the_memory_store() {
        let store = MemoryBlobStore::default();
        let bytes = b"physical".to_vec();
        let cid = cid_for(&bytes);
        let temp = store.put_temp(bytes.clone()).await.unwrap();
        store.put_quarantined(cid, bytes.clone());
        store.put_named("elsewhere/object", bytes.clone());
        let keys = store.physical_keys();
        assert_eq!(
            keys,
            vec![
                "elsewhere/object".to_owned(),
                format!("quarantine/{cid}"),
                format!("temp/{temp}")
            ]
        );
        for key in &keys {
            assert_eq!(
                BlobStore::get_object(&store, key.clone()).await.unwrap(),
                bytes
            );
            store.delete_object(key.clone()).await.unwrap();
            assert!(!store.object_exists(key.clone()).await.unwrap());
        }
        assert!(BlobStore::get_object(&store, "temp/missing".to_owned())
            .await
            .is_err());
        assert!(store.physical_keys().is_empty());
        // deleting an unknown key is not an error
        store.delete_object("nothing".to_owned()).await.unwrap();
    }

    fn disk_config(dir: &Path) -> BlobstoreConfig {
        BlobstoreConfig::Disk {
            location: dir.join("blobs").to_string_lossy().to_string(),
            tmp_location: None,
        }
    }

    #[tokio::test]
    async fn a_factory_is_rebuilt_over_the_same_store_and_journal() {
        let dir = tempfile::tempdir().unwrap();
        let journal =
            crate::blob_attempts::AttemptJournal::open(dir.path().join("attempts.sqlite"), false)
                .await
                .unwrap();
        let registry = crate::blob_generations::Generations::open(dir.path().join("gen.sqlite"))
            .await
            .unwrap();
        let factory = BlobstoreFactory::new(disk_config(dir.path()), SdkConfig::builder().build())
            .with_attempts(journal);
        assert!(factory.generations().is_none());
        let rebuilt =
            BlobstoreFactory::from_attempts(factory.clone_config(), factory.attempts().cloned())
                .with_generations(registry);
        assert!(rebuilt.attempts().is_some() && rebuilt.generations().is_some());
        let bare = BlobstoreFactory::from_attempts(factory.clone_config(), None);
        assert!(bare.attempts().is_none() && bare.generations().is_none());
        // without a registry the wrapper is the store itself
        let inner: Arc<dyn BlobStore> = Arc::new(MemoryBlobStore::default());
        let same = bare.generation_aware("did:plc:x".to_owned(), inner.clone());
        assert_eq!(
            Arc::as_ptr(&same) as *const (),
            Arc::as_ptr(&inner) as *const ()
        );
    }

    #[tokio::test]
    async fn a_generation_aware_store_follows_the_registry() {
        let dir = tempfile::tempdir().unwrap();
        let did = "did:plc:gen";
        let registry = crate::blob_generations::Generations::open(dir.path().join("gen.sqlite"))
            .await
            .unwrap();
        let journal =
            crate::blob_attempts::AttemptJournal::open(dir.path().join("attempts.sqlite"), false)
                .await
                .unwrap();
        let inner = Arc::new(MemoryBlobStore::default());
        let factory = BlobstoreFactory::new(disk_config(dir.path()), SdkConfig::builder().build())
            .with_attempts(journal.clone())
            .with_generations(registry.clone());
        let store = factory.generation_aware(did.to_owned(), inner.clone());
        let bytes = b"generation".to_vec();
        let cid = cid_for(&bytes);
        let streamed = |stream: ByteStream| async move { stream.collect().await.unwrap().to_vec() };

        // generation zero is the inner store's own layout
        let key = store.put_temp(bytes.clone()).await.unwrap();
        assert!(store.has_temp(key.clone()).await.unwrap());
        store.make_permanent(key, cid).await.unwrap();
        assert!(store.has_stored(cid).await.unwrap());
        assert_eq!(store.get_bytes(cid).await.unwrap(), bytes);
        assert_eq!(streamed(store.get_stream(cid).await.unwrap()).await, bytes);
        assert_eq!(inner.stored_cids(), vec![cid.to_string()]);
        assert_eq!(
            store.get_object(format!("permanent/{cid}")).await.unwrap(),
            bytes
        );
        store.quarantine(cid).await.unwrap();
        assert!(store.has_quarantined(cid).await.unwrap());
        store.restore_copy_only(cid).await.unwrap();
        assert!(store.has_stored(cid).await.unwrap());
        store.unquarantine(cid).await.unwrap();
        store.delete(cid).await.unwrap();
        assert!(!store.has_stored(cid).await.unwrap());
        store.put_permanent(cid, bytes.clone()).await.unwrap();
        store.delete_many(vec![cid]).await.unwrap();
        assert!(inner.stored_cids().is_empty());
        let temp = store.put_temp(bytes.clone()).await.unwrap();
        store
            .make_permanent_copy_only(temp.clone(), cid)
            .await
            .unwrap();
        assert!(store.has_stored(cid).await.unwrap());
        assert!(inner.has_temp(&temp));
        assert_eq!(store.namespace_prefixes(), inner.namespace_prefixes());
        assert!(store.delete_all().is_none());
        store
            .put_object("named/x".to_owned(), b"x".to_vec())
            .await
            .unwrap();
        assert_eq!(
            store.list_objects("named/".to_owned()).await.unwrap(),
            vec!["named/x".to_owned()]
        );
        assert_eq!(store.get_object("named/x".to_owned()).await.unwrap(), b"x");
        store.delete_object("named/x".to_owned()).await.unwrap();
        assert!(!store.object_exists("named/x".to_owned()).await.unwrap());
        let path = dir.path().join("spooled");
        std::fs::write(&path, b"spooled").unwrap();
        let spooled = store.put_temp_from_path(path).await.unwrap();
        assert!(store.has_temp(spooled).await.unwrap());

        // once the base key is retired, the content lives at generation one
        let base = store.object_key(ObjectKind::Permanent, &cid.to_string(), 0);
        assert_eq!(
            registry.retire(did, &cid.to_string(), &base).await.unwrap(),
            1
        );
        let g1 = store.object_key(ObjectKind::Permanent, &cid.to_string(), 1);
        assert_eq!(g1, format!("permanent/{cid}.g1"));
        assert!(!store.has_stored(cid).await.unwrap());
        store.make_permanent(temp.clone(), cid).await.unwrap();
        assert!(store.has_stored(cid).await.unwrap());
        assert!(!inner.has_temp(&temp));
        assert_eq!(store.get_bytes(cid).await.unwrap(), bytes);
        assert_eq!(streamed(store.get_stream(cid).await.unwrap()).await, bytes);
        assert_eq!(store.get_object(g1.clone()).await.unwrap(), bytes);
        // promoting again finds the generation object in place
        let again = store.put_temp(bytes.clone()).await.unwrap();
        store.make_permanent(again.clone(), cid).await.unwrap();
        assert!(!inner.has_temp(&again));
        let kept = store.put_temp(bytes.clone()).await.unwrap();
        store
            .make_permanent_copy_only(kept.clone(), cid)
            .await
            .unwrap();
        assert!(inner.has_temp(&kept));
        // a copy-only promotion writes the generation object when it is absent
        store.delete(cid).await.unwrap();
        assert!(!store.object_exists(g1.clone()).await.unwrap());
        store.make_permanent_copy_only(kept, cid).await.unwrap();
        assert!(store.has_stored(cid).await.unwrap());
        store.delete_many(vec![cid]).await.unwrap();
        assert!(!store.has_stored(cid).await.unwrap());
        store.put_permanent(cid, bytes.clone()).await.unwrap();
        assert_eq!(store.get_bytes(cid).await.unwrap(), bytes);

        // an unconfirmed delete of the current key makes the content unavailable
        let attempt = journal.begin(did, &g1, "delete").await.unwrap();
        let err = store.get_bytes(cid).await.unwrap_err();
        assert!(
            err.downcast_ref::<BlobUnavailableError>().is_some(),
            "{err}"
        );
        assert!(store.get_stream(cid).await.is_err());
        journal
            .resolve(
                attempt,
                crate::blob_attempts::AttemptOutcome::Ambiguous("lost".to_owned()),
            )
            .await
            .unwrap();
        assert!(store.get_bytes(cid).await.is_err());
        let retry = journal.begin(did, &g1, "delete").await.unwrap();
        journal
            .resolve(
                retry,
                crate::blob_attempts::AttemptOutcome::Failed("refused".to_owned()),
            )
            .await
            .unwrap();
        assert_eq!(store.get_bytes(cid).await.unwrap(), bytes);
        // without an attempt journal nothing is ever held back
        let unjournaled = BlobstoreFactory::from_attempts(factory.clone_config(), None)
            .with_generations(registry)
            .generation_aware(did.to_owned(), inner.clone());
        journal.begin(did, &g1, "delete").await.unwrap();
        assert!(store.get_bytes(cid).await.is_err());
        assert_eq!(unjournaled.get_bytes(cid).await.unwrap(), bytes);
    }

    #[tokio::test]
    async fn factory_builds_disk_store_from_disk_config() {
        let dir = tempfile::tempdir().unwrap();
        let location = dir.path().join("blobs");
        let factory = BlobstoreFactory::new(
            BlobstoreConfig::Disk {
                location: location.to_string_lossy().to_string(),
                tmp_location: None,
            },
            SdkConfig::builder().build(),
        );
        let store = factory.blobstore("did:example:alice".to_owned());
        let bytes = b"factory blob".to_vec();
        let cid = cid_for(&bytes);
        store.put_permanent(cid, bytes.clone()).await.unwrap();
        // bytes landed on disk under {location}/{did}/{cid}
        let stored_path = location.join("did:example:alice").join(cid.to_string());
        assert_eq!(std::fs::read(stored_path).unwrap(), bytes);
        assert!(store.delete_all().is_some());
    }

    #[tokio::test]
    async fn factory_builds_disk_store_with_custom_tmp_location() {
        let dir = tempfile::tempdir().unwrap();
        let location = dir.path().join("blobs");
        let tmp_location = dir.path().join("tmp");
        let factory = BlobstoreFactory::new(
            BlobstoreConfig::Disk {
                location: location.to_string_lossy().to_string(),
                tmp_location: Some(tmp_location.to_string_lossy().to_string()),
            },
            SdkConfig::builder().build(),
        );
        let store = factory.blobstore("did:example:alice".to_owned());
        let key = store.put_temp(b"temp blob".to_vec()).await.unwrap();
        assert!(tmp_location.join("did:example:alice").join(&key).is_file());
    }

    #[tokio::test]
    async fn factory_derives_the_sdk_config_from_the_s3_settings() {
        let s3 = S3Config {
            bucket: Some("bucket".to_owned()),
            region: Some("nyc3".to_owned()),
            endpoint: Some("https://nyc3.digitaloceanspaces.com".to_owned()),
            force_path_style: true,
            access_key_id: Some("key".to_owned()),
            secret_access_key: Some("secret".to_owned()),
        };
        let sdk = sdk_config_for(&s3).await;
        assert_eq!(sdk.region().map(|r| r.as_ref()), Some("nyc3"));
        assert_eq!(
            sdk.endpoint_url(),
            Some("https://nyc3.digitaloceanspaces.com")
        );
        assert!(sdk.credentials_provider().is_some());
        let factory = BlobstoreFactory::from_config(BlobstoreConfig::S3(s3)).await;
        assert!(factory
            .blobstore("did:example:alice".to_owned())
            .delete_all()
            .is_none());
        let disk_dir = tempfile::tempdir().unwrap();
        let disk = BlobstoreFactory::from_config(BlobstoreConfig::Disk {
            location: disk_dir.path().join("blobs").to_string_lossy().to_string(),
            tmp_location: None,
        })
        .await;
        assert!(disk
            .blobstore("did:example:alice".to_owned())
            .delete_all()
            .is_some());
    }

    #[tokio::test]
    async fn factory_builds_s3_store_from_s3_config() {
        let aws_cfg = SdkConfig::builder()
            .behavior_version(aws_sdk_s3::config::BehaviorVersion::latest())
            .build();
        let factory = BlobstoreFactory::new(
            BlobstoreConfig::S3(S3Config {
                bucket: Some("my-bucket".to_owned()),
                ..Default::default()
            }),
            aws_cfg.clone(),
        );
        // constructs without touching the network; s3 stores cannot delete_all
        let store = factory.blobstore("did:example:alice".to_owned());
        assert!(store.delete_all().is_none());

        let legacy =
            BlobstoreFactory::new(BlobstoreConfig::S3(S3Config::default()), aws_cfg.clone());
        let store = legacy.blobstore("did:example:alice".to_owned());
        assert!(store.delete_all().is_none());

        let dir = tempfile::tempdir().unwrap();
        let journal =
            crate::blob_attempts::AttemptJournal::open(dir.path().join("attempts.sqlite"), true)
                .await
                .unwrap();
        let journaled = BlobstoreFactory::new(BlobstoreConfig::S3(S3Config::default()), aws_cfg)
            .with_attempts(journal);
        assert!(journaled
            .blobstore("did:example:alice".to_owned())
            .delete_all()
            .is_none());
    }
}
