use std::io::BufRead;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::time::{Duration, Instant};

use bytes::{Buf, Bytes};
use chrono::{DateTime, Utc};
use futures::StreamExt;
use futures::stream::FuturesUnordered;
use hashbrown::HashSet;
use lru::LruCache;
use reqwest::Client;
use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;
use serde_json::value::RawValue;
use thiserror::Error;
use tokio::time::timeout;

use rsky_common::get_verification_material;
use rsky_identity::types::DidDocument;

const POLL_TIMEOUT: Duration = Duration::from_micros(1);
const REQ_TIMEOUT: Duration = Duration::from_secs(30);
const TCP_KEEPALIVE: Duration = Duration::from_secs(300);

const MAX_CACHED: usize = 1 << 16;
const EXPORT_INTERVAL: Duration = Duration::from_secs(60);

const PLC_URL: &str = "https://plc.directory/export?count=1000&after";
const DOC_PATH: &str = "/.well-known/did.json";

type RequestFuture = Pin<Box<dyn Future<Output = (Option<String>, reqwest::Result<Bytes>)> + Send>>;

#[derive(Debug, Error)]
pub enum ResolverError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("size error")]
    SizeError,
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
}

pub struct Resolver {
    cache: LruCache<String, (Option<Box<str>>, [u8; 35])>,
    conn: Connection,
    last: Instant,
    after: Option<String>,
    client: Client,
    inflight: HashSet<String>,
    futures: FuturesUnordered<RequestFuture>,
}

impl Resolver {
    pub fn new() -> Result<Self, ResolverError> {
        let cache = LruCache::new(unsafe { NonZeroUsize::new_unchecked(MAX_CACHED) });
        let conn = Connection::open_with_flags(
            "plc_directory.db",
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let now = Instant::now();
        let last = now.checked_sub(EXPORT_INTERVAL).unwrap_or(now);
        let after = conn.query_row(
            "SELECT created_at FROM plc_operations ORDER BY created_at DESC LIMIT 1",
            [],
            |row| Ok(Some(row.get("created_at")?)),
        )?;
        let client = Client::builder()
            .timeout(REQ_TIMEOUT)
            .tcp_keepalive(Some(TCP_KEEPALIVE))
            .https_only(true)
            .build()?;
        let inflight = HashSet::new();
        let futures = FuturesUnordered::new();
        Ok(Self { cache, conn, last, after, client, inflight, futures })
    }

    pub fn expire(&mut self, did: &str, time: DateTime<Utc>) {
        if let Some(after) = &self.after {
            if DateTime::parse_from_rfc3339(after).map_or(true, |after| after < time) {
                tracing::trace!("expiring did");
                self.cache.pop(did);
                self.request(did);
            }
        }
    }

    #[expect(clippy::type_complexity)]
    pub fn resolve(
        &mut self, did: &str,
    ) -> Result<Option<(Option<&str>, &[u8; 35])>, ResolverError> {
        // the identity might have expired, so check inflight dids first
        if self.inflight.contains(did) {
            return Ok(None);
        }
        // if let Some(_) = self.cache.get(did) doesn't work because of NLL
        if self.cache.get(did).is_some() || self.query_db(did)? {
            return Ok(self.cache.peek_mru().map(|(_, v)| (v.0.as_ref().map(AsRef::as_ref), &v.1)));
        }
        self.request(did);
        Ok(None)
    }

    pub fn query_db(&mut self, did: &str) -> Result<bool, ResolverError> {
        let mut stmt = self.conn.prepare_cached("SELECT * FROM plc_keys WHERE did = ?1")?;
        match stmt.query_row([did], |row| {
            // key can be null for legacy doc formats
            if let Some(key) = row.get_ref("key")?.as_str_or_null()? {
                match multibase::decode(&key[8..]) {
                    Ok((_, vec)) => match vec.try_into() {
                        Ok(key) => {
                            // endpoint can be null for legacy doc formats
                            let pds =
                                row.get_ref("endpoint")?.as_str_or_null()?.and_then(|endpoint| {
                                    Some(
                                        endpoint
                                            .strip_prefix("https://")?
                                            .trim_end_matches('/')
                                            .into(),
                                    )
                                });
                            return Ok(Some((pds, key)));
                        }
                        Err(_) => {
                            tracing::debug!(%key, "invalid key length");
                        }
                    },
                    Err(err) => {
                        tracing::debug!(%key, %err, "invalid key");
                    }
                }
            }
            Ok(None)
        }) {
            Ok(Some((pds, key))) => {
                self.cache.put(did.to_owned(), (pds, key));
                return Ok(true);
            }
            Ok(None) => {}
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                tracing::trace!("not found in db");
            }
            Err(err) => Err(err)?,
        }
        drop(stmt);
        Ok(false)
    }

    pub fn request(&mut self, did: &str) {
        self.inflight.insert(did.to_owned());
        if did.starts_with("did:plc:") {
            self.send_req(None);
        } else if let Some(id) = did.strip_prefix("did:web:") {
            let Ok(hostname) = urlencoding::decode(id) else {
                tracing::debug!("invalid did");
                return;
            };
            self.send_req(Some(&hostname));
        } else {
            tracing::debug!("invalid did");
        }
    }

    fn send_req(&mut self, hostname: Option<&str>) {
        let (req, hostname) = if let Some(hostname) = hostname {
            tracing::trace!("fetching did");
            (self.client.get(format!("https://{hostname}/{DOC_PATH}")), Some(hostname.to_owned()))
        } else if let Some(after) = self.after.take() {
            tracing::trace!(%after, "fetching after");
            self.last = Instant::now();
            (self.client.get(format!("{PLC_URL}={after}")), None)
        } else {
            return;
        };
        self.futures.push(Box::pin(async move {
            match req.send().await {
                Ok(req) => match req.bytes().await {
                    Ok(bytes) => (hostname, Ok(bytes)),
                    Err(err) => (hostname, Err(err)),
                },
                Err(err) => (hostname, Err(err)),
            }
        }));
    }

    pub async fn poll(&mut self) -> Result<Vec<String>, ResolverError> {
        if let Ok(Some((hostname, res))) = timeout(POLL_TIMEOUT, self.futures.next()).await {
            match res {
                Ok(bytes) => {
                    if hostname.is_some() {
                        if let Some((did, pds, key)) = parse_did_doc(&bytes) {
                            self.inflight.remove(&did);
                            self.cache.put(did.clone(), (pds, key));
                            return Ok(vec![did]);
                        }
                    } else {
                        let mut dids = Vec::new();
                        let mut count = 0;
                        let tx = self.conn.transaction()?;
                        let mut stmt = tx.prepare_cached("INSERT INTO plc_operations (did, cid, nullified, created_at, operation) VALUES (?1, ?2, ?3, ?4, ?5)")?;
                        for line in bytes.reader().lines() {
                            count += 1;
                            if let Some(doc) = parse_plc_doc(&line.unwrap_or_default()) {
                                stmt.execute((
                                    &doc.did,
                                    &doc.cid,
                                    &doc.nullified,
                                    &doc.created_at,
                                    doc.operation.get(),
                                ))?;
                                self.after = Some(doc.created_at);
                                if self.inflight.remove(&doc.did) {
                                    dids.push(doc.did);
                                }
                            }
                        }
                        drop(stmt);
                        tx.commit()?;
                        if count == 1000 {
                            self.send_req(None);
                        } else {
                            // no more plc operations, drain inflight dids
                            dids.extend(
                                self.inflight.extract_if(|did| did.starts_with("did:plc:")),
                            );
                        }
                        return Ok(dids);
                    }
                }
                Err(err) => {
                    tracing::debug!(%err, "fetch error");
                }
            }
        } else if self.last.elapsed() > EXPORT_INTERVAL {
            self.send_req(None);
        }
        Ok(Vec::new())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlcDocument<'a> {
    did: String,
    #[serde(borrow)]
    operation: &'a RawValue,
    cid: String,
    nullified: bool,
    created_at: String,
}

fn parse_plc_doc(input: &str) -> Option<PlcDocument<'_>> {
    match serde_json::from_slice::<PlcDocument<'_>>(input.as_bytes()) {
        Ok(doc) => {
            return Some(doc);
        }
        Err(err) => {
            tracing::debug!(%input, %err, "parse error");
        }
    }
    None
}

fn parse_did_doc(input: &Bytes) -> Option<(String, Option<Box<str>>, [u8; 35])> {
    match serde_json::from_slice::<DidDocument>(input) {
        Ok(doc) => match get_verification_material(&doc, "atproto") {
            Some(material) => {
                let key = material.public_key_multibase;
                match multibase::decode(&key) {
                    Ok((_, vec)) => {
                        if let Ok(key) = vec.try_into() {
                            let pds = doc.service.and_then(|services| {
                                Some(
                                    services
                                        .first()?
                                        .service_endpoint
                                        .strip_prefix("https://")?
                                        .trim_end_matches('/')
                                        .into(),
                                )
                            });
                            return Some((doc.id, pds, key));
                        }
                        tracing::debug!(%key, "invalid key length");
                    }
                    Err(err) => {
                        tracing::debug!(%key, %err, "invalid key");
                    }
                }
            }
            None => {
                tracing::debug!(?doc, "no valid key found");
            }
        },
        Err(err) => {
            tracing::debug!(?input, %err, "parse error");
        }
    }
    None
}
