#[cfg(feature = "aws-s3")]
use crate::actor_store::aws::s3::S3BlobStore;
use crate::config::BlobstoreConfig;
#[cfg(feature = "aws-s3")]
use crate::config::S3Config;
use anyhow::Result;
#[cfg(feature = "aws-s3")]
use aws_config::SdkConfig;
use bytes::Bytes;
use futures::future::BoxFuture;
use futures::stream::StreamExt;
use lexicon_cid::Cid;
#[cfg(feature = "disk")]
use std::path::Path;
use std::sync::Arc;

#[cfg(feature = "disk")]
pub use rsky_blobstore::backends::disk::DiskBlobStore;
#[cfg(feature = "opendal")]
pub use rsky_blobstore::backends::opendal::OpendalBlobStore;
pub use rsky_blobstore::{
    unavailable, BlobNotFoundError, BlobStore, BlobUnavailableError, BoxedBlobStream, DeleteError,
    MemoryBlobStore, ObjectKind,
};

/// Builds the configured blobstore implementation for a given actor.
pub struct BlobstoreFactory {
    cfg: BlobstoreConfig,
    #[cfg(feature = "aws-s3")]
    aws_cfg: SdkConfig,
    attempts: Option<crate::blob_attempts::AttemptJournal>,
    generations: Option<crate::blob_generations::Generations>,
}

impl BlobstoreFactory {
    #[cfg(feature = "aws-s3")]
    pub fn new(cfg: BlobstoreConfig, aws_cfg: SdkConfig) -> Self {
        BlobstoreFactory {
            cfg,
            aws_cfg,
            attempts: None,
            generations: None,
        }
    }

    #[cfg(not(feature = "aws-s3"))]
    pub fn new(cfg: BlobstoreConfig) -> Self {
        BlobstoreFactory {
            cfg,
            attempts: None,
            generations: None,
        }
    }

    /// Builds the factory for the configured store, deriving the S3 client
    /// settings from the `PDS_BLOBSTORE_S3_*` configuration over whatever
    /// the AWS environment provides.
    pub async fn from_config(cfg: BlobstoreConfig) -> Self {
        #[cfg(feature = "aws-s3")]
        {
            let s3 = match &cfg {
                BlobstoreConfig::S3(s3) => s3.clone(),
                _ => S3Config::default(),
            };
            Self::new(cfg, sdk_config_for(&s3).await)
        }
        #[cfg(not(feature = "aws-s3"))]
        {
            Self::new(cfg)
        }
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

    #[cfg(feature = "aws-s3")]
    pub fn clone_config(&self) -> (BlobstoreConfig, SdkConfig) {
        (self.cfg.clone(), self.aws_cfg.clone())
    }

    #[cfg(not(feature = "aws-s3"))]
    pub fn clone_config(&self) -> BlobstoreConfig {
        self.cfg.clone()
    }

    /// A factory over the same store and journal.
    #[cfg(feature = "aws-s3")]
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

    /// A factory over the same store and journal.
    #[cfg(not(feature = "aws-s3"))]
    pub fn from_attempts(
        cfg: BlobstoreConfig,
        attempts: Option<crate::blob_attempts::AttemptJournal>,
    ) -> Self {
        BlobstoreFactory {
            cfg,
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
            #[cfg(feature = "disk")]
            BlobstoreConfig::Disk {
                location,
                tmp_location,
            } => Arc::new(DiskBlobStore::new(
                did.clone(),
                Path::new(location),
                tmp_location.as_deref().map(Path::new),
                None,
            )),
            #[cfg(feature = "aws-s3")]
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
            #[cfg(feature = "opendal")]
            BlobstoreConfig::Opendal {
                operator_kind,
                bucket,
            } => {
                let operator = rsky_blobstore::backends::opendal::build_operator(
                    operator_kind,
                    bucket,
                )
                .expect("opendal blobstore construction failed");
                Arc::new(OpendalBlobStore::new(operator, did.clone()))
            }
            #[cfg(any(not(feature = "disk"), not(feature = "aws-s3")))]
            other => disabled_backend(other),
        };
        self.generation_aware(did, inner)
    }
}

/// Refuses a configuration whose backend this build left out. Only compiled
/// when a backend is missing, so a build with every backend never carries the
/// unreachable arm.
#[cfg(any(not(feature = "disk"), not(feature = "aws-s3")))]
fn disabled_backend(cfg: &BlobstoreConfig) -> Arc<dyn BlobStore> {
    panic!(
        "blobstore backend for {cfg:?} is not enabled in this build; enable `disk`, `aws-s3`, or `opendal`"
    )
}

/// The SDK configuration for an S3 store: the environment's defaults with
/// the reference PDS settings layered on top.
#[cfg(feature = "aws-s3")]
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

/// A factory over the given configuration for tests that never reach a
/// production backend's credentials.
#[cfg(test)]
pub(crate) fn test_factory(cfg: BlobstoreConfig) -> BlobstoreFactory {
    #[cfg(feature = "aws-s3")]
    {
        BlobstoreFactory::new(cfg, SdkConfig::builder().build())
    }
    #[cfg(not(feature = "aws-s3"))]
    {
        BlobstoreFactory::new(cfg)
    }
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
    fn get_stream(&self, cid: Cid) -> BoxFuture<'_, Result<BoxedBlobStream>> {
        Box::pin(async move {
            let generation = self.generation(cid).await?;
            let key = self.key(cid, generation);
            self.ensure_available(&key).await?;
            if generation == 0 {
                return self.inner.get_stream(cid).await;
            }
            let bytes = self.inner.get_object(key).await?;
            Ok(futures::stream::once(async move { Ok(Bytes::from(bytes)) }).boxed())
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

    fn disk_config(dir: &std::path::Path) -> BlobstoreConfig {
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
        let factory = test_factory(disk_config(dir.path())).with_attempts(journal);
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
        let factory = test_factory(disk_config(dir.path()))
            .with_attempts(journal.clone())
            .with_generations(registry.clone());
        let store = factory.generation_aware(did.to_owned(), inner.clone());
        let bytes = b"generation".to_vec();
        let cid = cid_for(&bytes);

        // generation zero is the inner store's own layout
        let key = store.put_temp(bytes.clone()).await.unwrap();
        assert!(store.has_temp(key.clone()).await.unwrap());
        store.make_permanent(key, cid).await.unwrap();
        assert!(store.has_stored(cid).await.unwrap());
        assert_eq!(store.get_bytes(cid).await.unwrap(), bytes);
        assert_eq!(
            streamed_bytes(store.get_stream(cid).await.unwrap()).await,
            bytes
        );
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
        assert_eq!(
            streamed_bytes(store.get_stream(cid).await.unwrap()).await,
            bytes
        );
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

    #[cfg(feature = "disk")]
    #[tokio::test]
    async fn factory_builds_disk_store_from_disk_config() {
        let dir = tempfile::tempdir().unwrap();
        let location = dir.path().join("blobs");
        let factory = test_factory(BlobstoreConfig::Disk {
            location: location.to_string_lossy().to_string(),
            tmp_location: None,
        });
        let store = factory.blobstore("did:example:alice".to_owned());
        let bytes = b"factory blob".to_vec();
        let cid = cid_for(&bytes);
        store.put_permanent(cid, bytes.clone()).await.unwrap();
        // bytes landed on disk under {location}/{did}/{cid}
        let stored_path = location.join("did:example:alice").join(cid.to_string());
        assert_eq!(std::fs::read(stored_path).unwrap(), bytes);
        assert!(store.delete_all().is_some());
    }

    #[cfg(feature = "disk")]
    #[tokio::test]
    async fn factory_builds_disk_store_with_custom_tmp_location() {
        let dir = tempfile::tempdir().unwrap();
        let location = dir.path().join("blobs");
        let tmp_location = dir.path().join("tmp");
        let factory = test_factory(BlobstoreConfig::Disk {
            location: location.to_string_lossy().to_string(),
            tmp_location: Some(tmp_location.to_string_lossy().to_string()),
        });
        let store = factory.blobstore("did:example:alice".to_owned());
        let key = store.put_temp(b"temp blob".to_vec()).await.unwrap();
        assert!(tmp_location.join("did:example:alice").join(&key).is_file());
    }

    #[cfg(feature = "aws-s3")]
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
    }

    #[cfg(all(feature = "aws-s3", feature = "disk"))]
    #[tokio::test]
    async fn factory_builds_the_configured_backend() {
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

    #[cfg(feature = "aws-s3")]
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
