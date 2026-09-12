// based on https://github.com/bluesky-social/atproto/blob/main/packages/aws/src/s3.ts
use crate::actor_store::blobstore::BlobStore;
use crate::blob_attempts::{AttemptJournal, AttemptOutcome};
use anyhow::Result;
use aws_config::retry::RetryConfig;
use aws_config::SdkConfig;
use aws_sdk_s3 as s3;
use aws_sdk_s3::error::SdkError;
use aws_sdk_s3::operation::put_object::builders::PutObjectFluentBuilder;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::{Delete, ObjectIdentifier};
use futures::future::BoxFuture;
use lexicon_cid::Cid;
use rsky_common::get_random_str;

struct MoveObject {
    from: String,
    to: String,
}

#[derive(Clone)]
pub struct S3BlobStore {
    client: s3::Client,
    pub did: String,
    pub bucket: String,
    path_style: bool,
    attempts: Option<AttemptJournal>,
}

impl std::fmt::Debug for S3BlobStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("S3BlobStore")
            .field("did", &self.did)
            .field("bucket", &self.bucket)
            .finish_non_exhaustive()
    }
}

// Works with any S3-compatible object storage service. A configured bucket
// holds all actors under did-prefixed keys; legacy deployments without a
// configured bucket keep one bucket per actor named after the DID.
impl S3BlobStore {
    /// The client never retries on its own: a request that timed out may
    /// still complete, and a hidden retry would report one success for two
    /// attempts. The caller journals each attempt instead.
    ///
    /// Objects are written without an ACL, as the reference PDS writes them;
    /// what is readable is the bucket's policy, not a per-object grant.
    pub fn new(
        did: String,
        cfg: &SdkConfig,
        bucket: Option<String>,
        force_path_style: bool,
    ) -> Self {
        let config = s3::config::Builder::from(cfg)
            .retry_config(RetryConfig::disabled())
            .force_path_style(force_path_style)
            .build();
        let client = aws_sdk_s3::Client::from_conf(config);
        let bucket = bucket.unwrap_or_else(|| did.clone());
        S3BlobStore {
            client,
            did,
            bucket,
            path_style: force_path_style,
            attempts: None,
        }
    }

    pub fn path_style(&self) -> bool {
        self.path_style
    }

    pub fn with_attempts(mut self, attempts: AttemptJournal) -> Self {
        self.attempts = Some(attempts);
        self
    }

    pub fn retries_disabled(&self) -> bool {
        self.client
            .config()
            .retry_config()
            .map(|retry| retry.max_attempts() == 1)
            .unwrap_or(false)
    }

    /// Runs one physical request with its attempt journaled before it is
    /// sent and its outcome recorded after.
    async fn attempt<T, F>(&self, key: &str, operation: &str, request: F) -> Result<T>
    where
        F: std::future::Future<Output = Result<T>>,
    {
        let Some(journal) = &self.attempts else {
            return request.await;
        };
        let id = journal.begin(&self.did, key, operation).await?;
        let result = request.await;
        let outcome = match &result {
            Ok(_) => AttemptOutcome::Succeeded,
            Err(err) => AttemptOutcome::Failed(err.to_string()),
        };
        journal.resolve(id, outcome).await?;
        result
    }

    fn gen_key(&self) -> String {
        get_random_str()
    }

    fn get_tmp_path(&self, key: &str) -> String {
        format!("tmp/{0}/{1}", self.did, key)
    }

    fn get_stored_path(&self, cid: Cid) -> String {
        format!("blocks/{0}/{1}", self.did, cid)
    }

    fn get_quarantined_path(&self, cid: Cid) -> String {
        format!("quarantine/{0}/{1}", self.did, cid)
    }

    fn put_object_request(&self, key: String, bytes: Vec<u8>) -> PutObjectFluentBuilder {
        self.client
            .put_object()
            .body(ByteStream::from(bytes))
            .bucket(&self.bucket)
            .key(key)
    }

    pub async fn put_temp(&self, bytes: Vec<u8>) -> Result<String> {
        let key = self.gen_key();
        let path = self.get_tmp_path(&key);
        self.attempt(&path, "put", async {
            self.put_object_request(path.clone(), bytes).send().await?;
            Ok(())
        })
        .await?;
        Ok(key)
    }

    pub async fn put_temp_from_path(&self, path: std::path::PathBuf) -> Result<String> {
        let key = self.gen_key();
        let object = self.get_tmp_path(&key);
        self.attempt(&object, "put", async {
            self.client
                .put_object()
                .body(ByteStream::from_path(path).await?)
                .bucket(&self.bucket)
                .key(object.clone())
                .send()
                .await?;
            Ok(())
        })
        .await?;
        Ok(key)
    }

    pub async fn make_permanent(&self, key: String, cid: Cid) -> Result<()> {
        let already_has = self.has_stored(cid).await?;
        if !already_has {
            self.move_object(MoveObject {
                from: self.get_tmp_path(&key),
                to: self.get_stored_path(cid),
            })
            .await
        } else {
            // already saved, so we no-op & just delete the temp
            self.delete_key(self.get_tmp_path(&key)).await
        }
    }

    pub async fn put_permanent(&self, cid: Cid, bytes: Vec<u8>) -> Result<()> {
        let path = self.get_stored_path(cid);
        self.attempt(&path, "put", async {
            self.put_object_request(path.clone(), bytes).send().await?;
            Ok(())
        })
        .await
    }

    pub async fn quarantine(&self, cid: Cid) -> Result<()> {
        self.move_object(MoveObject {
            from: self.get_stored_path(cid),
            to: self.get_quarantined_path(cid),
        })
        .await
    }

    pub async fn unquarantine(&self, cid: Cid) -> Result<()> {
        self.move_object(MoveObject {
            from: self.get_quarantined_path(cid),
            to: self.get_stored_path(cid),
        })
        .await
    }

    async fn get_object(&self, cid: Cid) -> Result<ByteStream> {
        let res = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(self.get_stored_path(cid))
            .send()
            .await;
        match res {
            Ok(res) => Ok(res.body),
            Err(SdkError::ServiceError(s)) => Err(anyhow::Error::new(s.into_err())),
            Err(e) => Err(anyhow::Error::new(e.into_service_error())),
        }
    }

    pub async fn get_bytes(&self, cid: Cid) -> Result<Vec<u8>> {
        let res = self.get_object(cid).await?;
        let bytes = res.collect().await.map(|data| data.into_bytes())?;
        Ok(bytes.to_vec())
    }

    pub async fn get_stream(&self, cid: Cid) -> Result<ByteStream> {
        self.get_object(cid).await
    }

    pub async fn delete(&self, cid: Cid) -> Result<()> {
        self.delete_key(self.get_stored_path(cid)).await
    }

    pub async fn delete_many(&self, cids: Vec<Cid>) -> Result<()> {
        let keys: Vec<String> = cids
            .into_iter()
            .map(|cid| self.get_stored_path(cid))
            .collect();
        self.delete_many_keys(keys).await
    }

    pub async fn has_stored(&self, cid: Cid) -> Result<bool> {
        Ok(self.has_key(self.get_stored_path(cid)).await)
    }

    pub async fn has_temp(&self, key: String) -> Result<bool> {
        Ok(self.has_key(self.get_tmp_path(&key)).await)
    }

    async fn has_key(&self, key: String) -> bool {
        let res = self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await;
        res.is_ok()
    }

    async fn delete_key(&self, key: String) -> Result<()> {
        self.attempt(&key, "delete", async {
            self.client
                .delete_object()
                .bucket(&self.bucket)
                .key(key.clone())
                .send()
                .await?;
            Ok(())
        })
        .await
    }

    /// Deletes each key as its own journaled attempt: a bulk request
    /// reports failures per key, and a per-key row is what a collector can
    /// reason about.
    async fn delete_many_keys(&self, keys: Vec<String>) -> Result<()> {
        if self.attempts.is_some() {
            for key in keys {
                self.delete_key(key).await?;
            }
            return Ok(());
        }
        let objects: Vec<ObjectIdentifier> = keys
            .into_iter()
            .map(|key| Ok(ObjectIdentifier::builder().key(key).build()?))
            .collect::<Result<Vec<ObjectIdentifier>>>()?;
        let deletes = Delete::builder().set_objects(Some(objects)).build()?;
        self.client
            .delete_objects()
            .bucket(&self.bucket)
            .delete(deletes)
            .send()
            .await?;
        Ok(())
    }

    async fn copy_object(&self, keys: MoveObject) -> Result<()> {
        let to = keys.to.clone();
        self.attempt(&to, "copy", async {
            self.client
                .copy_object()
                .bucket(&self.bucket)
                .copy_source(format!("{0}/{1}", self.bucket, keys.from))
                .key(keys.to)
                .send()
                .await?;
            Ok(())
        })
        .await
    }

    async fn move_object(&self, keys: MoveObject) -> Result<()> {
        let from = keys.from.clone();
        self.copy_object(keys).await?;
        self.delete_key(from).await
    }

    pub async fn make_permanent_copy_only(&self, key: String, cid: Cid) -> Result<()> {
        if self.has_stored(cid).await? {
            return Ok(());
        }
        self.copy_object(MoveObject {
            from: self.get_tmp_path(&key),
            to: self.get_stored_path(cid),
        })
        .await
    }

    pub async fn has_quarantined(&self, cid: Cid) -> Result<bool> {
        Ok(self.has_key(self.get_quarantined_path(cid)).await)
    }

    pub async fn restore_copy_only(&self, cid: Cid) -> Result<()> {
        if self.has_stored(cid).await? {
            return Ok(());
        }
        self.copy_object(MoveObject {
            from: self.get_quarantined_path(cid),
            to: self.get_stored_path(cid),
        })
        .await
    }
}

impl BlobStore for S3BlobStore {
    fn put_temp(&self, bytes: Vec<u8>) -> BoxFuture<'_, Result<String>> {
        Box::pin(S3BlobStore::put_temp(self, bytes))
    }

    fn put_temp_from_path(&self, path: std::path::PathBuf) -> BoxFuture<'_, Result<String>> {
        Box::pin(S3BlobStore::put_temp_from_path(self, path))
    }

    fn make_permanent(&self, key: String, cid: Cid) -> BoxFuture<'_, Result<()>> {
        Box::pin(S3BlobStore::make_permanent(self, key, cid))
    }

    fn put_permanent(&self, cid: Cid, bytes: Vec<u8>) -> BoxFuture<'_, Result<()>> {
        Box::pin(S3BlobStore::put_permanent(self, cid, bytes))
    }

    fn quarantine(&self, cid: Cid) -> BoxFuture<'_, Result<()>> {
        Box::pin(S3BlobStore::quarantine(self, cid))
    }

    fn unquarantine(&self, cid: Cid) -> BoxFuture<'_, Result<()>> {
        Box::pin(S3BlobStore::unquarantine(self, cid))
    }

    fn get_bytes(&self, cid: Cid) -> BoxFuture<'_, Result<Vec<u8>>> {
        Box::pin(S3BlobStore::get_bytes(self, cid))
    }

    fn get_stream(&self, cid: Cid) -> BoxFuture<'_, Result<ByteStream>> {
        Box::pin(S3BlobStore::get_stream(self, cid))
    }

    fn has_temp(&self, key: String) -> BoxFuture<'_, Result<bool>> {
        Box::pin(S3BlobStore::has_temp(self, key))
    }

    fn has_stored(&self, cid: Cid) -> BoxFuture<'_, Result<bool>> {
        Box::pin(S3BlobStore::has_stored(self, cid))
    }

    fn delete(&self, cid: Cid) -> BoxFuture<'_, Result<()>> {
        Box::pin(S3BlobStore::delete(self, cid))
    }

    fn delete_many(&self, cids: Vec<Cid>) -> BoxFuture<'_, Result<()>> {
        Box::pin(S3BlobStore::delete_many(self, cids))
    }

    fn make_permanent_copy_only(&self, key: String, cid: Cid) -> BoxFuture<'_, Result<()>> {
        Box::pin(S3BlobStore::make_permanent_copy_only(self, key, cid))
    }

    fn has_quarantined(&self, cid: Cid) -> BoxFuture<'_, Result<bool>> {
        Box::pin(S3BlobStore::has_quarantined(self, cid))
    }

    fn restore_copy_only(&self, cid: Cid) -> BoxFuture<'_, Result<()>> {
        Box::pin(S3BlobStore::restore_copy_only(self, cid))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_s3::config::BehaviorVersion;
    use rsky_common::ipld::sha256_to_cid;
    use sha2::{Digest, Sha256};

    fn sdk_config(endpoint: Option<&str>) -> SdkConfig {
        let builder = SdkConfig::builder().behavior_version(BehaviorVersion::latest());
        match endpoint {
            Some(endpoint) => builder.endpoint_url(endpoint).build(),
            None => builder.build(),
        }
    }

    #[test]
    fn did_prefixed_key_layout() {
        let cfg = sdk_config(None);
        let store = S3BlobStore::new(
            "did:example:alice".to_owned(),
            &cfg,
            Some("shared-bucket".to_owned()),
            true,
        );
        assert!(store.path_style());
        assert_eq!(store.bucket, "shared-bucket");
        assert_eq!(store.get_tmp_path("key"), "tmp/did:example:alice/key");
        let cid = sha256_to_cid(Sha256::digest(b"layout").to_vec());
        assert_eq!(
            store.get_stored_path(cid),
            format!("blocks/did:example:alice/{cid}")
        );
        assert_eq!(
            store.get_quarantined_path(cid),
            format!("quarantine/did:example:alice/{cid}")
        );
        let key = store.gen_key();
        assert_eq!(key.len(), 32);
    }

    #[test]
    fn legacy_fallback_uses_did_as_bucket() {
        let cfg = sdk_config(Some("https://nyc3.digitaloceanspaces.com"));
        let store = S3BlobStore::new("did:example:alice".to_owned(), &cfg, None, false);
        assert_eq!(store.bucket, "did:example:alice");
        assert!(!store.path_style());
        assert!(store.retries_disabled());
        assert!(format!("{store:?}").contains("did:example:alice"));
    }

    /// A request against an endpoint that does not answer is journaled as
    /// one failed attempt, never retried behind the journal's back.
    #[tokio::test]
    async fn every_request_is_one_journaled_attempt() {
        let dir = tempfile::tempdir().unwrap();
        let journal = AttemptJournal::open(dir.path().join("attempts.sqlite"), true)
            .await
            .unwrap();
        let cfg = SdkConfig::builder()
            .behavior_version(BehaviorVersion::latest())
            .endpoint_url("http://127.0.0.1:1")
            .region(aws_sdk_s3::config::Region::new("us-east-1"))
            .credentials_provider(aws_sdk_s3::config::SharedCredentialsProvider::new(
                aws_sdk_s3::config::Credentials::new("k", "s", None, None, "test"),
            ))
            .build();
        let store = S3BlobStore::new(
            "did:example:alice".to_owned(),
            &cfg,
            Some("bucket".to_owned()),
            false,
        )
        .with_attempts(journal.clone());
        let cid = sha256_to_cid(Sha256::digest(b"unreachable").to_vec());
        assert!(store
            .put_permanent(cid, b"unreachable".to_vec())
            .await
            .is_err());
        assert!(store.put_temp(b"unreachable".to_vec()).await.is_err());
        assert!(store.delete(cid).await.is_err());
        assert!(store.delete_many(vec![cid]).await.is_err());
        assert!(store.restore_copy_only(cid).await.is_err());
        let stored = store.get_stored_path(cid);
        let attempts = journal
            .attempts("did:example:alice", &stored)
            .await
            .unwrap();
        assert_eq!(
            attempts
                .iter()
                .map(|attempt| attempt.operation.as_str())
                .collect::<Vec<_>>(),
            ["put", "delete", "delete", "copy"]
        );
        assert!(attempts.iter().all(|attempt| attempt
            .outcome
            .as_deref()
            .unwrap_or("")
            .starts_with("failed")));
        assert!(journal
            .unresolved("did:example:alice")
            .await
            .unwrap()
            .is_empty());
        // without a journal the bulk delete goes out as one request
        let bare = S3BlobStore::new(
            "did:example:alice".to_owned(),
            &cfg,
            Some("b".to_owned()),
            false,
        );
        assert!(bare.delete_many(vec![cid]).await.is_err());
        assert!(bare.make_permanent("k".to_owned(), cid).await.is_err());
        assert!(bare.quarantine(cid).await.is_err());
    }
}
