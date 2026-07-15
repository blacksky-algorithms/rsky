use crate::actor_store::aws::s3::S3BlobStore;
use crate::actor_store::disk_blobstore::DiskBlobStore;
use crate::config::BlobstoreConfig;
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
}

/// Builds the configured blobstore implementation for a given actor.
pub struct BlobstoreFactory {
    cfg: BlobstoreConfig,
    aws_cfg: SdkConfig,
}

impl BlobstoreFactory {
    pub fn new(cfg: BlobstoreConfig, aws_cfg: SdkConfig) -> Self {
        BlobstoreFactory { cfg, aws_cfg }
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
            BlobstoreConfig::S3 { bucket } => {
                Arc::new(S3BlobStore::new(did, &self.aws_cfg, bucket.clone()))
            }
        }
    }
}

/// In-memory blobstore used by deterministic tests.
#[derive(Debug, Default)]
pub struct MemoryBlobStore {
    state: Mutex<MemoryBlobStoreState>,
    next_key: AtomicU64,
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
            let mut state = self.lock();
            let Some(bytes) = state.temp.remove(&key) else {
                bail!("temp blob not found: {key}")
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
            self.lock().stored.remove(&cid.to_string());
            Ok(())
        })
    }

    fn delete_many(&self, cids: Vec<Cid>) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
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
        assert!(store.make_permanent(key, cid).await.is_err());
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
    async fn factory_builds_s3_store_from_s3_config() {
        let aws_cfg = SdkConfig::builder()
            .behavior_version(aws_sdk_s3::config::BehaviorVersion::latest())
            .build();
        let factory = BlobstoreFactory::new(
            BlobstoreConfig::S3 {
                bucket: Some("my-bucket".to_owned()),
            },
            aws_cfg.clone(),
        );
        // constructs without touching the network; s3 stores cannot delete_all
        let store = factory.blobstore("did:example:alice".to_owned());
        assert!(store.delete_all().is_none());

        let legacy = BlobstoreFactory::new(BlobstoreConfig::S3 { bucket: None }, aws_cfg);
        let store = legacy.blobstore("did:example:alice".to_owned());
        assert!(store.delete_all().is_none());
    }
}
