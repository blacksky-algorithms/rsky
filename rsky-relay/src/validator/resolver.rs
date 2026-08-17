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

use rsky_identity::types::DidDocument;

use crate::config::{CAPACITY_CACHE, DO_PLC_EXPORT, PLC_EXPORT_INTERVAL};
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
const REQ_TIMEOUT: Duration = Duration::from_secs(30);
const TCP_KEEPALIVE: Duration = Duration::from_secs(300);
// Hard ceiling on concurrent DID fetches: event floods from never-before-seen
// DIDs must not grow the future set without bound. Skipped DIDs retry on
// their next event once capacity frees.
const MAX_INFLIGHT_FETCHES: usize = 4096;

const PLC_URL: &str = "https://plc.directory";
const PLC_EXPORT: &str = "export?count=1000&after";
const DOC_PATH: &str = ".well-known/did.json";

type RequestFuture = Pin<Box<dyn Future<Output = (Query, reqwest::Result<Bytes>)> + Send>>;

#[derive(Debug)]
enum Query {
    Did(String),
    Export(String),
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
    inflight: HashSet<String>,
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
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS plc_operations (cid TEXT, did TEXT, created_at TEXT, nullified INT, operation BLOB);
                 CREATE TABLE IF NOT EXISTS plc_keys (did TEXT PRIMARY KEY, pds_endpoint TEXT, pds_key TEXT, labeler_endpoint TEXT, labeler_key TEXT);",
            )?;
        }
        let now = Instant::now();
        let last = now.checked_sub(PLC_EXPORT_INTERVAL).unwrap_or(now);
        let after = match conn.query_one(
            "SELECT created_at FROM plc_operations ORDER BY created_at DESC LIMIT 1",
            [],
            |row| row.get::<_, String>("created_at"),
        ) {
            Ok(created_at) => Some(created_at),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(err) => Err(err)?,
        };
        let client = Client::builder()
            .user_agent("rsky-relay")
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

    pub fn resolve(&mut self, did: &str) -> Result<Option<(Option<&str>, &DidKey)>, ResolverError> {
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

    pub fn request(&mut self, did: &str) {
        self.request_inner(did, false);
    }

    /// Force an individual DID lookup from plc.directory, bypassing the export stream.
    /// Used when a hostname mismatch suggests the user may have migrated.
    pub fn request_direct(&mut self, did: &str) {
        self.request_inner(did, true);
    }

    fn request_inner(&mut self, did: &str, force_direct: bool) {
        // One fetch per DID at a time, bounded overall: repeat events for a
        // pending DID must not stack additional futures.
        if self.inflight.contains(did) || self.futures.len() >= MAX_INFLIGHT_FETCHES {
            return;
        }
        if let Some(plc) = did.strip_prefix("did:plc:") {
            let plc = if *DO_PLC_EXPORT && !force_direct { None } else { Some(plc) };
            self.inflight.insert(did.to_owned());
            self.send_req(Some(did), None, plc);
        } else if let Some(web) = did.strip_prefix("did:web:") {
            let Ok(web) = urlencoding::decode(web) else {
                tracing::debug!(%did, "invalid did");
                return;
            };
            self.inflight.insert(did.to_owned());
            self.send_req(Some(did), Some(&web), None);
        } else {
            tracing::debug!(%did, "invalid did");
        }
    }

    fn send_req(&mut self, did: Option<&str>, web: Option<&str>, plc: Option<&str>) {
        let (req, query) = if let (Some(did), Some(web)) = (did, web) {
            tracing::trace!("fetching did");
            (self.client.get(format!("https://{web}/{DOC_PATH}")), Query::Did(did.to_owned()))
        } else if let (Some(did), Some(plc)) = (did, plc) {
            tracing::trace!("fetching did");
            (self.client.get(format!("{PLC_URL}/did:plc:{plc}")), Query::Did(did.to_owned()))
        } else if let Some(after) = self.after.take() {
            tracing::trace!(%after, "fetching after");
            self.last = Instant::now();
            (self.client.get(format!("{PLC_URL}/{PLC_EXPORT}={after}")), Query::Export(after))
        } else {
            return;
        };
        self.futures.push(Box::pin(async move {
            match req.send().await {
                Ok(req) => match req.bytes().await {
                    Ok(bytes) => (query, Ok(bytes)),
                    Err(err) => (query, Err(err)),
                },
                Err(err) => (query, Err(err)),
            }
        }));
    }

    pub async fn poll_inner(&mut self) -> Result<Vec<String>, ResolverError> {
        if let Ok(Some((query, res))) = timeout(POLL_TIMEOUT, self.futures.next()).await {
            match res {
                Ok(bytes) => match query {
                    Query::Did(query) => {
                        // Clear inflight on every fetch outcome so the DID can
                        // be retried; a stuck entry would pin it unresolved.
                        self.inflight.remove(&query);
                        if let Some((did, (pds, key))) = parse_did_doc(&bytes) {
                            if query != did {
                                tracing::warn!(%query, %did, "did query mismatch");
                                return Ok(Vec::new());
                            }
                            self.cache.put(did.clone(), (pds, key));
                            return Ok(vec![did]);
                        }
                    }
                    Query::Export(after) => {
                        self.after = Some(after);
                        let mut dids = Vec::new();
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
                                // Export is chronological, so the last non-nullified op wins;
                                // nullified ops are losing forks and never update routing.
                                if !doc.nullified {
                                    apply_pds_key(&tx, &doc.did, doc.operation.get().as_bytes())?;
                                }
                                self.after = Some(doc.created_at);
                                if self.inflight.remove(&doc.did) {
                                    dids.push(doc.did);
                                }
                            }
                        }
                        drop(stmt);
                        tx.commit()?;
                        if count == 1000 {
                            self.send_req(None, None, None);
                        } else {
                            // no more plc operations, drain inflight dids
                            dids.extend(
                                self.inflight.extract_if(|did| did.starts_with("did:plc:")),
                            );
                        }
                        return Ok(dids);
                    }
                },
                Err(err) => {
                    tracing::debug!(%err, "fetch error");
                    match query {
                        // Restore the after cursor on export failure so exports can be retried
                        Query::Export(after) => {
                            self.after = Some(after);
                        }
                        // Clear inflight on failed DID fetches so they can be retried
                        Query::Did(query) => {
                            self.inflight.remove(&query);
                        }
                    }
                }
            }
        } else if *DO_PLC_EXPORT && self.last.elapsed() > PLC_EXPORT_INTERVAL {
            self.send_req(None, None, None);
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

/// PDS routing derived from a single PLC operation, ready to apply to `plc_keys`.
enum KeyUpdate {
    /// The account's current atproto signing key (and PDS endpoint, if declared).
    Set { endpoint: Option<String>, key: String },
    /// The account was tombstoned; drop any cached routing.
    Tombstone,
}

#[derive(Deserialize)]
struct PlcOpService {
    endpoint: String,
}

/// The subset of a PLC operation needed to route atproto traffic. Genesis `create`
/// operations use the legacy flat shape; every later op uses `plc_operation`.
#[derive(Deserialize)]
#[serde(tag = "type")]
enum PlcOperation {
    #[serde(rename = "plc_operation")]
    Operation {
        #[serde(rename = "verificationMethods", default)]
        verification_methods: std::collections::HashMap<String, String>,
        #[serde(default)]
        services: std::collections::HashMap<String, PlcOpService>,
    },
    #[serde(rename = "create")]
    Create {
        #[serde(rename = "signingKey")]
        signing_key: String,
        service: String,
    },
    #[serde(rename = "plc_tombstone")]
    Tombstone,
    #[serde(other)]
    Other,
}

/// Derive the atproto PDS signing key and endpoint from a raw PLC operation. Returns
/// `None` for operations that don't change PDS routing (unknown op types, or ops with no
/// atproto verification method). Applies only to the PDS plane: `--no-plc-export`/labeler
/// builds never run the export path this feeds.
fn derive_pds_key(operation: &[u8]) -> Option<KeyUpdate> {
    match serde_json::from_slice::<PlcOperation>(operation) {
        Ok(PlcOperation::Operation { verification_methods, mut services }) => {
            let key = verification_methods.get("atproto")?.clone();
            let endpoint = services.remove("atproto_pds").map(|svc| svc.endpoint);
            Some(KeyUpdate::Set { endpoint, key })
        }
        Ok(PlcOperation::Create { signing_key, service }) => {
            Some(KeyUpdate::Set { endpoint: Some(service), key: signing_key })
        }
        Ok(PlcOperation::Tombstone) => Some(KeyUpdate::Tombstone),
        Ok(PlcOperation::Other) => None,
        Err(err) => {
            tracing::debug!(%err, "plc op parse error");
            None
        }
    }
}

/// Apply a non-nullified PLC operation's derived routing to `plc_keys`: upsert the
/// current key/endpoint, drop the row on a tombstone, or no-op for irrelevant ops.
/// `conn` may be a transaction; `prepare_cached` keeps the export hot path fast.
fn apply_pds_key(conn: &Connection, did: &str, operation: &[u8]) -> rusqlite::Result<()> {
    match derive_pds_key(operation) {
        Some(KeyUpdate::Set { endpoint, key }) => {
            conn.prepare_cached(
                "INSERT INTO plc_keys (did, pds_endpoint, pds_key) VALUES (?1, ?2, ?3) \
                 ON CONFLICT(did) DO UPDATE SET pds_endpoint = excluded.pds_endpoint, pds_key = excluded.pds_key",
            )?
            .execute((did, &endpoint, &key))?;
        }
        Some(KeyUpdate::Tombstone) => {
            conn.prepare_cached("DELETE FROM plc_keys WHERE did = ?1")?.execute((did,))?;
        }
        None => {}
    }
    Ok(())
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
mod tests {
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

    #[test]
    fn repeat_requests_for_pending_did_do_not_stack_futures() {
        let dir = tempfile::TempDir::with_prefix("resolver_test_").unwrap();
        let mut resolver = test_resolver(&dir);
        for _ in 0..5 {
            resolver.request_direct("did:web:pds.example.com");
        }
        assert_eq!(resolver.futures.len(), 1);
        assert_eq!(resolver.inflight.len(), 1);
        for _ in 0..5 {
            resolver.request_direct("did:plc:aaaabbbbccccdddd");
        }
        assert_eq!(resolver.futures.len(), 2);
        assert_eq!(resolver.inflight.len(), 2);
    }

    #[test]
    fn distinct_did_fetches_are_capped() {
        let dir = tempfile::TempDir::with_prefix("resolver_test_").unwrap();
        let mut resolver = test_resolver(&dir);
        for i in 0..(MAX_INFLIGHT_FETCHES + 10) {
            resolver.request_direct(&format!("did:web:host{i}.example.com"));
        }
        assert_eq!(resolver.futures.len(), MAX_INFLIGHT_FETCHES);
        assert_eq!(resolver.inflight.len(), MAX_INFLIGHT_FETCHES);
    }

    #[test]
    fn invalid_did_leaves_no_inflight_entry() {
        let dir = tempfile::TempDir::with_prefix("resolver_test_").unwrap();
        let mut resolver = test_resolver(&dir);
        resolver.request_direct("did:example:nonsense");
        resolver.request_direct("not-a-did");
        assert_eq!(resolver.futures.len(), 0);
        assert_eq!(resolver.inflight.len(), 0);
    }

    const VALID_KEY: &str = "did:key:zQ3shokFTS3brHcDQrn82RUDfCZESWL1ZdCEJwekUDPQiYBme";

    fn plc_op(endpoint: Option<&str>) -> String {
        endpoint.map_or_else(
            || format!(
                r#"{{"type":"plc_operation","verificationMethods":{{"atproto":"{VALID_KEY}"}},"services":{{}}}}"#
            ),
            |ep| format!(
                r#"{{"type":"plc_operation","verificationMethods":{{"atproto":"{VALID_KEY}"}},"services":{{"atproto_pds":{{"type":"AtprotoPersonalDataServer","endpoint":"{ep}"}}}}}}"#
            ),
        )
    }

    #[test]
    fn derive_pds_key_modern_operation_yields_key_and_endpoint() {
        let op = plc_op(Some("https://pds.example.com"));
        match derive_pds_key(op.as_bytes()) {
            Some(KeyUpdate::Set { endpoint, key }) => {
                assert_eq!(endpoint.as_deref(), Some("https://pds.example.com"));
                assert_eq!(key, VALID_KEY);
            }
            other => panic!("expected Set, got {}", label(other.as_ref())),
        }
    }

    #[test]
    fn derive_pds_key_operation_without_pds_service_has_no_endpoint() {
        let op = plc_op(None);
        match derive_pds_key(op.as_bytes()) {
            Some(KeyUpdate::Set { endpoint, key }) => {
                assert!(endpoint.is_none());
                assert_eq!(key, VALID_KEY);
            }
            other => panic!("expected Set, got {}", label(other.as_ref())),
        }
    }

    #[test]
    fn derive_pds_key_operation_without_atproto_key_is_ignored() {
        let op = r#"{"type":"plc_operation","verificationMethods":{},"services":{}}"#;
        assert!(derive_pds_key(op.as_bytes()).is_none());
    }

    #[test]
    fn derive_pds_key_legacy_create_yields_key_and_endpoint() {
        let op = format!(
            r#"{{"type":"create","signingKey":"{VALID_KEY}","recoveryKey":"{VALID_KEY}","handle":"alice.test","service":"https://legacy.example.com","prev":null,"sig":"x"}}"#
        );
        match derive_pds_key(op.as_bytes()) {
            Some(KeyUpdate::Set { endpoint, key }) => {
                assert_eq!(endpoint.as_deref(), Some("https://legacy.example.com"));
                assert_eq!(key, VALID_KEY);
            }
            other => panic!("expected Set, got {}", label(other.as_ref())),
        }
    }

    #[test]
    fn derive_pds_key_tombstone_signals_removal() {
        let op = r#"{"type":"plc_tombstone","prev":"bafy","sig":"x"}"#;
        assert!(matches!(derive_pds_key(op.as_bytes()), Some(KeyUpdate::Tombstone)));
    }

    #[test]
    fn derive_pds_key_unknown_type_is_ignored() {
        let op = r#"{"type":"something_else"}"#;
        assert!(derive_pds_key(op.as_bytes()).is_none());
    }

    #[test]
    fn derive_pds_key_malformed_json_is_ignored() {
        assert!(derive_pds_key(b"not json").is_none());
    }

    fn label(update: Option<&KeyUpdate>) -> &'static str {
        match update {
            Some(KeyUpdate::Set { .. }) => "Set",
            Some(KeyUpdate::Tombstone) => "Tombstone",
            None => "None",
        }
    }

    /// The stored endpoint for `did`, or `None` when no `plc_keys` row exists. These
    /// tests only ever store non-null endpoints, so `None` unambiguously means "no row".
    fn pds_endpoint(resolver: &Resolver, did: &str) -> Option<String> {
        resolver
            .conn
            .query_one("SELECT pds_endpoint FROM plc_keys WHERE did = ?1", [did], |row| {
                row.get::<_, String>(0)
            })
            .ok()
    }

    #[test]
    fn apply_pds_key_upserts_then_resolves() {
        let dir = tempfile::TempDir::with_prefix("resolver_test_").unwrap();
        let mut resolver = test_resolver(&dir);
        apply_pds_key(&resolver.conn, "did:plc:alice", plc_op(Some("https://pds.one")).as_bytes())
            .unwrap();
        assert_eq!(pds_endpoint(&resolver, "did:plc:alice"), Some("https://pds.one".into()));

        assert!(resolver.query_db("did:plc:alice").unwrap());
        match resolver.resolve("did:plc:alice").unwrap() {
            Some((Some(pds), _key)) => assert_eq!(pds, "pds.one"),
            other => panic!("expected resolved endpoint, got {other:?}"),
        }
    }

    #[test]
    fn apply_pds_key_latest_write_wins() {
        let dir = tempfile::TempDir::with_prefix("resolver_test_").unwrap();
        let resolver = test_resolver(&dir);
        apply_pds_key(&resolver.conn, "did:plc:bob", plc_op(Some("https://pds.old")).as_bytes())
            .unwrap();
        apply_pds_key(&resolver.conn, "did:plc:bob", plc_op(Some("https://pds.new")).as_bytes())
            .unwrap();
        assert_eq!(pds_endpoint(&resolver, "did:plc:bob"), Some("https://pds.new".into()));
    }

    #[test]
    fn apply_pds_key_tombstone_removes_row() {
        let dir = tempfile::TempDir::with_prefix("resolver_test_").unwrap();
        let resolver = test_resolver(&dir);
        apply_pds_key(&resolver.conn, "did:plc:carol", plc_op(Some("https://pds.gone")).as_bytes())
            .unwrap();
        apply_pds_key(
            &resolver.conn,
            "did:plc:carol",
            br#"{"type":"plc_tombstone","prev":"bafy"}"#,
        )
        .unwrap();
        assert_eq!(pds_endpoint(&resolver, "did:plc:carol"), None);
    }

    #[test]
    fn apply_pds_key_irrelevant_op_is_noop() {
        let dir = tempfile::TempDir::with_prefix("resolver_test_").unwrap();
        let resolver = test_resolver(&dir);
        apply_pds_key(&resolver.conn, "did:plc:dave", br#"{"type":"noop"}"#).unwrap();
        assert_eq!(pds_endpoint(&resolver, "did:plc:dave"), None);
    }

    #[test]
    fn empty_operations_table_yields_no_cursor() {
        let dir = tempfile::TempDir::with_prefix("resolver_test_").unwrap();
        let db_path = dir.path().join("plc_directory.db");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE plc_operations (cid TEXT, did TEXT, created_at TEXT, nullified INT, operation BLOB);
             CREATE TABLE plc_keys (did TEXT PRIMARY KEY, pds_endpoint TEXT, pds_key TEXT, labeler_endpoint TEXT, labeler_key TEXT);",
        )
        .unwrap();
        drop(conn);
        let resolver = Resolver::with_db_path(db_path.to_str().unwrap()).unwrap();
        assert!(resolver.after.is_none());
    }
}
