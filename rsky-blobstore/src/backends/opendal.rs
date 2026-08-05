// Blobstore implementation backed by opendal so the same code can drive any
// opendal-supported backend (S3, GCS, Azure, local fs, sftp, ...) without
// pulling in each service's SDK directly.
use crate::blobstore::{
    generation_name, BlobNotFoundError, BlobStore, BoxedBlobStream, DeleteError, ObjectKind,
};
use anyhow::{anyhow, bail, Result};
use bytes::Bytes;
use futures::future::BoxFuture;
use futures::stream::StreamExt;
use lexicon_cid::Cid;
use opendal::{services, Operator};
use std::io::ErrorKind;

#[derive(Debug, Clone)]
pub struct OpendalBlobStore {
    op: Operator,
    pub did: String,
}

impl OpendalBlobStore {
    pub fn new(op: Operator, did: String) -> Self {
        OpendalBlobStore { op, did }
    }
}

/// Build an [`Operator`] for one of the supported opendal schemes. New schemes
/// can be added here without changing the blobstore trait.
pub fn build_operator(kind: &str, bucket: &str) -> Result<Operator> {
    let op = match kind {
        "fs" => Operator::new(services::Fs::default().root(bucket))?.finish(),
        "s3" => Operator::new(services::S3::default().bucket(bucket))?.finish(),
        other => bail!("unsupported opendal operator kind: {other}"),
    };
    Ok(op)
}

/// Convenience constructor: build an [`OpendalBlobStore`] in one call. Useful
/// for callers (e.g. rsky-pds's `BlobstoreFactory`) that don't already hold an
/// [`Operator`].
pub fn new_blobstore(kind: &str, bucket: &str, did: String) -> Result<OpendalBlobStore> {
    Ok(OpendalBlobStore::new(build_operator(kind, bucket)?, did))
}

fn translate_err(err: opendal::Error) -> anyhow::Error {
    if err.kind() == opendal::ErrorKind::NotFound {
        BlobNotFoundError.into()
    } else {
        anyhow!(err)
    }
}

fn gen_key() -> String {
    use rand::RngCore;
    format!("{:016x}", rand::thread_rng().next_u64())
}

impl BlobStore for OpendalBlobStore {
    fn namespace_prefixes(&self) -> Vec<String> {
        vec![
            format!("blocks/{}/", self.did),
            format!("tmp/{}/", self.did),
            format!("quarantine/{}/", self.did),
        ]
    }

    fn object_key(&self, kind: ObjectKind, name: &str, generation: u32) -> String {
        let name = generation_name(name, generation);
        match kind {
            ObjectKind::Permanent => format!("blocks/{}/{name}", self.did),
            ObjectKind::Temp => format!("tmp/{}/{name}", self.did),
            ObjectKind::Quarantine => format!("quarantine/{}/{name}", self.did),
        }
    }

    fn list_objects(&self, prefix: String) -> BoxFuture<'_, Result<Vec<String>>> {
        let op = self.op.clone();
        Box::pin(async move {
            let entries = op
                .list_with(&prefix)
                .recursive(true)
                .await
                .map_err(translate_err)?;
            Ok(entries
                .into_iter()
                .map(|entry| entry.into_parts().0)
                .filter(|path| !path.ends_with('/'))
                .collect())
        })
    }

    fn delete_object(&self, key: String) -> BoxFuture<'_, std::result::Result<(), DeleteError>> {
        let op = self.op.clone();
        Box::pin(async move {
            match op.delete(&key).await {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == opendal::ErrorKind::NotFound => Ok(()),
                Err(err) if err.is_temporary() => Err(DeleteError::Ambiguous(err.to_string())),
                Err(err) => Err(DeleteError::Definitive(err.to_string())),
            }
        })
    }

    fn object_exists(&self, key: String) -> BoxFuture<'_, Result<bool>> {
        let op = self.op.clone();
        Box::pin(async move { op.exists(&key).await.map_err(translate_err) })
    }

    fn get_object(&self, key: String) -> BoxFuture<'_, Result<Vec<u8>>> {
        let op = self.op.clone();
        Box::pin(async move {
            op.read(&key)
                .await
                .map(|buf| buf.to_vec())
                .map_err(translate_err)
        })
    }

    fn put_object(&self, key: String, bytes: Vec<u8>) -> BoxFuture<'_, Result<()>> {
        let op = self.op.clone();
        Box::pin(async move {
            op.write(&key, bytes)
                .await
                .map(|_| ())
                .map_err(translate_err)
        })
    }

    fn put_temp(&self, bytes: Vec<u8>) -> BoxFuture<'_, Result<String>> {
        let key = gen_key();
        let path = self.object_key(ObjectKind::Temp, &key, 0);
        let op = self.op.clone();
        Box::pin(async move {
            op.write(&path, bytes).await.map_err(translate_err)?;
            Ok(key)
        })
    }

    fn make_permanent(&self, key: String, cid: Cid) -> BoxFuture<'_, Result<()>> {
        let from = self.object_key(ObjectKind::Temp, &key, 0);
        let to = self.object_key(ObjectKind::Permanent, &cid.to_string(), 0);
        let op = self.op.clone();
        Box::pin(async move {
            if op.exists(&to).await.map_err(translate_err)? {
                op.delete(&from).await.map_err(translate_err)?;
                return Ok(());
            }
            op.rename(&from, &to).await.map_err(translate_err)?;
            Ok(())
        })
    }

    fn put_permanent(&self, cid: Cid, bytes: Vec<u8>) -> BoxFuture<'_, Result<()>> {
        let path = self.object_key(ObjectKind::Permanent, &cid.to_string(), 0);
        let op = self.op.clone();
        Box::pin(async move {
            op.write(&path, bytes).await.map_err(translate_err)?;
            Ok(())
        })
    }

    fn make_permanent_copy_only(&self, key: String, cid: Cid) -> BoxFuture<'_, Result<()>> {
        let from = self.object_key(ObjectKind::Temp, &key, 0);
        let to = self.object_key(ObjectKind::Permanent, &cid.to_string(), 0);
        let op = self.op.clone();
        Box::pin(async move {
            if !op.exists(&to).await.map_err(translate_err)? {
                op.copy(&from, &to).await.map_err(translate_err)?;
            }
            Ok(())
        })
    }

    fn has_quarantined(&self, cid: Cid) -> BoxFuture<'_, Result<bool>> {
        let path = self.object_key(ObjectKind::Quarantine, &cid.to_string(), 0);
        let op = self.op.clone();
        Box::pin(async move { op.exists(&path).await.map_err(translate_err) })
    }

    fn restore_copy_only(&self, cid: Cid) -> BoxFuture<'_, Result<()>> {
        let from = self.object_key(ObjectKind::Quarantine, &cid.to_string(), 0);
        let to = self.object_key(ObjectKind::Permanent, &cid.to_string(), 0);
        let op = self.op.clone();
        Box::pin(async move {
            if !op.exists(&to).await.map_err(translate_err)? {
                op.copy(&from, &to).await.map_err(translate_err)?;
            }
            Ok(())
        })
    }

    fn quarantine(&self, cid: Cid) -> BoxFuture<'_, Result<()>> {
        let from = self.object_key(ObjectKind::Permanent, &cid.to_string(), 0);
        let to = self.object_key(ObjectKind::Quarantine, &cid.to_string(), 0);
        let op = self.op.clone();
        Box::pin(async move { op.rename(&from, &to).await.map_err(translate_err) })
    }

    fn unquarantine(&self, cid: Cid) -> BoxFuture<'_, Result<()>> {
        let from = self.object_key(ObjectKind::Quarantine, &cid.to_string(), 0);
        let to = self.object_key(ObjectKind::Permanent, &cid.to_string(), 0);
        let op = self.op.clone();
        Box::pin(async move { op.rename(&from, &to).await.map_err(translate_err) })
    }

    fn get_bytes(&self, cid: Cid) -> BoxFuture<'_, Result<Vec<u8>>> {
        let path = self.object_key(ObjectKind::Permanent, &cid.to_string(), 0);
        let op = self.op.clone();
        Box::pin(async move {
            let buf = op.read(&path).await.map_err(translate_err)?;
            Ok(buf.to_vec())
        })
    }

    fn get_stream(&self, cid: Cid) -> BoxFuture<'_, Result<BoxedBlobStream>> {
        let path = self.object_key(ObjectKind::Permanent, &cid.to_string(), 0);
        let op = self.op.clone();
        Box::pin(async move {
            let reader = match op.reader(&path).await {
                Ok(r) => r,
                Err(e) if e.kind() == opendal::ErrorKind::NotFound => {
                    return Err(BlobNotFoundError.into());
                }
                Err(e) => return Err(anyhow!(e)),
            };
            let mut async_read = reader
                .into_futures_async_read(..)
                .await
                .map_err(translate_err)?;
            let stream = futures::stream::poll_fn(move |cx| {
                let mut buf = vec![0u8; 16 * 1024];
                let pinned = std::pin::Pin::new(&mut async_read);
                match futures::AsyncRead::poll_read(pinned, cx, &mut buf) {
                    std::task::Poll::Ready(Ok(0)) => std::task::Poll::Ready(None),
                    std::task::Poll::Ready(Ok(n)) => {
                        std::task::Poll::Ready(Some(Ok(Bytes::copy_from_slice(&buf[..n]))))
                    }
                    std::task::Poll::Ready(Err(e)) if e.kind() == ErrorKind::UnexpectedEof => {
                        std::task::Poll::Ready(None)
                    }
                    std::task::Poll::Ready(Err(e)) => std::task::Poll::Ready(Some(Err(anyhow!(e)))),
                    std::task::Poll::Pending => std::task::Poll::Pending,
                }
            });
            Ok(stream.boxed())
        })
    }

    fn has_temp(&self, key: String) -> BoxFuture<'_, Result<bool>> {
        let path = self.object_key(ObjectKind::Temp, &key, 0);
        let op = self.op.clone();
        Box::pin(async move { op.exists(&path).await.map_err(translate_err) })
    }

    fn has_stored(&self, cid: Cid) -> BoxFuture<'_, Result<bool>> {
        let path = self.object_key(ObjectKind::Permanent, &cid.to_string(), 0);
        let op = self.op.clone();
        Box::pin(async move { op.exists(&path).await.map_err(translate_err) })
    }

    fn delete(&self, cid: Cid) -> BoxFuture<'_, Result<()>> {
        let path = self.object_key(ObjectKind::Permanent, &cid.to_string(), 0);
        let op = self.op.clone();
        Box::pin(async move {
            // opendal.delete is a no-op on missing paths; nothing to translate.
            op.delete(&path).await.map_err(translate_err)
        })
    }

    fn delete_many(&self, cids: Vec<Cid>) -> BoxFuture<'_, Result<()>> {
        let paths: Vec<String> = cids
            .iter()
            .map(|cid| self.object_key(ObjectKind::Permanent, &cid.to_string(), 0))
            .collect();
        let op = self.op.clone();
        Box::pin(async move {
            for path in paths {
                let _ = op.delete(&path).await;
            }
            Ok(())
        })
    }

    fn delete_all(&self) -> Option<BoxFuture<'_, Result<()>>> {
        let prefixes = self.namespace_prefixes();
        let op = self.op.clone();
        Some(Box::pin(async move {
            for prefix in prefixes {
                op.remove_all(prefix.trim_end_matches('/'))
                    .await
                    .map_err(translate_err)?;
            }
            Ok(())
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::TryStreamExt;
    use sha2::{Digest, Sha256};

    const SHA2_256: u64 = 0x12;
    const RAWCODEC: u64 = 0x55;
    const DID: &str = "did:example:alice";

    fn cid_for(bytes: &[u8]) -> Cid {
        let hash = Sha256::digest(bytes).to_vec();
        Cid::new_v1(
            RAWCODEC,
            lexicon_cid::multihash::Multihash::<64>::wrap(SHA2_256, &hash).unwrap(),
        )
    }

    fn temp_store() -> OpendalBlobStore {
        let dir = tempfile::tempdir().unwrap();
        let op = build_operator("fs", dir.path().to_str().unwrap()).unwrap();
        OpendalBlobStore::new(op, DID.to_owned())
    }

    async fn streamed_bytes(stream: BoxedBlobStream) -> Vec<u8> {
        let chunks: Vec<Bytes> = stream.try_collect().await.unwrap();
        chunks
            .iter()
            .flat_map(|chunk| chunk.iter().copied())
            .collect()
    }

    #[tokio::test]
    async fn temp_to_permanent_lifecycle() {
        let store = temp_store();
        let bytes = b"hello opendal blob".to_vec();
        let cid = cid_for(&bytes);
        let key = store.put_temp(bytes.clone()).await.unwrap();
        assert!(BlobStore::has_temp(&store, key.clone()).await.unwrap());
        assert!(!store.has_stored(cid).await.unwrap());
        store.make_permanent(key.clone(), cid).await.unwrap();
        assert!(!store.has_temp(key.clone()).await.unwrap());
        assert!(store.has_stored(cid).await.unwrap());
        assert_eq!(BlobStore::get_bytes(&store, cid).await.unwrap(), bytes);
        assert_eq!(
            streamed_bytes(BlobStore::get_stream(&store, cid).await.unwrap()).await,
            bytes
        );
    }

    #[tokio::test]
    async fn quarantine_round_trip() {
        let store = temp_store();
        let bytes = b"quarantine me".to_vec();
        let cid = cid_for(&bytes);
        store.put_permanent(cid, bytes).await.unwrap();
        store.quarantine(cid).await.unwrap();
        assert!(!store.has_stored(cid).await.unwrap());
        assert!(store.has_quarantined(cid).await.unwrap());
        store.unquarantine(cid).await.unwrap();
        assert!(store.has_stored(cid).await.unwrap());
        assert!(!store.has_quarantined(cid).await.unwrap());
    }

    #[tokio::test]
    async fn missing_blob_returns_not_found() {
        let store = temp_store();
        let cid = cid_for(b"never uploaded");
        let err = BlobStore::get_bytes(&store, cid).await.unwrap_err();
        assert!(err.downcast_ref::<BlobNotFoundError>().is_some());
        match store.get_stream(cid).await {
            Ok(_) => panic!("expected BlobNotFoundError"),
            Err(err) => assert!(err.downcast_ref::<BlobNotFoundError>().is_some()),
        }
    }

    #[tokio::test]
    async fn delete_is_idempotent() {
        let store = temp_store();
        let cid = cid_for(b"deletable");
        store
            .put_permanent(cid, b"deletable".to_vec())
            .await
            .unwrap();
        store.delete(cid).await.unwrap();
        store.delete(cid).await.unwrap();
    }

    #[tokio::test]
    async fn generation_keys_share_the_object_layout() {
        let store = temp_store();
        let bytes = b"generation".to_vec();
        let cid = cid_for(&bytes);
        let base = store.object_key(ObjectKind::Permanent, &cid.to_string(), 0);
        assert_eq!(base, format!("blocks/{DID}/{cid}"));
        let g1 = store.object_key(ObjectKind::Permanent, &cid.to_string(), 1);
        assert_eq!(g1, format!("blocks/{DID}/{cid}.g1"));
        assert_eq!(g1, format!("{}{cid}.g1", store.namespace_prefixes()[0]));

        store.put_object(base.clone(), bytes.clone()).await.unwrap();
        store.put_object(g1.clone(), bytes.clone()).await.unwrap();
        let listed = store
            .list_objects(store.namespace_prefixes()[0].clone())
            .await
            .unwrap();
        assert!(listed.contains(&base), "{listed:?}");
        assert!(listed.contains(&g1), "{listed:?}");
        assert_eq!(store.get_object(g1.clone()).await.unwrap(), bytes);
        assert!(store.object_exists(g1.clone()).await.unwrap());
        store.delete_object(g1.clone()).await.unwrap();
        assert!(!store.object_exists(g1).await.unwrap());
        assert!(store.object_exists(base).await.unwrap());
    }
}
