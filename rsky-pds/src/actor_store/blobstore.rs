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
}

impl BlobstoreFactory {
    pub fn new(cfg: BlobstoreConfig, aws_cfg: SdkConfig) -> Self {
        BlobstoreFactory {
            cfg,
            aws_cfg,
            attempts: None,
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

    pub fn blobstore(&self, did: String) -> Arc<dyn BlobStore> {
        match &self.cfg {
            BlobstoreConfig::Disk {
                location,
                tmp_location,
            } => Arc::new(DiskBlobStore::new(
                did,
                Path::new(location),
                tmp_location.as_deref().map(Path::new),
                None,
            )),
            BlobstoreConfig::S3(s3) => {
                let store =
                    S3BlobStore::new(did, &self.aws_cfg, s3.bucket.clone(), s3.force_path_style);
                Arc::new(match &self.attempts {
                    Some(attempts) => store.with_attempts(attempts.clone()),
                    None => store,
                })
            }
        }
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

    fn destructive(&self) {
        self.destructive_calls.fetch_add(1, Ordering::SeqCst);
    }
}

impl BlobStore for MemoryBlobStore {
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
