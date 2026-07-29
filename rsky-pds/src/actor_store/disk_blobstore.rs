// based on https://github.com/bluesky-social/atproto/blob/main/packages/pds/src/disk-blobstore.ts
use crate::actor_store::blobstore::{BlobNotFoundError, BlobStore};
use anyhow::{bail, Result};
use aws_sdk_s3::primitives::ByteStream;
use futures::future::BoxFuture;
use lexicon_cid::Cid;
use rand::RngCore;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

const DELETE_MANY_CHUNK_SIZE: usize = 500;

#[derive(Debug, Clone)]
pub struct DiskBlobStore {
    pub did: String,
    pub location: PathBuf,
    pub tmp_location: PathBuf,
    pub quarantine_location: PathBuf,
}

impl DiskBlobStore {
    pub fn new(
        did: String,
        location: &Path,
        tmp_location: Option<&Path>,
        quarantine_location: Option<&Path>,
    ) -> Self {
        DiskBlobStore {
            did,
            location: location.to_path_buf(),
            tmp_location: tmp_location
                .map(Path::to_path_buf)
                .unwrap_or_else(|| location.join("tempt")),
            quarantine_location: quarantine_location
                .map(Path::to_path_buf)
                .unwrap_or_else(|| location.join("quarantine")),
        }
    }

    fn gen_key() -> String {
        let mut bytes = [0u8; 20];
        rand::thread_rng().fill_bytes(&mut bytes);
        data_encoding::BASE32_NOPAD.encode(&bytes).to_lowercase()
    }

    pub fn tmp_path(&self, key: &str) -> PathBuf {
        self.tmp_location.join(&self.did).join(key)
    }

    pub fn stored_path(&self, cid: Cid) -> PathBuf {
        self.location.join(&self.did).join(cid.to_string())
    }

    pub fn quarantine_path(&self, cid: Cid) -> PathBuf {
        self.quarantine_location
            .join(&self.did)
            .join(cid.to_string())
    }

    async fn ensure_dir(&self) -> Result<()> {
        Ok(tokio::fs::create_dir_all(self.location.join(&self.did)).await?)
    }

    async fn ensure_temp(&self) -> Result<()> {
        Ok(tokio::fs::create_dir_all(self.tmp_location.join(&self.did)).await?)
    }

    async fn ensure_quarantine(&self) -> Result<()> {
        Ok(tokio::fs::create_dir_all(self.quarantine_location.join(&self.did)).await?)
    }
}

fn translate_err(err: std::io::Error) -> anyhow::Error {
    if err.kind() == ErrorKind::NotFound {
        BlobNotFoundError.into()
    } else {
        err.into()
    }
}

async fn copy_from_temp(tmp_path: &Path, stored_path: &Path) -> Result<()> {
    let data = tokio::fs::read(tmp_path).await.map_err(translate_err)?;
    Ok(tokio::fs::write(stored_path, data).await?)
}

async fn remove_file_if_exists(path: &Path) -> Result<()> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

async fn remove_dir_if_exists(path: &Path) -> Result<()> {
    match tokio::fs::remove_dir_all(path).await {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

impl BlobStore for DiskBlobStore {
    fn put_temp(&self, bytes: Vec<u8>) -> BoxFuture<'_, Result<String>> {
        Box::pin(async move {
            self.ensure_temp().await?;
            let key = Self::gen_key();
            tokio::fs::write(self.tmp_path(&key), bytes).await?;
            Ok(key)
        })
    }

    fn make_permanent(&self, key: String, cid: Cid) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.ensure_dir().await?;
            let tmp_path = self.tmp_path(&key);
            let stored_path = self.stored_path(cid);
            if !BlobStore::has_stored(self, cid).await? {
                // rename only works within a filesystem; fall back to copy+unlink
                if tokio::fs::rename(&tmp_path, &stored_path).await.is_ok() {
                    return Ok(());
                }
                copy_from_temp(&tmp_path, &stored_path).await?;
            }
            if let Err(err) = remove_file_if_exists(&tmp_path).await {
                tracing::error!(?err, ?tmp_path, "could not delete file from temp storage");
            }
            Ok(())
        })
    }

    fn put_permanent(&self, cid: Cid, bytes: Vec<u8>) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.ensure_dir().await?;
            Ok(tokio::fs::write(self.stored_path(cid), bytes).await?)
        })
    }

    fn quarantine(&self, cid: Cid) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.ensure_quarantine().await?;
            tokio::fs::rename(self.stored_path(cid), self.quarantine_path(cid))
                .await
                .map_err(translate_err)
        })
    }

    fn unquarantine(&self, cid: Cid) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.ensure_dir().await?;
            tokio::fs::rename(self.quarantine_path(cid), self.stored_path(cid))
                .await
                .map_err(translate_err)
        })
    }

    fn get_bytes(&self, cid: Cid) -> BoxFuture<'_, Result<Vec<u8>>> {
        Box::pin(async move {
            tokio::fs::read(self.stored_path(cid))
                .await
                .map_err(translate_err)
        })
    }

    fn get_stream(&self, cid: Cid) -> BoxFuture<'_, Result<ByteStream>> {
        Box::pin(async move {
            let path = self.stored_path(cid);
            if !tokio::fs::try_exists(&path).await? {
                return Err(BlobNotFoundError.into());
            }
            Ok(ByteStream::from_path(&path).await?)
        })
    }

    fn has_temp(&self, key: String) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async move { Ok(tokio::fs::try_exists(self.tmp_path(&key)).await?) })
    }

    fn has_stored(&self, cid: Cid) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async move { Ok(tokio::fs::try_exists(self.stored_path(cid)).await?) })
    }

    fn delete(&self, cid: Cid) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { remove_file_if_exists(&self.stored_path(cid)).await })
    }

    fn delete_many(&self, cids: Vec<Cid>) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let mut error_count = 0;
            for chunk in cids.chunks(DELETE_MANY_CHUNK_SIZE) {
                for cid in chunk {
                    if let Err(err) = BlobStore::delete(self, *cid).await {
                        tracing::error!(?err, cid = %cid, "error deleting blob");
                        error_count += 1;
                    }
                }
            }
            if error_count > 0 {
                bail!("failed to delete {error_count} blobs")
            }
            Ok(())
        })
    }

    fn delete_all(&self) -> Option<BoxFuture<'_, Result<()>>> {
        Some(Box::pin(async move {
            remove_dir_if_exists(&self.location.join(&self.did)).await?;
            remove_dir_if_exists(&self.tmp_location.join(&self.did)).await?;
            remove_dir_if_exists(&self.quarantine_location.join(&self.did)).await
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsky_common::ipld::sha256_to_cid;
    use sha2::{Digest, Sha256};

    const TEST_DID: &str = "did:example:alice";

    fn cid_for(bytes: &[u8]) -> Cid {
        sha256_to_cid(Sha256::digest(bytes).to_vec())
    }

    fn test_store(dir: &Path) -> DiskBlobStore {
        DiskBlobStore::new(TEST_DID.to_owned(), &dir.join("blobs"), None, None)
    }

    #[test]
    fn default_and_custom_locations() {
        let store = DiskBlobStore::new(TEST_DID.to_owned(), Path::new("/blobs"), None, None);
        assert_eq!(store.tmp_location, Path::new("/blobs/tempt"));
        assert_eq!(store.quarantine_location, Path::new("/blobs/quarantine"));

        let store = DiskBlobStore::new(
            TEST_DID.to_owned(),
            Path::new("/blobs"),
            Some(Path::new("/tmp/blobs")),
            Some(Path::new("/quarantine/blobs")),
        );
        assert_eq!(store.tmp_location, Path::new("/tmp/blobs"));
        assert_eq!(store.quarantine_location, Path::new("/quarantine/blobs"));
        let cid = cid_for(b"path check");
        assert_eq!(
            store.tmp_path("key"),
            Path::new("/tmp/blobs/did:example:alice/key")
        );
        assert_eq!(
            store.stored_path(cid),
            Path::new("/blobs/did:example:alice").join(cid.to_string())
        );
        assert_eq!(
            store.quarantine_path(cid),
            Path::new("/quarantine/blobs/did:example:alice").join(cid.to_string())
        );
    }

    #[tokio::test]
    async fn temp_to_permanent_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let bytes = b"hello disk blob".to_vec();
        let cid = cid_for(&bytes);

        let key = store.put_temp(bytes.clone()).await.unwrap();
        assert!(store.has_temp(key.clone()).await.unwrap());
        assert!(!store.has_stored(cid).await.unwrap());

        store.make_permanent(key.clone(), cid).await.unwrap();
        assert!(!store.has_temp(key.clone()).await.unwrap());
        assert!(store.has_stored(cid).await.unwrap());
        assert_eq!(store.get_bytes(cid).await.unwrap(), bytes);
        let streamed = store
            .get_stream(cid)
            .await
            .unwrap()
            .collect()
            .await
            .unwrap()
            .to_vec();
        assert_eq!(streamed, bytes);

        // temp key is a 32-char base32 string
        assert_eq!(key.len(), 32);
        assert!(key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
    }

    #[tokio::test]
    async fn make_permanent_missing_temp_is_blob_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let cid = cid_for(b"never uploaded");
        let err = store
            .make_permanent("missingkey".to_owned(), cid)
            .await
            .unwrap_err();
        assert!(err.downcast_ref::<BlobNotFoundError>().is_some());
    }

    #[tokio::test]
    async fn make_permanent_noops_when_already_stored() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let bytes = b"already stored".to_vec();
        let cid = cid_for(&bytes);
        store.put_permanent(cid, bytes.clone()).await.unwrap();

        let key = store
            .put_temp(b"different temp bytes".to_vec())
            .await
            .unwrap();
        store.make_permanent(key.clone(), cid).await.unwrap();
        // stored content wins and the temp file is cleaned up
        assert_eq!(store.get_bytes(cid).await.unwrap(), bytes);
        assert!(!store.has_temp(key).await.unwrap());
    }

    #[tokio::test]
    async fn make_permanent_logs_undeletable_temp() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let bytes = b"stored bytes".to_vec();
        let cid = cid_for(&bytes);
        store.put_permanent(cid, bytes).await.unwrap();
        // a non-empty directory at the temp path cannot be removed with remove_file
        let tmp_path = store.tmp_path("stubbornkey");
        tokio::fs::create_dir_all(tmp_path.join("child"))
            .await
            .unwrap();
        store
            .make_permanent("stubbornkey".to_owned(), cid)
            .await
            .unwrap();
        assert!(tmp_path.exists());
    }

    #[tokio::test]
    async fn copy_fallback_copies_then_removes() {
        let dir = tempfile::tempdir().unwrap();
        let tmp = dir.path().join("src");
        let stored = dir.path().join("dst");
        tokio::fs::write(&tmp, b"cross device").await.unwrap();
        copy_from_temp(&tmp, &stored).await.unwrap();
        assert_eq!(tokio::fs::read(&stored).await.unwrap(), b"cross device");

        let err = copy_from_temp(&dir.path().join("missing"), &stored)
            .await
            .unwrap_err();
        assert!(err.downcast_ref::<BlobNotFoundError>().is_some());
    }

    #[tokio::test]
    async fn quarantine_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let bytes = b"quarantine me".to_vec();
        let cid = cid_for(&bytes);
        let missing = store.quarantine(cid).await.unwrap_err();
        assert!(missing.downcast_ref::<BlobNotFoundError>().is_some());

        store.put_permanent(cid, bytes.clone()).await.unwrap();
        store.quarantine(cid).await.unwrap();
        assert!(!store.has_stored(cid).await.unwrap());
        assert!(store.quarantine_path(cid).is_file());
        let not_found = store.get_bytes(cid).await.unwrap_err();
        assert!(not_found.downcast_ref::<BlobNotFoundError>().is_some());
        let not_found = store.get_stream(cid).await.unwrap_err();
        assert!(not_found.downcast_ref::<BlobNotFoundError>().is_some());

        store.unquarantine(cid).await.unwrap();
        assert!(store.has_stored(cid).await.unwrap());
        assert_eq!(store.get_bytes(cid).await.unwrap(), bytes);
        let unquarantined = store.unquarantine(cid).await.unwrap_err();
        assert!(unquarantined.downcast_ref::<BlobNotFoundError>().is_some());
    }

    #[tokio::test]
    async fn non_not_found_io_errors_pass_through() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let cid = cid_for(b"directory in the way");
        // a directory at the stored path cannot be read as a file
        tokio::fs::create_dir_all(store.stored_path(cid))
            .await
            .unwrap();
        let err = store.get_bytes(cid).await.unwrap_err();
        assert!(err.downcast_ref::<BlobNotFoundError>().is_none());
    }

    #[tokio::test]
    async fn deletes_single_and_many() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let one = b"one".to_vec();
        let two = b"two".to_vec();
        let (cid_one, cid_two) = (cid_for(&one), cid_for(&two));
        store.put_permanent(cid_one, one).await.unwrap();
        store.put_permanent(cid_two, two).await.unwrap();

        store.delete(cid_one).await.unwrap();
        assert!(!store.has_stored(cid_one).await.unwrap());
        // deleting a missing blob is a no-op
        store.delete(cid_one).await.unwrap();

        store.delete_many(vec![cid_one, cid_two]).await.unwrap();
        assert!(!store.has_stored(cid_two).await.unwrap());
    }

    #[tokio::test]
    async fn delete_many_aggregates_errors() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let ok_bytes = b"deletable".to_vec();
        let ok_cid = cid_for(&ok_bytes);
        store.put_permanent(ok_cid, ok_bytes).await.unwrap();
        // a non-empty directory at a stored path cannot be removed with remove_file
        let bad_cid = cid_for(b"undeletable");
        tokio::fs::create_dir_all(store.stored_path(bad_cid).join("child"))
            .await
            .unwrap();
        let err = store.delete_many(vec![ok_cid, bad_cid]).await.unwrap_err();
        assert!(err.to_string().contains("failed to delete 1 blobs"));
        assert!(!store.has_stored(ok_cid).await.unwrap());
    }

    #[tokio::test]
    async fn delete_all_removes_every_per_did_dir() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let stored = b"stored".to_vec();
        let quarantined = b"quarantined".to_vec();
        let (stored_cid, quarantined_cid) = (cid_for(&stored), cid_for(&quarantined));
        store.put_permanent(stored_cid, stored).await.unwrap();
        store
            .put_permanent(quarantined_cid, quarantined)
            .await
            .unwrap();
        store.quarantine(quarantined_cid).await.unwrap();
        let temp_key = store.put_temp(b"temp".to_vec()).await.unwrap();

        store.delete_all().unwrap().await.unwrap();
        assert!(!store.has_stored(stored_cid).await.unwrap());
        assert!(!store.has_temp(temp_key).await.unwrap());
        assert!(!store.quarantine_path(quarantined_cid).exists());

        // deleting again is a no-op
        store.delete_all().unwrap().await.unwrap();
    }

    #[tokio::test]
    async fn delete_all_propagates_unexpected_errors() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        // a plain file where the per-did directory belongs cannot be removed as a dir
        tokio::fs::create_dir_all(&store.location).await.unwrap();
        tokio::fs::write(store.location.join(TEST_DID), b"not a dir")
            .await
            .unwrap();
        assert!(store.delete_all().unwrap().await.is_err());
    }

    #[tokio::test]
    async fn concurrent_put_temp_keys_are_unique() {
        let dir = tempfile::tempdir().unwrap();
        let store = std::sync::Arc::new(test_store(dir.path()));
        let handles: Vec<_> = (0..32)
            .map(|i| {
                let store = store.clone();
                tokio::spawn(async move { store.put_temp(vec![i as u8]).await.unwrap() })
            })
            .collect();
        let mut keys = std::collections::HashSet::new();
        for handle in handles {
            let key = handle.await.unwrap();
            assert!(store.has_temp(key.clone()).await.unwrap());
            assert!(keys.insert(key));
        }
        assert_eq!(keys.len(), 32);
    }
}
