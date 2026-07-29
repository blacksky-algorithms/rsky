// based on https://github.com/bluesky-social/atproto/blob/main/packages/aws/src/s3.ts
use crate::actor_store::blobstore::BlobStore;
use anyhow::Result;
use aws_config::SdkConfig;
use aws_sdk_s3 as s3;
use aws_sdk_s3::error::SdkError;
use aws_sdk_s3::operation::put_object::builders::PutObjectFluentBuilder;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::{Delete, ObjectCannedAcl, ObjectIdentifier};
use futures::future::BoxFuture;
use lexicon_cid::Cid;
use rsky_common::get_random_str;

struct MoveObject {
    from: String,
    to: String,
}

#[derive(Debug, Clone)]
pub struct S3BlobStore {
    client: s3::Client,
    pub did: String,
    pub bucket: String,
    apply_acl: bool,
}

// Works with any S3-compatible object storage service. A configured bucket
// holds all actors under did-prefixed keys; legacy deployments without a
// configured bucket keep one bucket per actor named after the DID.
impl S3BlobStore {
    pub fn new(did: String, cfg: &SdkConfig, bucket: Option<String>) -> Self {
        let client = aws_sdk_s3::Client::new(cfg);
        let apply_acl = !is_gcs_endpoint(cfg.endpoint_url());
        let bucket = bucket.unwrap_or_else(|| did.clone());
        S3BlobStore {
            client,
            did,
            bucket,
            apply_acl,
        }
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
        let req = self
            .client
            .put_object()
            .body(ByteStream::from(bytes))
            .bucket(&self.bucket)
            .key(key);
        if self.apply_acl {
            req.acl(ObjectCannedAcl::PublicRead)
        } else {
            req
        }
    }

    pub async fn put_temp(&self, bytes: Vec<u8>) -> Result<String> {
        let key = self.gen_key();
        self.put_object_request(self.get_tmp_path(&key), bytes)
            .send()
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
        self.put_object_request(self.get_stored_path(cid), bytes)
            .send()
            .await?;
        Ok(())
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
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await?;
        Ok(())
    }

    async fn delete_many_keys(&self, keys: Vec<String>) -> Result<()> {
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

    async fn move_object(&self, keys: MoveObject) -> Result<()> {
        let req = self
            .client
            .copy_object()
            .bucket(&self.bucket)
            .copy_source(format!("{0}/{1}", self.bucket, keys.from))
            .key(keys.to);
        let req = if self.apply_acl {
            req.acl(ObjectCannedAcl::PublicRead)
        } else {
            req
        };
        req.send().await?;
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(keys.from)
            .send()
            .await?;
        Ok(())
    }
}

fn is_gcs_endpoint(endpoint_url: Option<&str>) -> bool {
    endpoint_url.is_some_and(|url| url.contains("storage.googleapis.com"))
}

impl BlobStore for S3BlobStore {
    fn put_temp(&self, bytes: Vec<u8>) -> BoxFuture<'_, Result<String>> {
        Box::pin(S3BlobStore::put_temp(self, bytes))
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
        );
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
        let store = S3BlobStore::new("did:example:alice".to_owned(), &cfg, None);
        assert_eq!(store.bucket, "did:example:alice");
        assert!(store.apply_acl);
    }

    #[test]
    fn gcs_endpoint_disables_per_object_acls() {
        assert!(!is_gcs_endpoint(None));
        assert!(!is_gcs_endpoint(Some("https://s3.us-east-1.amazonaws.com")));
        assert!(is_gcs_endpoint(Some("https://storage.googleapis.com")));

        let cfg = sdk_config(Some("https://storage.googleapis.com"));
        let store = S3BlobStore::new(
            "did:example:alice".to_owned(),
            &cfg,
            Some("gcs-bucket".to_owned()),
        );
        assert!(!store.apply_acl);
    }
}
