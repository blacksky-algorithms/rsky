use crate::common::{DAY, HOUR};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::future::Future;
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
    pub did_cache: Option<DidCache>,
    pub backup_nameservers: Option<Vec<String>>,
}

pub struct HandleResolverOpts {
    pub timeout: Option<Duration>,
    pub backup_nameservers: Option<Vec<String>>,
}

pub struct DidResolverOpts {
    pub timeout: Option<Duration>,
    pub plc_url: Option<String>,
    pub did_cache: DidCache,
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

/// MemoryCache implementation of DidCache
#[derive(Clone, Debug)]
pub struct DidCache {
    pub stale_ttl: Duration,
    pub max_ttl: Duration,
    pub cache: BTreeMap<String, CacheVal>,
}

impl DidCache {
    pub fn new(stale_ttl: Option<Duration>, max_ttl: Option<Duration>) -> Self {
        Self {
            stale_ttl: stale_ttl.unwrap_or_else(|| Duration::new(HOUR as u64, 0)),
            max_ttl: max_ttl.unwrap_or_else(|| Duration::new(DAY as u64, 0)),
            cache: BTreeMap::new(),
        }
    }

    pub async fn cache_did(&mut self, did: String, doc: DidDocument) -> Result<()> {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("timestamp in micros since UNIX epoch")
            .as_micros();
        self.cache.insert(
            did,
            CacheVal {
                doc,
                updated_at: now,
            },
        );
        Ok(())
    }

    pub async fn refresh_cache<Fut>(&mut self, did: String, get_doc: impl Fn() -> Fut) -> Result<()>
    where
        Fut: Future<Output = Result<Option<DidDocument>>>,
    {
        match get_doc().await? {
            None => Ok(()),
            Some(doc) => self.cache_did(did, doc).await,
        }
    }

    pub fn check_cache(&self, did: String) -> Result<Option<CacheResult>> {
        match self.cache.get(&did) {
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

    pub fn clear_entry(&mut self, did: String) -> Result<()> {
        self.cache.remove(&did);
        Ok(())
    }

    pub fn clear(&mut self) -> Result<()> {
        Ok(self.cache.clear())
    }
}
