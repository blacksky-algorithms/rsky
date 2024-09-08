use std::str::FromStr;
// based on https://github.com/bluesky-social/atproto/blob/main/packages/aws/src/s3.ts
use crate::common::env::env_str;
use crate::common::get_random_str;
use anyhow::Result;
use aws_config::SdkConfig;
use aws_sdk_s3 as s3;
use aws_sdk_s3::error::SdkError;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::{Delete, ObjectCannedAcl, ObjectIdentifier};
use lexicon_cid::Cid;

struct MoveObject {
    from: String,
    to: String,
}

#[derive(Debug, Clone)]
pub struct S3BlobStore {
    client: s3::Client,
    pub bucket: String,
}

// Intended to work with DigitalOcean Spaces Object Storage which is an
// S3-compatible object storage service
impl S3BlobStore {
    pub fn new(did: String, cfg: &SdkConfig) -> Self {
        let client = aws_sdk_s3::Client::new(cfg);
        S3BlobStore {
            client,
            bucket: did,
        }
    }

    pub fn creator(cfg: &SdkConfig) -> Box<dyn Fn(String) -> S3BlobStore + '_> {
        Box::new(move |did: String| {
            return S3BlobStore::new(did, cfg);
        })
    }

    fn gen_key(&self) -> String {
        get_random_str()
    }

    fn get_tmp_path(&self, key: &String) -> String {
        format!("tmp/{0}/{1}", self.bucket, key)
    }

    fn get_stored_path(&self, cid: Cid) -> String {
        format!("blocks/{0}/{1}", self.bucket, cid.to_string())
    }

    fn get_quarantined_path(&self, cid: Cid) -> String {
        format!("quarantine/{0}/{1}", self.bucket, cid.to_string())
    }

    pub async fn put_temp(&self, bytes: Vec<u8>) -> Result<String> {
        let key = self.gen_key();
        let body = ByteStream::from(bytes);
        self.client
            .put_object()
            .body(body)
            .bucket(&self.bucket)
            .key(self.get_tmp_path(&key))
            .acl(ObjectCannedAcl::PublicRead)
            .send()
            .await?;
        Ok(key)
    }

    pub async fn make_permanent(&self, key: String, cid: Cid) -> Result<()> {
        let already_has = self.has_stored(cid).await?;
        if !already_has {
            Ok(self
                .move_object(MoveObject {
                    from: self.get_tmp_path(&key),
                    to: self.get_stored_path(cid),
                })
                .await?)
        } else {
            // already saved, so we no-op & just delete the temp
            Ok(self.delete_key(self.get_tmp_path(&key)).await?)
        }
    }

    pub async fn put_permanent(&self, cid: Cid, bytes: Vec<u8>) -> Result<()> {
        let body = ByteStream::from(bytes);
        self.client
            .put_object()
            .body(body)
            .bucket(&self.bucket)
            .key(self.get_stored_path(cid))
            .acl(ObjectCannedAcl::PublicRead)
            .send()
            .await?;
        Ok(())
    }

    pub async fn quarantine(&self, cid: Cid) -> Result<()> {
        Ok(self
            .move_object(MoveObject {
                from: self.get_stored_path(cid),
                to: self.get_quarantined_path(cid),
            })
            .await?)
    }

    pub async fn unquarantine(&self, cid: Cid) -> Result<()> {
        Ok(self
            .move_object(MoveObject {
                from: self.get_quarantined_path(cid),
                to: self.get_stored_path(cid),
            })
            .await?)
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
        Ok(self.get_object(cid).await?)
    }

    pub async fn delete(&self, cid: String) -> Result<()> {
        Ok(self
            .delete_key(self.get_stored_path(Cid::from_str(&cid)?))
            .await?)
    }

    pub async fn delete_many(&self, cids: Vec<Cid>) -> Result<()> {
        let keys: Vec<String> = cids
            .into_iter()
            .map(|cid| self.get_stored_path(cid))
            .collect();
        Ok(self.delete_many_keys(keys).await?)
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
        match res {
            Ok(_) => true,
            Err(_) => false,
        }
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
        self.client
            .copy_object()
            .bucket(&self.bucket)
            .copy_source(format!(
                "{0}/{1}/{2}",
                env_str("AWS_ENDPOINT_BUCKET").unwrap(),
                self.bucket,
                keys.from
            ))
            .key(keys.to)
            .acl(ObjectCannedAcl::PublicRead)
            .send()
            .await?;
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(keys.from)
            .send()
            .await?;
        Ok(())
    }
}
