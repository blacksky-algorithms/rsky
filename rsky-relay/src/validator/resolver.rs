use std::collections::VecDeque;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::time::{Duration, SystemTime, SystemTimeError, UNIX_EPOCH};

use futures::StreamExt;
use futures::stream::FuturesUnordered;
use hashbrown::HashSet;
use lru::LruCache;
use reqwest::Client;
use sled::Tree;
use thiserror::Error;
use tokio::time::timeout;
use zerocopy::big_endian::U64;
use zerocopy::{CastError, FromBytes, Immutable, IntoBytes, KnownLayout, SizeError, Unaligned};

use rsky_common::get_verification_material;
use rsky_identity::types::DidDocument;

use crate::types::DB;

const POLL_TIMEOUT: Duration = Duration::from_micros(1);
const REQ_TIMEOUT: Duration = Duration::from_secs(30);
const TCP_KEEPALIVE: Duration = Duration::from_secs(300);
const MAX_INFLIGHT: usize = 64;

const KEY_CAP: NonZeroUsize = unsafe { NonZeroUsize::new_unchecked(1 << 16) };
const KEY_TTL: Duration = Duration::from_secs(60 * 60 * 24);

const PLC_URL: &str = "https://plc.directory";
const DOC_PATH: &str = "/.well-known/did.json";

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
    cache: LruCache<String, [u8; 35]>,
    keys: Tree,
    queue: VecDeque<String>,
    client: Client,
    inflight: HashSet<String>,
    futures: FuturesUnordered<RequestFuture>,
}

impl Resolver {
    pub fn new() -> Result<Self, ResolverError> {
        let cache = LruCache::new(KEY_CAP);
        let keys = DB.open_tree("keys")?;
        let queue = VecDeque::new();
        let client = Client::builder()
            .timeout(REQ_TIMEOUT)
            .tcp_keepalive(Some(TCP_KEEPALIVE))
            .https_only(true)
            .build()?;
        let inflight = HashSet::new();
        let futures = FuturesUnordered::new();
        Ok(Self { cache, keys, queue, client, inflight, futures })
    }

    pub fn expire(&mut self, did: &str) -> Result<bool, ResolverError> {
        tracing::trace!("expiring did: {did}");
        self.cache.pop(did);
        Ok(self.keys.remove(did)?.is_some())
    }

    pub fn resolve(&mut self, did: &str) -> Result<Option<[u8; 35]>, ResolverError> {
        if let Some(key) = self.cache.get(did) {
            return Ok(Some(*key));
        }
        if let Some(bytes) = self.keys.get(did)? {
            let doc = Document::ref_from_bytes(&bytes)?;
            let time = UNIX_EPOCH + Duration::from_secs(doc.timestamp.get());
            if time.elapsed()? > KEY_TTL {
                self.keys.remove(did)?;
            } else {
                self.cache.put(did.to_owned(), doc.key);
                return Ok(Some(doc.key));
            }
        }
        self.request(did);
        Ok(None)
    }

    pub fn request(&mut self, did: &str) {
        fn send(this: &Resolver, did: &str) {
            tracing::trace!("fetching did: {did}");
            let req = if did.starts_with("did:plc:") {
                this.client.get(format!("{PLC_URL}/{did}"))
            } else if let Some(id) = did.strip_prefix("did:web:") {
                let Ok(hostname) = urlencoding::decode(id) else {
                    tracing::debug!("invalid did: {did}");
                    return;
                };
                this.client.get(format!("https://{hostname}/{DOC_PATH}"))
            } else {
                tracing::debug!("invalid did: {did}");
                return;
            };
            this.futures.push(Box::pin(async move { req.send().await?.json().await }));
        }

        if !self.inflight.insert(did.to_owned()) {
            return;
        }
        self.queue.push_back(did.to_owned());
        loop {
            if self.futures.len() == MAX_INFLIGHT {
                break;
            }
            if let Some(did) = self.queue.pop_front() {
                send(self, &did);
            } else {
                break;
            }
        }
    }

    pub async fn poll(&mut self) -> Result<Option<(&String, &[u8; 35])>, ResolverError> {
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
                                self.keys.insert(&response.id, doc.as_bytes())?;
                                self.cache.put(response.id, key);
                                return Ok(self.cache.peek_mru());
                            }
                            tracing::debug!("key len error");
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
