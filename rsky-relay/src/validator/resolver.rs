use std::io::BufRead;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::time::{Duration, Instant};

use bytes::{Buf, Bytes};
use chrono::{DateTime, Utc};
use futures::StreamExt;
use futures::stream::FuturesUnordered;
use hashbrown::{HashMap, HashSet};
use lru::LruCache;
use reqwest::Client;
use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;
use serde_json::value::RawValue;
use thiserror::Error;
use tokio::time::timeout;

use rsky_identity::types::DidDocument;

use crate::config::{CAPACITY_CACHE, DO_PLC_EXPORT, PLC_EXPORT_INTERVAL};
use crate::metrics;
use crate::validator::event::{DidEndpoint, DidKey};

/// Hot-path interface used by the validator. Returns owned values so the resolver isn't
/// borrowed across the rest of the validation pipeline. Implemented by the production
/// `Resolver` and by test fakes.
pub trait IdentityResolver: Send {
    fn expire(&mut self, did: &str, time: DateTime<Utc>);
    fn resolve_owned(
        &mut self, did: &str,
    ) -> Result<Option<(Option<String>, DidKey)>, ResolverError>;
    fn request_direct(&mut self, did: &str);
    fn poll(
        &mut self,
    ) -> impl std::future::Future<Output = Result<Vec<String>, ResolverError>> + Send;
}

const POLL_TIMEOUT: Duration = Duration::from_micros(10);
// Completed fetches handled per validator iteration; one per iteration let
// thousands of pending DIDs back up behind a busy ring.
const POLL_BATCH: usize = 256;
const REQ_TIMEOUT: Duration = Duration::from_secs(30);
const TCP_KEEPALIVE: Duration = Duration::from_secs(300);
// Hard ceiling on concurrent DID fetches: event floods from never-before-seen
// DIDs must not grow the future set without bound. Skipped DIDs retry on
// their next event once capacity frees.
const MAX_INFLIGHT_FETCHES: usize = 4096;
// DIDs parked on the export stream; older waiters are handed back unresolved
// so the validator can decide, instead of accumulating forever.
const MAX_EXPORT_WAITERS: usize = CAPACITY_CACHE;
const EXPORT_WAITER_MAX_AGE: Duration = Duration::from_secs(2 * 60);
// Direct document fetches against the PLC directory: sustained rate and burst.
const DIRECT_FETCH_RATE: f64 = 50.0;
const DIRECT_FETCH_BURST: f64 = 50.0;

/// The PLC directory to resolve against; `RELAY_PLC_URL` overrides the
/// public directory for a private deployment.
static PLC_URL: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    std::env::var("RELAY_PLC_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "https://plc.directory".to_owned())
});
const PLC_EXPORT: &str = "export?count=1000&after";
const DOC_PATH: &str = ".well-known/did.json";

type RequestFuture = Pin<Box<dyn Future<Output = (Query, Option<Bytes>)> + Send>>;

#[derive(Debug, Clone)]
enum Query {
    Did(String),
    Export(String),
}

/// Token bucket for direct fetches: `rate` tokens per second up to `burst`.
#[derive(Debug)]
struct TokenBucket {
    tokens: f64,
    rate: f64,
    burst: f64,
    last: Instant,
}

impl TokenBucket {
    fn new(rate: f64, burst: f64) -> Self {
        Self { tokens: burst, rate, burst, last: Instant::now() }
    }

    fn take(&mut self) -> bool {
        let now = Instant::now();
        self.tokens = now
            .duration_since(self.last)
            .as_secs_f64()
            .mul_add(self.rate, self.tokens)
            .min(self.burst);
        self.last = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

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
    cache: LruCache<String, (DidEndpoint, DidKey)>,
    conn: Connection,
    last: Instant,
    after: Option<String>,
    client: Client,
    /// DIDs waiting for their op to show up on the export stream.
    export_waiters: HashMap<String, Instant>,
    /// DIDs with a document fetch in flight.
    direct_inflight: HashSet<String>,
    exporting: bool,
    bucket: TokenBucket,
    /// HTTP runs on the shared multi-thread runtime when one exists, so a busy
    /// validator thread never starves its own DNS and TLS.
    handle: Option<tokio::runtime::Handle>,
    futures: FuturesUnordered<RequestFuture>,
}

impl Resolver {
    pub fn new() -> Result<Self, ResolverError> {
        Self::with_db_path("plc_directory.db")
    }

    fn with_db_path(db_path: &str) -> Result<Self, ResolverError> {
        #[expect(clippy::unwrap_used)]
        let cache = LruCache::new(NonZeroUsize::new(CAPACITY_CACHE).unwrap());
        let flag = if *DO_PLC_EXPORT {
            OpenFlags::SQLITE_OPEN_READ_WRITE
        } else {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        };
        let conn = Connection::open_with_flags(db_path, flag | OpenFlags::SQLITE_OPEN_CREATE)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA wal_autocheckpoint = 1000;")?;
        if *DO_PLC_EXPORT {
            match conn.execute("PRAGMA secure_delete = OFF", []) {
                Ok(_) | Err(rusqlite::Error::ExecuteReturnedResults) => {}
                Err(err) => Err(err)?,
            }
            conn.execute("PRAGMA synchronous = NORMAL", [])?;
            conn.execute("PRAGMA incremental_vacuum", [])?;
            conn.execute("PRAGMA optimize = 0x10002", [])?;
        }
        let now = Instant::now();
        let last = now.checked_sub(PLC_EXPORT_INTERVAL).unwrap_or(now);
        let after = conn.query_one(
            "SELECT created_at FROM plc_operations ORDER BY created_at DESC LIMIT 1",
            [],
            |row| Ok(Some(row.get("created_at")?)),
        )?;
        let client = Client::builder()
            .user_agent("rsky-relay")
            .timeout(REQ_TIMEOUT)
            .tcp_keepalive(Some(TCP_KEEPALIVE))
            // a private directory may be reached without TLS; the public one never is
            .https_only(!PLC_URL.starts_with("http://"))
            .build()?;
        let futures = FuturesUnordered::new();
        Ok(Self {
            cache,
            conn,
            last,
            after,
            client,
            export_waiters: HashMap::new(),
            direct_inflight: HashSet::new(),
            exporting: false,
            bucket: TokenBucket::new(DIRECT_FETCH_RATE, DIRECT_FETCH_BURST),
            handle: tokio::runtime::Handle::try_current().ok(),
            futures,
        })
    }

    #[cfg(all(test, not(feature = "labeler")))]
    pub(crate) fn set_direct_rate(&mut self, rate: f64, burst: f64) {
        self.bucket = TokenBucket::new(rate, burst);
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

    pub fn resolve(&mut self, did: &str) -> Result<Option<(Option<&str>, &DidKey)>, ResolverError> {
        // the identity might have expired, so check inflight dids first
        if self.direct_inflight.contains(did) || self.export_waiters.contains_key(did) {
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
        match stmt.query_one([did], |row| {
            let endpoint =
                if cfg!(feature = "labeler") { "labeler_endpoint" } else { "pds_endpoint" };
            let key = if cfg!(feature = "labeler") { "labeler_key" } else { "pds_key" };
            let endpoint = row.get_ref(endpoint)?.as_str_or_null()?;
            let key = row.get_ref(key)?.as_str_or_null()?;
            Ok(parse_key_endpoint(endpoint, key))
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

    /// Park a `did:plc` on the export stream (its next op resolves it); any other
    /// DID goes straight to a document fetch.
    pub fn request(&mut self, did: &str) {
        if did.starts_with("did:plc:") && *DO_PLC_EXPORT {
            if self.direct_inflight.contains(did) || self.export_waiters.contains_key(did) {
                return;
            }
            if self.export_waiters.len() >= MAX_EXPORT_WAITERS {
                return;
            }
            self.export_waiters.insert(did.to_owned(), Instant::now());
            metrics::record_resolver_inflight("export_waiters", self.export_waiters.len());
            self.send_req(None, None, None);
            return;
        }
        self.request_direct(did);
    }

    /// Fetch the DID document now, even for a DID parked on the export stream:
    /// a waiter whose op never arrives would otherwise stay unresolved forever.
    pub fn request_direct(&mut self, did: &str) {
        // One fetch per DID at a time, bounded overall: repeat events for a
        // pending DID must not stack additional futures.
        if self.direct_inflight.contains(did) || self.direct_inflight.len() >= MAX_INFLIGHT_FETCHES
        {
            return;
        }
        if let Some(plc) = did.strip_prefix("did:plc:") {
            if !self.bucket.take() {
                metrics::record_resolver_fetch("rate_limited");
                return;
            }
            self.direct_inflight.insert(did.to_owned());
            self.send_req(Some(did), None, Some(plc));
        } else if let Some(web) = did.strip_prefix("did:web:") {
            let Ok(web) = urlencoding::decode(web) else {
                tracing::debug!(%did, "invalid did");
                return;
            };
            if !self.bucket.take() {
                metrics::record_resolver_fetch("rate_limited");
                return;
            }
            self.direct_inflight.insert(did.to_owned());
            self.send_req(Some(did), Some(&web), None);
        } else {
            tracing::debug!(%did, "invalid did");
            return;
        }
        metrics::record_resolver_inflight("direct", self.direct_inflight.len());
    }

    fn send_req(&mut self, did: Option<&str>, web: Option<&str>, plc: Option<&str>) {
        let (req, query) = if let (Some(did), Some(web)) = (did, web) {
            tracing::trace!("fetching did");
            (self.client.get(format!("https://{web}/{DOC_PATH}")), Query::Did(did.to_owned()))
        } else if let (Some(did), Some(plc)) = (did, plc) {
            tracing::trace!("fetching did");
            (
                self.client.get(format!("{}/did:plc:{plc}", PLC_URL.as_str())),
                Query::Did(did.to_owned()),
            )
        } else if !self.exporting && *DO_PLC_EXPORT {
            let Some(after) = self.after.take() else {
                return;
            };
            tracing::trace!(%after, "fetching after");
            self.last = Instant::now();
            self.exporting = true;
            (
                self.client.get(format!("{}/{PLC_EXPORT}={after}", PLC_URL.as_str())),
                Query::Export(after),
            )
        } else {
            return;
        };
        let fallback = query.clone();
        let fetch = async move {
            match req.send().await {
                Ok(response) => match response.bytes().await {
                    Ok(bytes) => (query, Some(bytes)),
                    Err(err) => {
                        tracing::debug!(%err, "fetch error");
                        (query, None)
                    }
                },
                Err(err) => {
                    tracing::debug!(%err, "fetch error");
                    (query, None)
                }
            }
        };
        match &self.handle {
            Some(handle) => {
                let task = handle.spawn(fetch);
                self.futures
                    .push(Box::pin(async move { task.await.unwrap_or_else(|_| (fallback, None)) }));
            }
            None => self.futures.push(Box::pin(fetch)),
        }
    }

    /// Handles every completed fetch that is ready now (bounded per call) and
    /// returns the DIDs whose resolution state changed.
    pub async fn poll_inner(&mut self) -> Result<Vec<String>, ResolverError> {
        let mut dids = Vec::new();
        for _ in 0..POLL_BATCH {
            let Ok(Some((query, res))) = timeout(POLL_TIMEOUT, self.futures.next()).await else {
                break;
            };
            self.handle_result(query, res, &mut dids)?;
        }
        if *DO_PLC_EXPORT && !self.exporting && self.last.elapsed() > PLC_EXPORT_INTERVAL {
            self.send_req(None, None, None);
        }
        self.expire_waiters(&mut dids);
        Ok(dids)
    }

    fn handle_result(
        &mut self, query: Query, res: Option<Bytes>, dids: &mut Vec<String>,
    ) -> Result<(), ResolverError> {
        match (query, res) {
            (Query::Did(query), Some(bytes)) => {
                // Clear inflight on every fetch outcome so the DID can
                // be retried; a stuck entry would pin it unresolved.
                self.direct_inflight.remove(&query);
                metrics::record_resolver_inflight("direct", self.direct_inflight.len());
                match parse_did_doc(&bytes) {
                    Some((did, (pds, key))) => {
                        if query != did {
                            tracing::warn!(%query, %did, "did query mismatch");
                            metrics::record_resolver_fetch("mismatch");
                            return Ok(());
                        }
                        self.export_waiters.remove(&did);
                        self.cache.put(did.clone(), (pds, key));
                        metrics::record_resolver_fetch("ok");
                        dids.push(did);
                    }
                    None => metrics::record_resolver_fetch("parse_error"),
                }
            }
            (Query::Did(query), None) => {
                self.direct_inflight.remove(&query);
                metrics::record_resolver_inflight("direct", self.direct_inflight.len());
                metrics::record_resolver_fetch("error");
            }
            (Query::Export(after), Some(bytes)) => {
                self.exporting = false;
                self.after = Some(after);
                let mut count = 0;
                let tx = self.conn.transaction()?;
                let mut stmt = tx.prepare_cached("INSERT OR IGNORE INTO plc_operations (cid, did, created_at, nullified, operation) VALUES (?1, ?2, ?3, ?4, ?5)")?;
                for line in bytes.reader().lines() {
                    count += 1;
                    if let Some(doc) = parse_plc_doc(&line.unwrap_or_default()) {
                        stmt.execute((
                            &doc.cid,
                            &doc.did,
                            &doc.created_at,
                            &doc.nullified,
                            doc.operation.get().as_bytes(),
                        ))?;
                        self.after = Some(doc.created_at);
                        if self.export_waiters.remove(&doc.did).is_some() {
                            dids.push(doc.did);
                        }
                    }
                }
                drop(stmt);
                tx.commit()?;
                if count == 1000 {
                    self.send_req(None, None, None);
                } else {
                    // no more plc operations, drain the waiters
                    dids.extend(self.export_waiters.drain().map(|(did, _)| did));
                }
                metrics::record_resolver_inflight("export_waiters", self.export_waiters.len());
            }
            (Query::Export(after), None) => {
                // Restore the after cursor on export failure so exports can be retried
                self.exporting = false;
                self.after = Some(after);
            }
        }
        Ok(())
    }

    fn expire_waiters(&mut self, dids: &mut Vec<String>) {
        if self.export_waiters.is_empty() {
            return;
        }
        let now = Instant::now();
        let expired: Vec<String> = self
            .export_waiters
            .iter()
            .filter(|(_, since)| now.duration_since(**since) > EXPORT_WAITER_MAX_AGE)
            .map(|(did, _)| did.clone())
            .collect();
        for did in expired {
            self.export_waiters.remove(&did);
            dids.push(did);
        }
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

impl IdentityResolver for Resolver {
    #[inline]
    fn expire(&mut self, did: &str, time: DateTime<Utc>) {
        Self::expire(self, did, time);
    }

    #[inline]
    fn resolve_owned(
        &mut self, did: &str,
    ) -> Result<Option<(Option<String>, DidKey)>, ResolverError> {
        match self.resolve(did)? {
            Some((pds, key)) => Ok(Some((pds.map(str::to_owned), *key))),
            None => Ok(None),
        }
    }

    #[inline]
    fn request_direct(&mut self, did: &str) {
        Self::request_direct(self, did);
    }

    #[inline]
    fn poll(
        &mut self,
    ) -> impl std::future::Future<Output = Result<Vec<String>, ResolverError>> + Send {
        self.poll_inner()
    }
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

fn parse_did_doc(input: &Bytes) -> Option<(String, (DidEndpoint, DidKey))> {
    match serde_json::from_slice::<DidDocument>(input) {
        Ok(doc) => {
            let endpoint =
                if cfg!(feature = "labeler") { "#atproto_labeler" } else { "#atproto_pds" };
            let key = if cfg!(feature = "labeler") { "#atproto_label" } else { "#atproto" };
            let endpoint = doc
                .service
                .as_ref()
                .and_then(|services| services.iter().find(|service| service.id.ends_with(endpoint)))
                .map(|service| service.service_endpoint.as_str());
            let key = doc
                .verification_method
                .as_ref()
                .and_then(|methods| methods.iter().find(|method| method.id.ends_with(key)))
                .and_then(|method| method.public_key_multibase.as_deref());
            Some((doc.id, parse_key_endpoint(endpoint, key)?))
        }
        Err(err) => {
            tracing::debug!(?input, %err, "parse error");
            None
        }
    }
}

fn parse_key_endpoint(endpoint: Option<&str>, key: Option<&str>) -> Option<(DidEndpoint, DidKey)> {
    // key can be null for legacy doc formats
    if let Some(key) = key {
        match multibase::decode(key.trim_start_matches("did:key:")) {
            Ok((_, vec)) => match vec.try_into() {
                Ok(key) => {
                    // endpoint can be null for legacy doc formats
                    let pds = endpoint.and_then(|endpoint| {
                        Some(endpoint.strip_prefix("https://")?.trim_end_matches('/').into())
                    });
                    return Some((pds, key));
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
    None
}

#[cfg(test)]
pub(crate) type ResolveResult = Result<Option<(Option<String>, DidKey)>, ResolverError>;
#[cfg(test)]
pub(crate) type PollResult = Result<Vec<String>, ResolverError>;

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// Minimal scriptable `IdentityResolver` fake for unit tests. The validator hot path
    /// only exercises `resolve_owned` + `request_direct` + `expire` + `poll`.
    pub struct FakeResolver {
        pub script: VecDeque<ResolveResult>,
        pub direct_requests: Vec<String>,
        pub expirations: Vec<(String, DateTime<Utc>)>,
        pub polls: VecDeque<PollResult>,
    }

    impl FakeResolver {
        pub fn new() -> Self {
            Self {
                script: VecDeque::new(),
                direct_requests: Vec::new(),
                expirations: Vec::new(),
                polls: VecDeque::new(),
            }
        }
    }

    impl IdentityResolver for FakeResolver {
        fn expire(&mut self, did: &str, time: DateTime<Utc>) {
            self.expirations.push((did.to_owned(), time));
        }

        fn resolve_owned(&mut self, _did: &str) -> ResolveResult {
            self.script.pop_front().unwrap_or(Ok(None))
        }

        fn request_direct(&mut self, did: &str) {
            self.direct_requests.push(did.to_owned());
        }

        fn poll(&mut self) -> impl std::future::Future<Output = PollResult> + Send {
            let next = self.polls.pop_front().unwrap_or_else(|| Ok(Vec::new()));
            std::future::ready(next)
        }
    }

    #[test]
    fn fake_resolver_resolve_owned_returns_scripted_value() {
        let mut fake = FakeResolver::new();
        fake.script.push_back(Ok(Some((Some("pds.example".to_owned()), [7u8; 35]))));
        fake.script.push_back(Ok(None));
        let r1 = fake.resolve_owned("did:plc:a").unwrap();
        let r2 = fake.resolve_owned("did:plc:b").unwrap();
        assert_eq!(r1, Some((Some("pds.example".to_owned()), [7u8; 35])));
        assert_eq!(r2, None);
    }

    #[test]
    fn fake_resolver_request_direct_records_did() {
        let mut fake = FakeResolver::new();
        fake.request_direct("did:plc:a");
        fake.request_direct("did:plc:b");
        assert_eq!(fake.direct_requests, vec!["did:plc:a".to_owned(), "did:plc:b".to_owned()]);
    }

    #[test]
    fn fake_resolver_expire_records_did_and_time() {
        let mut fake = FakeResolver::new();
        let t = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z").unwrap().with_timezone(&Utc);
        fake.expire("did:plc:a", t);
        assert_eq!(fake.expirations, vec![("did:plc:a".to_owned(), t)]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fake_resolver_poll_returns_scripted_value() {
        let mut fake = FakeResolver::new();
        fake.polls.push_back(Ok(vec!["did:plc:a".to_owned()]));
        fake.polls.push_back(Ok(Vec::new()));
        assert_eq!(fake.poll().await.unwrap(), vec!["did:plc:a".to_owned()]);
        assert_eq!(fake.poll().await.unwrap(), Vec::<String>::new());
    }

    #[test]
    fn parse_key_endpoint_with_null_key_returns_none() {
        assert!(parse_key_endpoint(None, None).is_none());
        assert!(parse_key_endpoint(Some("https://pds.example"), None).is_none());
    }

    #[test]
    fn parse_key_endpoint_strips_https_prefix_and_trailing_slash() {
        let valid_key = "did:key:zQ3shokFTS3brHcDQrn82RUDfCZESWL1ZdCEJwekUDPQiYBme";
        let pair = parse_key_endpoint(Some("https://pds.example.com/"), Some(valid_key));
        match pair {
            Some((Some(pds), _key)) => assert_eq!(pds.as_ref(), "pds.example.com"),
            other => panic!("expected Some endpoint, got {other:?}"),
        }
    }

    #[cfg(not(feature = "labeler"))]
    fn test_resolver(dir: &tempfile::TempDir) -> Resolver {
        let db_path = dir.path().join("plc_directory.db");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE plc_operations (cid TEXT, did TEXT, created_at TEXT, nullified INT, operation BLOB);
             CREATE TABLE plc_keys (did TEXT PRIMARY KEY, pds_endpoint TEXT, pds_key TEXT, labeler_endpoint TEXT, labeler_key TEXT);
             INSERT INTO plc_operations (cid, did, created_at, nullified, operation)
             VALUES ('cid', 'did:plc:seed', '2026-01-01T00:00:00Z', 0, x'7b7d');",
        )
        .unwrap();
        drop(conn);
        Resolver::with_db_path(db_path.to_str().unwrap()).unwrap()
    }

    #[cfg(not(feature = "labeler"))]
    #[test]
    fn repeat_requests_for_pending_did_do_not_stack_futures() {
        let dir = tempfile::TempDir::with_prefix("resolver_test_").unwrap();
        let mut resolver = test_resolver(&dir);
        for _ in 0..5 {
            resolver.request_direct("did:web:pds.example.com");
        }
        assert_eq!(resolver.futures.len(), 1);
        assert_eq!(resolver.direct_inflight.len(), 1);
        for _ in 0..5 {
            resolver.request_direct("did:plc:aaaabbbbccccdddd");
        }
        assert_eq!(resolver.futures.len(), 2);
        assert_eq!(resolver.direct_inflight.len(), 2);
    }

    #[cfg(not(feature = "labeler"))]
    #[test]
    fn distinct_did_fetches_are_capped() {
        let dir = tempfile::TempDir::with_prefix("resolver_test_").unwrap();
        let mut resolver = test_resolver(&dir);
        resolver.set_direct_rate(1e9, 1e9);
        for i in 0..(MAX_INFLIGHT_FETCHES + 10) {
            resolver.request_direct(&format!("did:web:host{i}.example.com"));
        }
        assert_eq!(resolver.futures.len(), MAX_INFLIGHT_FETCHES);
        assert_eq!(resolver.direct_inflight.len(), MAX_INFLIGHT_FETCHES);
    }

    #[cfg(not(feature = "labeler"))]
    #[test]
    fn invalid_did_leaves_no_inflight_entry() {
        let dir = tempfile::TempDir::with_prefix("resolver_test_").unwrap();
        let mut resolver = test_resolver(&dir);
        resolver.request_direct("did:example:nonsense");
        resolver.request_direct("not-a-did");
        assert_eq!(resolver.futures.len(), 0);
        assert_eq!(resolver.direct_inflight.len(), 0);
    }

    #[cfg(not(feature = "labeler"))]
    fn did_doc(did: &str, pds: &str) -> Bytes {
        Bytes::from(
            serde_json::json!({
                "id": did,
                "verificationMethod": [{
                    "id": format!("{did}#atproto"),
                    "type": "Multikey",
                    "controller": did,
                    "publicKeyMultibase": "zQ3shokFTS3brHcDQrn82RUDfCZESWL1ZdCEJwekUDPQiYBme"
                }],
                "service": [{
                    "id": "#atproto_pds",
                    "type": "AtprotoPersonalDataServer",
                    "serviceEndpoint": format!("https://{pds}")
                }]
            })
            .to_string(),
        )
    }

    #[cfg(not(feature = "labeler"))]
    #[test]
    fn request_parks_plc_dids_on_the_export_stream_and_direct_promotes_them() {
        let dir = tempfile::TempDir::with_prefix("resolver_test_").unwrap();
        let mut resolver = test_resolver(&dir);
        resolver.request("did:plc:aaaabbbbccccdddd");
        resolver.request("did:plc:aaaabbbbccccdddd");
        assert_eq!(resolver.export_waiters.len(), 1);
        assert!(resolver.exporting, "a waiter kicks an export page fetch");
        assert_eq!(resolver.futures.len(), 1);
        // resolve() reports pending while parked
        assert!(resolver.resolve("did:plc:aaaabbbbccccdddd").unwrap().is_none());
        // promotion: a direct fetch is issued even though the DID is a waiter
        resolver.request_direct("did:plc:aaaabbbbccccdddd");
        assert!(resolver.direct_inflight.contains("did:plc:aaaabbbbccccdddd"));
        assert_eq!(resolver.futures.len(), 2);
        // non-plc DIDs go straight to a direct fetch
        resolver.request("did:web:pds.example.com");
        assert!(resolver.direct_inflight.contains("did:web:pds.example.com"));
        // a second export request while one is running does not stack
        resolver.request("did:plc:eeeeffffgggghhhh");
        assert_eq!(resolver.futures.len(), 3);
    }

    #[cfg(not(feature = "labeler"))]
    #[test]
    fn export_waiters_are_bounded_and_direct_fetches_are_rate_limited() {
        let dir = tempfile::TempDir::with_prefix("resolver_test_").unwrap();
        let mut resolver = test_resolver(&dir);
        resolver.set_direct_rate(0.0, 1.0);
        resolver.request_direct("did:web:a.example");
        resolver.request_direct("did:web:b.example");
        assert_eq!(resolver.direct_inflight.len(), 1, "bucket of one token");
        resolver.request_direct("did:plc:aaaabbbbccccdddd");
        assert_eq!(resolver.direct_inflight.len(), 1);
        for i in 0..MAX_EXPORT_WAITERS + 5 {
            resolver.export_waiters.insert(format!("did:plc:{i}"), Instant::now());
        }
        let before = resolver.export_waiters.len();
        resolver.request("did:plc:overflow");
        assert_eq!(resolver.export_waiters.len(), before);
        let mut bucket = TokenBucket::new(1000.0, 2.0);
        assert!(bucket.take());
        assert!(bucket.take());
        assert!(!bucket.take());
        std::thread::sleep(Duration::from_millis(5));
        assert!(bucket.take());
    }

    #[cfg(not(feature = "labeler"))]
    #[tokio::test(flavor = "current_thread")]
    async fn poll_inner_handles_every_fetch_outcome() {
        let dir = tempfile::TempDir::with_prefix("resolver_test_").unwrap();
        let mut resolver = test_resolver(&dir);
        resolver.direct_inflight.insert("did:plc:ok".to_owned());
        resolver.export_waiters.insert("did:plc:ok".to_owned(), Instant::now());
        resolver.direct_inflight.insert("did:plc:mismatch".to_owned());
        resolver.direct_inflight.insert("did:plc:garbage".to_owned());
        resolver.direct_inflight.insert("did:plc:failed".to_owned());
        let push = |resolver: &mut Resolver, q: Query, b: Option<Bytes>| {
            resolver.futures.push(Box::pin(async move { (q, b) }));
        };
        push(
            &mut resolver,
            Query::Did("did:plc:ok".to_owned()),
            Some(did_doc("did:plc:ok", "pds.a")),
        );
        push(
            &mut resolver,
            Query::Did("did:plc:mismatch".to_owned()),
            Some(did_doc("did:plc:other", "pds.b")),
        );
        push(
            &mut resolver,
            Query::Did("did:plc:garbage".to_owned()),
            Some(Bytes::from_static(b"{")),
        );
        push(&mut resolver, Query::Did("did:plc:failed".to_owned()), None);
        let mut from_docs = resolver.poll_inner().await.unwrap();
        from_docs.sort();
        assert_eq!(from_docs, vec!["did:plc:ok".to_owned()]);
        assert!(resolver.direct_inflight.is_empty());
        assert!(resolver.export_waiters.is_empty(), "a resolved waiter leaves the export set");
        assert!(resolver.cache.contains("did:plc:ok"));
        let (endpoint, _) = resolver.resolve("did:plc:ok").unwrap().unwrap();
        assert_eq!(endpoint, Some("pds.a"));

        // export page: op for a waiter resolves it; short page drains the rest; failure restores cursor
        resolver.export_waiters.insert("did:plc:waiter".to_owned(), Instant::now());
        resolver.export_waiters.insert("did:plc:drained".to_owned(), Instant::now());
        resolver.exporting = true;
        let op = serde_json::json!({
            "did": "did:plc:waiter", "operation": {"type": "plc_operation"}, "cid": "bafycid1",
            "nullified": false, "createdAt": "2026-09-23T00:00:00.000Z"
        });
        let page = format!("{op}\nnot json\n");
        push(
            &mut resolver,
            Query::Export("2026-01-01T00:00:00Z".to_owned()),
            Some(Bytes::from(page)),
        );
        let mut from_export = resolver.poll_inner().await.unwrap();
        from_export.sort();
        assert_eq!(from_export, vec!["did:plc:drained".to_owned(), "did:plc:waiter".to_owned()]);
        assert!(!resolver.exporting);
        assert_eq!(resolver.after.as_deref(), Some("2026-09-23T00:00:00.000Z"));
        let rows: i64 = resolver
            .conn
            .query_row(
                "SELECT COUNT(*) FROM plc_operations WHERE did = 'did:plc:waiter'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rows, 1);

        resolver.exporting = true;
        push(&mut resolver, Query::Export("cursor-x".to_owned()), None);
        assert!(resolver.poll_inner().await.unwrap().is_empty());
        assert_eq!(resolver.after.as_deref(), Some("cursor-x"));
        assert!(!resolver.exporting);

        // a full page chains another export request
        resolver.exporting = true;
        let full: String = (0..1000).map(|_| "x\n").collect();
        push(&mut resolver, Query::Export("c".to_owned()), Some(Bytes::from(full)));
        resolver.poll_inner().await.unwrap();
        assert!(resolver.exporting, "count == 1000 requests the next page");

        // stale waiters are handed back unresolved
        resolver.export_waiters.insert(
            "did:plc:stale".to_owned(),
            Instant::now().checked_sub(EXPORT_WAITER_MAX_AGE + Duration::from_secs(1)).unwrap(),
        );
        let expired = resolver.poll_inner().await.unwrap();
        assert_eq!(expired, vec!["did:plc:stale".to_owned()]);
        assert!(resolver.export_waiters.is_empty());
    }

    #[cfg(not(feature = "labeler"))]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn fetches_run_on_the_shared_runtime_when_available() {
        let dir = tempfile::TempDir::with_prefix("resolver_test_").unwrap();
        let mut resolver = test_resolver(&dir);
        assert!(resolver.handle.is_some());
        resolver.request_direct("did:web:127.0.0.1");
        assert_eq!(resolver.futures.len(), 1);
        // the fetch fails (nothing listens) and the outcome is reported as an error
        let deadline = Instant::now() + Duration::from_secs(20);
        while resolver.direct_inflight.contains("did:web:127.0.0.1") && Instant::now() < deadline {
            resolver.poll_inner().await.unwrap();
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(!resolver.direct_inflight.contains("did:web:127.0.0.1"));
    }

    #[cfg(not(feature = "labeler"))]
    #[tokio::test(flavor = "current_thread")]
    async fn identity_resolver_impl_and_query_db_paths() {
        let dir = tempfile::TempDir::with_prefix("resolver_test_").unwrap();
        let mut resolver = test_resolver(&dir);
        resolver
            .conn
            .execute(
                "INSERT INTO plc_keys (did, pds_endpoint, pds_key, labeler_endpoint, labeler_key)
                 VALUES ('did:plc:indb', 'https://pds.db/', 'did:key:zQ3shokFTS3brHcDQrn82RUDfCZESWL1ZdCEJwekUDPQiYBme', NULL, NULL),
                        ('did:plc:nokey', 'https://pds.db/', NULL, NULL, NULL)",
                [],
            )
            .unwrap();
        let (pds, _) =
            IdentityResolver::resolve_owned(&mut resolver, "did:plc:indb").unwrap().unwrap();
        assert_eq!(pds.as_deref(), Some("pds.db"));
        assert!(resolver.cache.contains("did:plc:indb"));
        assert!(IdentityResolver::resolve_owned(&mut resolver, "did:plc:nokey").unwrap().is_none());
        assert!(
            IdentityResolver::resolve_owned(&mut resolver, "did:plc:missing").unwrap().is_none()
        );
        assert!(resolver.export_waiters.contains_key("did:plc:missing"), "a miss parks the DID");
        IdentityResolver::request_direct(&mut resolver, "did:plc:missing");
        assert!(resolver.direct_inflight.contains("did:plc:missing"));
        let t = DateTime::parse_from_rfc3339("2030-01-01T00:00:00Z").unwrap().with_timezone(&Utc);
        resolver.after = Some("2026-01-01T00:00:00Z".to_owned());
        IdentityResolver::expire(&mut resolver, "did:plc:indb", t);
        assert!(
            !resolver.cache.contains("did:plc:indb"),
            "a newer identity event evicts the cache"
        );
        assert!(IdentityResolver::poll(&mut resolver).await.unwrap().is_empty());
        resolver.request("not-a-did");
        assert!(!resolver.direct_inflight.contains("not-a-did"));
    }

    #[test]
    fn parse_key_endpoint_rejects_bad_keys() {
        assert!(parse_key_endpoint(Some("https://p"), Some("did:key:zQ3shokFTS3")).is_none());
        assert!(parse_key_endpoint(Some("https://p"), Some("!!!")).is_none());
    }
}
