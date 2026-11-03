use async_trait::async_trait;
use aws_credential_types::Credentials;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client;
use sha2::{Digest, Sha256};

use crate::error::{DaemonError, Result};

#[derive(Clone)]
pub struct DeliveryBucketConfig {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub access_key_id: String,
    pub secret_access_key: String,
}

#[async_trait]
pub trait DeliveryStore: Send + Sync {
    async fn put(&self, key: &str, content_type: &str, bytes: Vec<u8>) -> Result<()>;
    async fn delete(&self, key: &str) -> Result<()>;
}

pub struct S3DeliveryStore {
    client: Client,
    bucket: String,
}

impl S3DeliveryStore {
    pub async fn new(config: DeliveryBucketConfig) -> Self {
        let sdk_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(aws_sdk_s3::config::Region::new(config.region))
            .endpoint_url(config.endpoint)
            .credentials_provider(Credentials::new(
                config.access_key_id,
                config.secret_access_key,
                None,
                None,
                "rsky-daemon-delivery",
            ))
            .load()
            .await;
        Self {
            client: Client::new(&sdk_config),
            bucket: config.bucket,
        }
    }
}

#[async_trait]
impl DeliveryStore for S3DeliveryStore {
    async fn put(&self, key: &str, content_type: &str, bytes: Vec<u8>) -> Result<()> {
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .content_type(content_type)
            .body(ByteStream::from(bytes))
            .send()
            .await
            .map_err(|error| DaemonError::Xrpc(format!("delivery PUT: {error}")))?;
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<()> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|error| DaemonError::Xrpc(format!("delivery DELETE: {error}")))?;
        Ok(())
    }
}

pub fn object_key(space: &str, did: &str, cid: &str) -> Result<String> {
    let without_scheme = space
        .strip_prefix("at://")
        .ok_or_else(|| DaemonError::Xrpc("invalid space URI".to_string()))?;
    let mut parts = without_scheme.split('/');
    let authority = parts
        .next()
        .filter(|value| value.starts_with("did:"))
        .ok_or_else(|| DaemonError::Xrpc("space authority must be a DID".to_string()))?;
    if parts.next() != Some("space") {
        return Err(DaemonError::Xrpc("invalid space URI".to_string()));
    }
    let _space_type = parts
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| DaemonError::Xrpc("invalid space URI".to_string()))?;
    let skey = parts
        .next()
        .filter(|value| !value.is_empty() && parts.next().is_none())
        .ok_or_else(|| DaemonError::Xrpc("invalid space URI".to_string()))?;
    Ok(format!("spaces/{authority}/{skey}/{did}/{cid}"))
}

pub fn verify_cid(cid: &str, bytes: &[u8]) -> Result<()> {
    let cid = cid
        .parse::<lexicon_cid::Cid>()
        .map_err(|_| DaemonError::Xrpc("invalid blob CID".to_string()))?;
    if cid.hash().code() != 0x12 {
        return Err(DaemonError::Xrpc("unsupported blob CID hash".to_string()));
    }
    let digest = Sha256::digest(bytes);
    if cid.hash().digest() != digest.as_slice() {
        return Err(DaemonError::Xrpc(
            "blob CID does not match body".to_string(),
        ));
    }
    Ok(())
}

pub fn allowed_image_mime(content_type: &str) -> bool {
    matches!(
        content_type.split(';').next().unwrap_or("").trim(),
        "image/jpeg" | "image/png" | "image/gif" | "image/webp" | "image/avif"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use lexicon_cid::multihash::Multihash;

    fn cid_for(bytes: &[u8]) -> String {
        let digest = Sha256::digest(bytes);
        lexicon_cid::Cid::new_v1(0x55, Multihash::<64>::wrap(0x12, &digest).unwrap()).to_string()
    }

    #[test]
    fn verifies_the_exact_cid_bytes() {
        let bytes = b"verified image";
        let cid = cid_for(bytes);
        verify_cid(&cid, bytes).unwrap();
        assert!(verify_cid(&cid, b"different image").is_err());
    }

    #[test]
    fn builds_a_space_scoped_key() {
        assert_eq!(
            object_key(
                "at://did:plc:authority/space/community.blacksky.feed/main",
                "did:plc:author",
                "bafyblob",
            )
            .unwrap(),
            "spaces/did:plc:authority/main/did:plc:author/bafyblob",
        );
        assert!(object_key(
            "at://did:plc:authority/not-space/type/key",
            "did:plc:author",
            "bafy"
        )
        .is_err());
    }

    #[test]
    fn allows_only_images() {
        assert!(allowed_image_mime("image/png; charset=binary"));
        assert!(!allowed_image_mime("video/mp4"));
    }
}
