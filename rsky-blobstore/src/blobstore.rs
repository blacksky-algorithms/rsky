use anyhow::{bail, Result};
use bytes::Bytes;
use futures::future::BoxFuture;
use futures::stream::{BoxStream, StreamExt};
use lexicon_cid::Cid;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

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

/// The stream of object bytes returned by [`BlobStore::get_stream`].
pub type BoxedBlobStream = BoxStream<'static, Result<Bytes>>;

/// The physical name of an object at a generation: generation zero is the
/// object's own name, later generations are suffixed keys.
pub fn generation_name(name: &str, generation: u32) -> String {
    if generation == 0 {
        name.to_owned()
    } else {
        format!("{name}.g{generation}")
    }
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
    fn get_stream(&self, cid: Cid) -> BoxFuture<'_, Result<BoxedBlobStream>>;
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
        let name = generation_name(name, generation);
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
pub fn unavailable() -> std::sync::Arc<dyn BlobStore> {
    std::sync::Arc::new(UnavailableBlobStore)
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
    fn get_stream(&self, _cid: Cid) -> BoxFuture<'_, Result<BoxedBlobStream>> {
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

    fn get_stream(&self, cid: Cid) -> BoxFuture<'_, Result<BoxedBlobStream>> {
        Box::pin(async move {
            let bytes = BlobStore::get_bytes(self, cid).await?;
            Ok(futures::stream::once(async move { Ok(Bytes::from(bytes)) }).boxed())
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
    use futures::TryStreamExt;
    use sha2::{Digest, Sha256};

    const SHA2_256: u64 = 0x12;
    const RAWCODEC: u64 = 0x55;

    fn cid_for(bytes: &[u8]) -> Cid {
        let hash = Sha256::digest(bytes).to_vec();
        Cid::new_v1(
            RAWCODEC,
            lexicon_cid::multihash::Multihash::<64>::wrap(SHA2_256, &hash).unwrap(),
        )
    }

    async fn streamed_bytes(stream: BoxedBlobStream) -> Vec<u8> {
        let chunks: Vec<Bytes> = stream.try_collect().await.unwrap();
        chunks
            .iter()
            .flat_map(|chunk| chunk.iter().copied())
            .collect()
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
        assert_eq!(
            streamed_bytes(BlobStore::get_stream(&store, cid).await.unwrap()).await,
            bytes
        );
        // promoting again finds the object stored and nothing left to move
        store.make_permanent(key, cid).await.unwrap();
        let other = cid_for(b"never uploaded");
        assert!(store
            .make_permanent("missing".to_owned(), other)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn get_stream_yields_same_bytes_as_get_bytes() {
        let store = MemoryBlobStore::default();
        let bytes = b"stream parity".to_vec();
        let cid = cid_for(&bytes);
        store.put_permanent(cid, bytes.clone()).await.unwrap();

        let direct = BlobStore::get_bytes(&store, cid).await.unwrap();
        let streamed = streamed_bytes(BlobStore::get_stream(&store, cid).await.unwrap()).await;
        assert_eq!(direct, streamed);
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
}
