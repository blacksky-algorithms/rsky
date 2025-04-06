use std::pin::Pin;
use std::time::{Duration, SystemTime, SystemTimeError, UNIX_EPOCH};

use futures::StreamExt;
use futures::stream::FuturesUnordered;
use hashbrown::HashSet;
use reqwest::Client;
use sled::Tree;
use thiserror::Error;
use tokio::time::timeout;
use zerocopy::big_endian::U64;
use zerocopy::{CastError, FromBytes, Immutable, IntoBytes, KnownLayout, SizeError, Unaligned};

use rsky_common::get_verification_material;
use rsky_identity::types::DidDocument;

use crate::types::DB;

const POLL_TIMEOUT: Duration = Duration::from_micros(100);
const REQ_TIMEOUT: Duration = Duration::from_secs(5);
const TCP_KEEPALIVE: Duration = Duration::from_secs(300);
const KEY_TTL: Duration = Duration::from_secs(60 * 60 * 24);

const PLC_URL: &str = "https://plc.directory";

type RequestFuture = Pin<Box<dyn Future<Output = reqwest::Result<DidDocument>> + Send>>;

#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C, packed)]
pub struct Document {
    timestamp: U64,
    key: [u8; 35],
}

#[derive(Debug, Error)]
pub enum ResolverError {
    #[error("sled error: {0}")]
    Sled(#[from] sled::Error),
    #[error("time error: {0}")]
    Time(#[from] SystemTimeError),
    #[error("size error")]
    SizeError,
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
}

impl<T: KnownLayout + Immutable + Unaligned + ?Sized> From<CastError<&[u8], T>> for ResolverError {
    fn from(v: CastError<&[u8], T>) -> Self {
        let _: SizeError<&[u8], T> = v.into();
        Self::SizeError
    }
}

pub struct Resolver {
    cache: Tree,
    client: Client,
    inflight: HashSet<String>,
    futures: FuturesUnordered<RequestFuture>,
}

impl Resolver {
    pub async fn new() -> Result<Self, ResolverError> {
        let cache = DB.open_tree("keys")?;
        let client = Client::builder()
            .timeout(REQ_TIMEOUT)
            .tcp_keepalive(Some(TCP_KEEPALIVE))
            .https_only(true)
            .build()?;
        let inflight = HashSet::new();
        let pending = FuturesUnordered::new();
        Ok(Self { client, cache, inflight, futures: pending })
    }

    pub async fn shutdown(&mut self) -> Result<(), ResolverError> {
        self.cache.flush_async().await?;
        Ok(())
    }

    pub fn expire(&mut self, did: &str) -> Result<bool, ResolverError> {
        tracing::debug!("expiring did: {did}");
        Ok(self.cache.remove(did)?.is_some())
    }

    pub fn resolve(&mut self, did: &str) -> Result<Option<[u8; 35]>, ResolverError> {
        if let Some(bytes) = self.cache.get(did)? {
            let doc = Document::ref_from_bytes(&bytes)?;
            let time = SystemTime::UNIX_EPOCH + Duration::from_secs(doc.timestamp.get());
            if time.elapsed()? > KEY_TTL {
                self.cache.remove(did)?;
            } else {
                return Ok(Some(doc.key));
            }
        }
        self.request(did);
        Ok(None)
    }

    pub fn request(&mut self, did: &str) {
        if self.inflight.contains(did) {
            return;
        }
        self.inflight.insert(did.to_owned());
        let req = self.client.get(&format!("{PLC_URL}/{did}"));
        self.futures.push(Box::pin(async move { req.send().await?.json().await }));
    }

    pub async fn poll(&mut self) -> Result<Option<(String, [u8; 35])>, ResolverError> {
        if let Ok(Some(res)) = timeout(POLL_TIMEOUT, self.futures.next()).await {
            match res {
                Ok(response) => {
                    self.inflight.remove(&response.id);
                    if let Some(material) = get_verification_material(&response, "atproto") {
                        if let Ok((_, key)) = multibase::decode(&material.public_key_multibase) {
                            if let Ok(key) = key.try_into() {
                                let doc = Document {
                                    timestamp: SystemTime::now()
                                        .duration_since(UNIX_EPOCH)?
                                        .as_secs()
                                        .into(),
                                    key,
                                };
                                self.cache.insert(&response.id, doc.as_bytes())?;
                                return Ok(Some((response.id, key)));
                            } else {
                                tracing::debug!("key len error");
                            }
                        } else {
                            tracing::debug!(
                                "multibase decode error: {}",
                                material.public_key_multibase
                            );
                        }
                    } else {
                        tracing::debug!("no valid key found: {response:?}");
                    }
                }
                Err(err) => {
                    tracing::debug!("fetch error: {err:?}");
                }
            }
        }
        Ok(None)
    }
}
