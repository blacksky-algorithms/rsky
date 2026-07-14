use crate::common::{DAY, HOUR};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerificationMethod {
    pub id: String,
    #[serde(rename = "type")]
    pub r#type: String,
    pub controller: String,
    #[serde(rename = "publicKeyMultibase")]
    pub public_key_multibase: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Service {
    pub id: String,
    #[serde(rename = "type")]
    pub r#type: String,
    #[serde(rename = "serviceEndpoint")]
    pub service_endpoint: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DidDocument {
    #[serde(rename = "@context")]
    pub context: Option<Vec<String>>,
    pub id: String,
    #[serde(rename = "alsoKnownAs")]
    pub also_known_as: Option<Vec<String>>,
    #[serde(rename = "verificationMethod")]
    pub verification_method: Option<Vec<VerificationMethod>>,
    pub service: Option<Vec<Service>>,
}

pub struct IdentityResolverOpts {
    pub timeout: Option<Duration>,
    pub plc_url: Option<String>,
    pub did_cache: Option<Arc<dyn DidCache>>,
    pub backup_nameservers: Option<Vec<String>>,
}

pub struct HandleResolverOpts {
    pub timeout: Option<Duration>,
    pub backup_nameservers: Option<Vec<String>>,
}

pub struct DidResolverOpts {
    pub timeout: Option<Duration>,
    pub plc_url: Option<String>,
    pub did_cache: Arc<dyn DidCache>,
}

pub struct AtprotoData {
    pub did: String,
    pub signing_key: String,
    pub handle: String,
    pub pds: String,
}

pub struct CacheResult {
    pub did: String,
    pub doc: DidDocument,
    pub updated_at: u128,
    pub stale: bool,
    pub expired: bool,
}

#[derive(Clone, Debug)]
pub struct CacheVal {
    pub doc: DidDocument,
    pub updated_at: u128,
}

/// A boxed factory producing the freshly-resolved document for a did,
/// used by `DidCache::refresh_cache` implementations.
pub type GetDocFn =
    Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = Result<Option<DidDocument>>> + Send>> + Send>;

/// Pluggable did document cache used by `DidResolver`.
#[async_trait::async_trait]
pub trait DidCache: Send + Sync + Debug {
    async fn cache_did(&self, did: String, doc: DidDocument) -> Result<()>;
    async fn refresh_cache(&self, did: String, get_doc: GetDocFn) -> Result<()>;
    async fn check_cache(&self, did: String) -> Result<Option<CacheResult>>;
    async fn clear_entry(&self, did: String) -> Result<()>;
    async fn clear(&self) -> Result<()>;
}

/// In-memory implementation of DidCache
#[derive(Debug)]
pub struct MemoryCache {
    pub stale_ttl: Duration,
    pub max_ttl: Duration,
    pub cache: RwLock<BTreeMap<String, CacheVal>>,
}

impl MemoryCache {
    pub fn new(stale_ttl: Option<Duration>, max_ttl: Option<Duration>) -> Self {
        Self {
            stale_ttl: stale_ttl.unwrap_or_else(|| Duration::new(HOUR as u64, 0)),
            max_ttl: max_ttl.unwrap_or_else(|| Duration::new(DAY as u64, 0)),
            cache: RwLock::new(BTreeMap::new()),
        }
    }
}

#[async_trait::async_trait]
impl DidCache for MemoryCache {
    async fn cache_did(&self, did: String, doc: DidDocument) -> Result<()> {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("timestamp in micros since UNIX epoch")
            .as_micros();
        self.cache.write().expect("did cache poisoned").insert(
            did,
            CacheVal {
                doc,
                updated_at: now,
            },
        );
        Ok(())
    }

    async fn refresh_cache(&self, did: String, get_doc: GetDocFn) -> Result<()> {
        match get_doc().await? {
            None => Ok(()),
            Some(doc) => self.cache_did(did, doc).await,
        }
    }

    async fn check_cache(&self, did: String) -> Result<Option<CacheResult>> {
        match self.cache.read().expect("did cache poisoned").get(&did) {
            None => Ok(None),
            Some(val) => {
                let now = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .expect("timestamp in micros since UNIX epoch")
                    .as_micros();
                let expired = now > val.updated_at + self.max_ttl.as_micros();
                let stale = now > val.updated_at + self.stale_ttl.as_micros();
                let CacheVal { doc, updated_at } = val.clone();
                Ok(Some(CacheResult {
                    did,
                    doc,
                    updated_at,
                    stale,
                    expired,
                }))
            }
        }
    }

    async fn clear_entry(&self, did: String) -> Result<()> {
        self.cache.write().expect("did cache poisoned").remove(&did);
        Ok(())
    }

    async fn clear(&self) -> Result<()> {
        self.cache.write().expect("did cache poisoned").clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(did: &str) -> DidDocument {
        DidDocument {
            context: None,
            id: did.to_owned(),
            also_known_as: None,
            verification_method: None,
            service: None,
        }
    }

    #[tokio::test]
    async fn memory_cache_caches_and_checks() {
        let cache = MemoryCache::new(None, None);
        assert!(cache
            .check_cache("did:example:alice".to_owned())
            .await
            .unwrap()
            .is_none());
        cache
            .cache_did("did:example:alice".to_owned(), doc("did:example:alice"))
            .await
            .unwrap();
        let result = cache
            .check_cache("did:example:alice".to_owned())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.doc.id, "did:example:alice");
        assert!(!result.stale);
        assert!(!result.expired);
    }

    #[tokio::test]
    async fn memory_cache_reports_stale_and_expired() {
        let cache = MemoryCache::new(Some(Duration::from_secs(0)), Some(Duration::from_secs(0)));
        cache
            .cache_did("did:example:bob".to_owned(), doc("did:example:bob"))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
        let result = cache
            .check_cache("did:example:bob".to_owned())
            .await
            .unwrap()
            .unwrap();
        assert!(result.stale);
        assert!(result.expired);
    }

    #[tokio::test]
    async fn memory_cache_refreshes_and_clears() {
        let cache = MemoryCache::new(None, None);
        cache
            .refresh_cache(
                "did:example:carol".to_owned(),
                Box::new(|| Box::pin(async { Ok(Some(doc("did:example:carol"))) })),
            )
            .await
            .unwrap();
        assert!(cache
            .check_cache("did:example:carol".to_owned())
            .await
            .unwrap()
            .is_some());

        // a doc that no longer resolves leaves the cache untouched
        cache
            .refresh_cache(
                "did:example:missing".to_owned(),
                Box::new(|| Box::pin(async { Ok(None) })),
            )
            .await
            .unwrap();
        assert!(cache
            .check_cache("did:example:missing".to_owned())
            .await
            .unwrap()
            .is_none());

        cache
            .cache_did("did:example:dave".to_owned(), doc("did:example:dave"))
            .await
            .unwrap();
        cache
            .clear_entry("did:example:carol".to_owned())
            .await
            .unwrap();
        assert!(cache
            .check_cache("did:example:carol".to_owned())
            .await
            .unwrap()
            .is_none());
        cache.clear().await.unwrap();
        assert!(cache
            .check_cache("did:example:dave".to_owned())
            .await
            .unwrap()
            .is_none());
    }
}
