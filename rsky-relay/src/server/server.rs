use std::fs::File;
use std::io::{self, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::Result;
use color_eyre::eyre::eyre;
use httparse::{EMPTY_HEADER, Status};
#[cfg(not(feature = "labeler"))]
use rusqlite::named_params;
#[cfg(feature = "labeler")]
use rusqlite::{Connection, OpenFlags};
#[cfg(not(feature = "labeler"))]
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use thiserror::Error;
use url::Url;

use crate::SHUTDOWN;
use crate::config::{ADMIN_PASSWORD, HOSTS_INTERVAL, PORT};
#[cfg(not(feature = "labeler"))]
use crate::config::{HOSTS_MIN_ACCOUNTS, HOSTS_RELAYS};
use crate::crawler::{RequestCrawl, RequestCrawlSender};
#[cfg(not(feature = "labeler"))]
use crate::metrics;
use crate::publisher::{MaybeTlsStream, SubscribeRepos, SubscribeReposSender};
use crate::server::types::{BannedHost, ListBans};
#[cfg(not(feature = "labeler"))]
use crate::server::types::{GetHostStatus, Host, HostStatus, ListHosts};

#[cfg(not(feature = "labeler"))]
pub trait HostListFetcher {
    fn fetch_page(&self, cursor: Option<&str>) -> Result<ListHosts>;
}

#[cfg(not(feature = "labeler"))]
struct ReqwestHostListFetcher {
    client: reqwest::blocking::Client,
    base_url: String,
}

#[cfg(not(feature = "labeler"))]
impl HostListFetcher for ReqwestHostListFetcher {
    fn fetch_page(&self, cursor: Option<&str>) -> Result<ListHosts> {
        let mut params: Vec<(&str, &str)> = vec![("limit", "1000")];
        if let Some(c) = cursor {
            params.push(("cursor", c));
        }
        let url = Url::parse_with_params(&self.base_url, params)?;
        Ok(self.client.get(url).send()?.json()?)
    }
}

/// Walks every `listHosts` page of one upstream and returns crawlable hostnames,
/// largest account count first; bans are applied by the caller on the accept
/// thread, which owns the `SQLite` handle.
#[cfg(not(feature = "labeler"))]
pub fn discover_hosts<F: HostListFetcher + ?Sized>(
    fetcher: &F, sleep: impl Fn(Duration) + Copy, seen: &mut hashbrown::HashSet<String>,
    upstream: &str,
) -> Vec<String> {
    let mut cursor: Option<String> = None;
    let mut total_seen: usize = 0;
    let mut added = Vec::new();
    let mut had_failure = false;
    loop {
        let page = match fetch_page_with_retry(fetcher, cursor.as_deref(), sleep) {
            Ok(page) => page,
            Err(err) => {
                tracing::warn!(%err, %upstream, "listHosts page failed after retries");
                had_failure = true;
                break;
            }
        };
        total_seen += page.hosts.len();
        let mut sorted = page.hosts;
        sorted.sort_unstable_by_key(|host| host.account_count);
        for host in sorted.into_iter().rev() {
            if host.account_count > HOSTS_MIN_ACCOUNTS
                && matches!(host.status, HostStatus::Active | HostStatus::Idle)
                && seen.insert(host.hostname.clone())
            {
                added.push(host.hostname);
            }
        }
        cursor = page.cursor;
        if cursor.is_none() {
            break;
        }
    }
    let outcome =
        if had_failure { if added.is_empty() { "fail" } else { "partial" } } else { "ok" };
    metrics::record_discovery_round(outcome);
    tracing::info!(total = %total_seen, added = %added.len(), %outcome, %upstream, "host discovery refresh complete");
    added
}

#[cfg(not(feature = "labeler"))]
pub fn fetch_page_with_retry<F: HostListFetcher + ?Sized>(
    fetcher: &F, cursor: Option<&str>, sleep: impl Fn(Duration),
) -> Result<ListHosts> {
    let mut delay = Duration::from_secs(1);
    let mut last: Option<color_eyre::Report> = None;
    for attempt in 0..3 {
        match fetcher.fetch_page(cursor) {
            Ok(page) => return Ok(page),
            Err(err) => {
                tracing::warn!(%err, attempt, "listHosts page fetch failed; retrying");
                last = Some(err);
                if attempt < 2 {
                    sleep(delay);
                    delay = delay.saturating_mul(2);
                }
            }
        }
    }
    Err(last.unwrap_or_else(|| eyre!("listHosts retries exhausted with no error")))
}

const SLEEP: Duration = Duration::from_millis(10);
const ACCEPTS_PER_TICK: usize = 64;
// One stalled client must not hold the accept loop: bounded socket I/O.
const SOCKET_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(not(feature = "labeler"))]
const DISCOVERY_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[cfg(not(feature = "labeler"))]
const PATH_LIST_HOSTS: &str = "/xrpc/com.atproto.sync.listHosts";

#[cfg(not(feature = "labeler"))]
const PATH_HOST_STATUS: &str = "/xrpc/com.atproto.sync.getHostStatus";

const PATH_SUBSCRIBE: &str = if cfg!(feature = "labeler") {
    "/xrpc/com.atproto.label.subscribeLabels"
} else {
    "/xrpc/com.atproto.sync.subscribeRepos"
};
const PATH_REQUEST_CRAWL: &str = if cfg!(feature = "labeler") {
    "/xrpc/com.atproto.label.requestCrawl"
} else {
    "/xrpc/com.atproto.sync.requestCrawl"
};

const PATH_ADMIN_BAN: &str = "/admin/pds/ban";
const PATH_ADMIN_UNBAN: &str = "/admin/pds/unban";
const PATH_ADMIN_LIST_BANS: &str = "/admin/pds/listBans";

const INDEX_ASCII: &str = r"
    .------..------..------..------.
    |R.--. ||S.--. ||K.--. ||Y.--. |
    | :(): || :/\: || :/\: || (\/) |
    | ()() || :\/: || :\/: || :\/: |
    | '--'R|| '--'S|| '--'K|| '--'Y|
    `------'`------'`------'`------'
    .------..------..------..------..------.
    |R.--. ||E.--. ||L.--. ||A.--. ||Y.--. |
    | :(): || (\/) || :/\: || (\/) || (\/) |
    | ()() || :\/: || (__) || :\/: || :\/: |
    | '--'R|| '--'E|| '--'L|| '--'A|| '--'Y|
    `------'`------'`------'`------'`------'

 This is an atproto relay instance running the
 'rsky-relay' codebase [https://github.com/blacksky-algorithms/rsky]

 The firehose WebSocket path is at:  /xrpc/com.atproto.sync.subscribeRepos
";

#[derive(Debug, Error)]
pub enum ServerError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("rustls error: {0}")]
    Rustls(#[from] rustls::Error),
    #[error("rtrb error: {0}")]
    PushError(#[from] rtrb::PushError<RequestCrawl>),
    #[error("url parse error: {0}")]
    UrlParse(#[from] url::ParseError),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

#[derive(Debug)]
struct ErrorOnDropTcpStream(Option<MaybeTlsStream<TcpStream>>);

impl Drop for ErrorOnDropTcpStream {
    #[cold]
    fn drop(&mut self) {
        let Some(mut stream) = self.0.take() else {
            return;
        };
        let _err = stream.write_all(b"HTTP/1.1 400 Bad Request\n");
        let _err = stream.flush();
        let _err = stream.shutdown();
    }
}

fn write_response(stream: &mut ErrorOnDropTcpStream, status: &str, body: &str) -> Result<()> {
    write_with_type(stream, status, "text/plain; charset=utf-8", body)
}

fn write_json(stream: &mut ErrorOnDropTcpStream, status: &str, body: &str) -> Result<()> {
    write_with_type(stream, status, "application/json", body)
}

fn write_with_type(
    stream: &mut ErrorOnDropTcpStream, status: &str, content_type: &str, body: &str,
) -> Result<()> {
    let response = format!(
        "HTTP/1.1 {status}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        body.len()
    );
    #[expect(clippy::unwrap_used)]
    let mut s = stream.0.take().unwrap();
    s.write_all(response.as_bytes())?;
    s.flush()?;
    s.shutdown()?;
    Ok(())
}

#[derive(Debug)]
pub struct Server {
    listener: TcpListener,
    tls_config: Option<Arc<ServerConfig>>,
    base_url: Url,
    buf: Vec<u8>,
    last: Instant,
    #[cfg(feature = "labeler")]
    conn: Connection,
    #[cfg(not(feature = "labeler"))]
    relay_conn: Connection,
    admin_conn: Connection,
    request_crawl_tx: RequestCrawlSender,
    subscribe_repos_tx: SubscribeReposSender,
    #[cfg(not(feature = "labeler"))]
    discovery: Option<std::sync::mpsc::Receiver<Vec<String>>>,
    #[cfg(not(feature = "labeler"))]
    discovery_pending: std::collections::VecDeque<String>,
}

impl Server {
    pub fn new(
        ssl_configs: Option<(PathBuf, PathBuf)>, request_crawl_tx: RequestCrawlSender,
        subscribe_repos_tx: SubscribeReposSender,
    ) -> Result<Self, ServerError> {
        Self::with_paths(
            ssl_configs,
            request_crawl_tx,
            subscribe_repos_tx,
            &format!("127.0.0.1:{PORT}"),
            Path::new("relay.db"),
            Path::new("plc_directory.db"),
        )
    }

    #[allow(unused_variables)]
    pub fn with_paths(
        ssl_configs: Option<(PathBuf, PathBuf)>, request_crawl_tx: RequestCrawlSender,
        subscribe_repos_tx: SubscribeReposSender, bind: &str, relay_db: &Path, plc_db: &Path,
    ) -> Result<Self, ServerError> {
        let tls_config = if let Some((certs, private_key)) = ssl_configs {
            let certs = rustls_pemfile::certs(&mut BufReader::new(&mut File::open(certs)?))
                .collect::<Result<Vec<_>, _>>()?;
            #[expect(clippy::expect_used)]
            let private_key =
                rustls_pemfile::private_key(&mut BufReader::new(&mut File::open(private_key)?))?
                    .expect("expected private key");
            let tls_config = rustls::ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(certs, private_key)?;
            Some(Arc::new(tls_config))
        } else {
            None
        };

        let listener = TcpListener::bind(bind)?;
        listener.set_nonblocking(true)?;
        let base_url = Url::parse("http://example.com")?;
        let now = Instant::now();
        let last = now.checked_sub(HOSTS_INTERVAL).unwrap_or(now);
        // Created by `ValidatorManager::new`.
        #[cfg(not(feature = "labeler"))]
        let relay_conn = Connection::open_with_flags(
            relay_db,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        #[cfg(feature = "labeler")]
        let conn = Connection::open_with_flags(
            plc_db,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let admin_conn = Connection::open_with_flags(
            relay_db,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        admin_conn.busy_timeout(Duration::from_secs(5))?;
        Ok(Self {
            listener,
            tls_config,
            base_url,
            buf: vec![0; 1024],
            last,
            #[cfg(feature = "labeler")]
            conn,
            #[cfg(not(feature = "labeler"))]
            relay_conn,
            admin_conn,
            request_crawl_tx,
            subscribe_repos_tx,
            #[cfg(not(feature = "labeler"))]
            discovery: None,
            #[cfg(not(feature = "labeler"))]
            discovery_pending: std::collections::VecDeque::new(),
        })
    }

    pub fn run(mut self) -> Result<(), ServerError> {
        while self.update()? {
            thread::sleep(SLEEP);
        }
        Ok(())
    }

    fn update(&mut self) -> Result<bool, ServerError> {
        if SHUTDOWN.load(Ordering::Relaxed) {
            tracing::info!("shutting down server");
            return Ok(false);
        }

        if self.last.elapsed() > HOSTS_INTERVAL {
            if let Err(err) = self.query_hosts() {
                tracing::warn!(%err, "unable to query hosts");
            }
            self.last = Instant::now();
        }
        #[cfg(not(feature = "labeler"))]
        self.drain_discovery();

        // Drain the accept backlog each tick: one accept per sleep caps intake
        // at ~100/s, which overflows the listen queue whenever every
        // subscriber reconnects at once.
        let mut accepted = 0;
        loop {
            match self.listener.accept() {
                Ok((mut stream, addr)) => {
                    tracing::trace!(%addr, "received request");
                    stream.set_read_timeout(Some(SOCKET_TIMEOUT))?;
                    stream.set_write_timeout(Some(SOCKET_TIMEOUT))?;
                    let stream = if let Some(tls_config) = self.tls_config.clone() {
                        let mut conn = ServerConnection::new(tls_config)?;
                        if let Err(err) = conn.complete_io(&mut stream) {
                            tracing::info!(%addr, %err, "tls handshake error");
                        }
                        let stream = StreamOwned::new(conn, stream);
                        MaybeTlsStream::Rustls(stream)
                    } else {
                        MaybeTlsStream::Plain(stream)
                    };
                    if let Err(err) = self.handle_stream(ErrorOnDropTcpStream(Some(stream)), addr) {
                        tracing::info!(%addr, %err, "invalid request");
                    }
                    accepted += 1;
                    if accepted >= ACCEPTS_PER_TICK {
                        break;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => Err(e)?,
            }
        }

        Ok(true)
    }

    fn handle_stream(&mut self, mut stream: ErrorOnDropTcpStream, addr: SocketAddr) -> Result<()> {
        // only peek to allow tungstenite to complete the handshake
        #[expect(clippy::unwrap_used)]
        let len = stream.0.as_mut().unwrap().peek(&mut self.buf)?;
        let mut headers = [EMPTY_HEADER; 32];
        let mut parser = httparse::Request::new(&mut headers);
        // try parsing as an HTTP request
        let res = parser.parse(&self.buf)?;
        let method = parser.method.ok_or_else(|| eyre!("method missing"))?;
        let path = parser.path.ok_or_else(|| eyre!("path missing"))?;
        // Extract admin auth before the match block so parser's borrow on
        // self.buf is released by NLL before &mut self methods in match arms.
        let is_admin_authed = check_admin_auth(parser.headers);
        let url = Url::options().base_url(Some(&self.base_url)).parse(path)?;

        match (method, url.path()) {
            ("GET", "/_health" | "/xrpc/_health") => {
                let (code, body) = crate::health::health_body();
                let status = if code.is_success() { "200 OK" } else { "503 Service Unavailable" };
                write_json(&mut stream, status, &body)
            }
            ("GET", "/") => write_response(&mut stream, "200 OK", INDEX_ASCII),
            #[cfg(not(feature = "labeler"))]
            ("GET", PATH_LIST_HOSTS) => {
                let (status, body) = match self.list_hosts(&url) {
                    Ok(hosts) => ("200 OK", serde_json::to_string(&hosts)?),
                    Err(e) => {
                        let error = serde_json::json!({
                            "error": "BadRequest",
                            "message": e.to_string(),
                        });
                        ("400 Bad Request", serde_json::to_string(&error)?)
                    }
                };
                write_response(&mut stream, status, &body)
            }
            #[cfg(not(feature = "labeler"))]
            ("GET", PATH_HOST_STATUS) => {
                let (status, body) = match self.host_status(&url) {
                    Ok(hosts) => ("200 OK", serde_json::to_string(&hosts)?),
                    Err(e) => {
                        let error = serde_json::json!({
                            "error": "BadRequest",
                            "message": e.to_string(),
                        });
                        ("400 Bad Request", serde_json::to_string(&error)?)
                    }
                };
                write_response(&mut stream, status, &body)
            }
            ("GET", PATH_SUBSCRIBE) => {
                let mut cursor = None;
                for (key, value) in url.query_pairs() {
                    if key == "cursor" {
                        cursor = u64::from_str(&value).ok();
                    }
                }
                self.subscribe_repos_tx.push(SubscribeRepos {
                    addr,
                    #[expect(clippy::unwrap_used)]
                    stream: stream.0.take().unwrap(),
                    cursor: cursor.map(Into::into),
                })?;
                Ok(())
            }
            ("POST", PATH_REQUEST_CRAWL) => {
                if let Status::Complete(offset) = res {
                    if let Ok(request_crawl) =
                        serde_json::from_reader::<_, RequestCrawl>(&self.buf[offset..len])
                    {
                        if self.is_host_banned(&request_crawl.hostname) {
                            tracing::info!(host = %request_crawl.hostname, "rejecting requestCrawl for banned host");
                            return write_response(
                                &mut stream,
                                "403 Forbidden",
                                "{\"error\":\"Forbidden\",\"message\":\"host is banned\"}",
                            );
                        }
                        self.request_crawl_tx.push(request_crawl)?;
                        return write_response(&mut stream, "200 OK", "");
                    }
                }
                write_response(
                    &mut stream,
                    "400 Bad Request",
                    "{\"error\":\"InvalidRequest\",\"message\":\"invalid or missing hostname\"}",
                )
            }
            ("POST", PATH_ADMIN_BAN | PATH_ADMIN_UNBAN) | ("GET", PATH_ADMIN_LIST_BANS) => {
                self.handle_admin(&mut stream, url.path(), &url, is_admin_authed)
            }
            _ => write_response(
                &mut stream,
                "404 Not Found",
                "{\"error\":\"NotFound\",\"message\":\"endpoint not found\"}",
            ),
        }
    }

    fn handle_admin(
        &self, stream: &mut ErrorOnDropTcpStream, path: &str, url: &Url, is_admin_authed: bool,
    ) -> Result<()> {
        if !is_admin_authed {
            return write_response(
                stream,
                "401 Unauthorized",
                "{\"error\":\"Unauthorized\",\"message\":\"invalid or missing auth\"}",
            );
        }
        match path {
            PATH_ADMIN_BAN | PATH_ADMIN_UNBAN => {
                let Some(hostname) = Self::get_query_param(url, "host") else {
                    return write_response(
                        stream,
                        "400 Bad Request",
                        "{\"error\":\"BadRequest\",\"message\":\"host parameter is required\"}",
                    );
                };
                let is_ban = path == PATH_ADMIN_BAN;
                let result =
                    if is_ban { self.ban_host(&hostname) } else { self.unban_host(&hostname) };
                let (status, body) = match result {
                    Ok(()) => {
                        let body = serde_json::json!({"host": hostname, "banned": is_ban});
                        ("200 OK", serde_json::to_string(&body)?)
                    }
                    Err(e) => {
                        let body =
                            serde_json::json!({"error": "InternalError", "message": e.to_string()});
                        ("500 Internal Server Error", serde_json::to_string(&body)?)
                    }
                };
                write_response(stream, status, &body)
            }
            PATH_ADMIN_LIST_BANS => {
                let (status, body) = match self.list_bans() {
                    Ok(bans) => ("200 OK", serde_json::to_string(&bans)?),
                    Err(e) => {
                        let body =
                            serde_json::json!({"error": "InternalError", "message": e.to_string()});
                        ("500 Internal Server Error", serde_json::to_string(&body)?)
                    }
                };
                write_response(stream, status, &body)
            }
            _ => write_response(
                stream,
                "404 Not Found",
                "{\"error\":\"NotFound\",\"message\":\"endpoint not found\"}",
            ),
        }
    }

    #[cfg(not(feature = "labeler"))]
    fn list_hosts(&self, url: &Url) -> Result<ListHosts> {
        // Default query parameters.
        let mut limit = 200;
        let mut cursor = None;

        for (key, value) in url.query_pairs() {
            match key.as_ref() {
                "limit" => match value.parse::<u16>() {
                    Ok(l @ 1..=1000) => limit = l,
                    _ => {
                        return Err(eyre!("limit parameter invalid or out of range: {value}"));
                    }
                },
                "cursor" => match value.parse::<i64>() {
                    Ok(c) => cursor = Some(c),
                    Err(_) => {
                        return Err(eyre!("cursor parameter invalid: {value}"));
                    }
                },
                // Ignore unknown query parameters.
                _ => (),
            }
        }

        let mut stmt_hosts = self.relay_conn.prepare_cached(
            "SELECT rowid, host, cursor
            FROM hosts
            WHERE :cursor is NULL OR rowid > :cursor
            LIMIT :limit;",
        )?;
        let hosts = stmt_hosts
            .query_map(
                named_params! {
                    ":cursor": cursor,
                    ":limit": limit,
                },
                |row| {
                    Ok((
                        row.get::<_, i64>("rowid")?,
                        row.get::<_, String>("host")?,
                        u64::try_from(row.get::<_, i64>("cursor")?).unwrap_or_default(),
                    ))
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;

        let cursor = hosts.last().map(|(rowid, ..)| rowid.to_string());

        let hosts = hosts
            .into_iter()
            .map(|(_, hostname, seq)| {
                let status = if self.is_host_banned(&hostname) {
                    HostStatus::Banned
                } else {
                    HostStatus::Active
                };
                Host {
                    // TODO: Track host account counts.
                    account_count: 0,
                    hostname,
                    seq,
                    status,
                }
            })
            .collect();

        Ok(ListHosts { cursor, hosts })
    }

    #[cfg(not(feature = "labeler"))]
    fn host_status(&self, url: &Url) -> Result<GetHostStatus> {
        let mut hostname = None;
        for (key, value) in url.query_pairs() {
            // Ignore unknown query parameters.
            if key.as_ref() == "hostname" {
                hostname = Some(value.to_string());
            }
        }
        let hostname = hostname.ok_or_else(|| eyre!("hostname param is required"))?;

        let is_banned = self.is_host_banned(&hostname);
        self.relay_conn
            .prepare_cached("SELECT cursor FROM hosts WHERE host = :host")?
            .query_one(named_params! { ":host": hostname.clone() }, |row| {
                Ok(GetHostStatus {
                    hostname: hostname.clone(),
                    seq: u64::try_from(row.get::<_, i64>("cursor")?).unwrap_or_default(),
                    status: if is_banned { HostStatus::Banned } else { HostStatus::Active },
                })
            })
            .optional()?
            .ok_or_else(|| eyre!("hostname {hostname:?} not found"))
    }

    /// Discovery runs on its own thread: fetching `listHosts` from every upstream
    /// with retries took long enough to stall the accept loop.
    #[cfg(not(feature = "labeler"))]
    fn query_hosts(&mut self) -> Result<()> {
        if self.discovery.is_some() {
            tracing::warn!("previous discovery round still running; skipping");
            return Ok(());
        }
        let client = reqwest::blocking::Client::builder()
            .user_agent("rsky-relay")
            .https_only(true)
            .timeout(DISCOVERY_REQUEST_TIMEOUT)
            .build()?;
        let (tx, rx) = std::sync::mpsc::channel();
        thread::Builder::new().name("rsky-discovery".into()).spawn(move || {
            let mut seen: hashbrown::HashSet<String> = hashbrown::HashSet::new();
            for upstream in HOSTS_RELAYS.iter() {
                let fetcher = ReqwestHostListFetcher {
                    client: client.clone(),
                    base_url: format!("https://{upstream}{PATH_LIST_HOSTS}"),
                };
                let hosts = discover_hosts(&fetcher, thread::sleep, &mut seen, upstream);
                if tx.send(hosts).is_err() {
                    return;
                }
            }
        })?;
        self.discovery = Some(rx);
        Ok(())
    }

    /// Hands discovered hosts to the crawler; whatever does not fit in the ring
    /// this tick stays buffered for the next one instead of being abandoned.
    #[cfg(not(feature = "labeler"))]
    fn drain_discovery(&mut self) {
        let mut finished = false;
        if let Some(rx) = &self.discovery {
            loop {
                match rx.try_recv() {
                    Ok(hosts) => self.discovery_pending.extend(hosts),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        finished = true;
                        break;
                    }
                }
            }
        }
        while let Some(hostname) = self.discovery_pending.pop_front() {
            if self.is_host_banned(&hostname) {
                continue;
            }
            if let Err(rtrb::PushError::Full(request)) =
                self.request_crawl_tx.push(RequestCrawl { hostname, cursor: None })
            {
                self.discovery_pending.push_front(request.hostname);
                break;
            }
        }
        if finished {
            self.discovery = None;
        }
    }

    #[cfg(feature = "labeler")]
    fn query_hosts(&mut self) -> Result<()> {
        let mut stmt =
            self.conn.prepare_cached("SELECT DISTINCT labeler_endpoint FROM plc_labelers")?;
        for res in stmt.query_map([], |row| row.get::<_, String>(0))? {
            if let Some(hostname) = res?.strip_prefix("https://").map(|x| x.trim_end_matches('/')) {
                self.request_crawl_tx
                    .push(RequestCrawl { hostname: hostname.to_owned(), cursor: None })?;
            }
        }
        drop(stmt);
        Ok(())
    }

    fn ban_host(&self, hostname: &str) -> Result<()> {
        self.admin_conn
            .execute("INSERT OR IGNORE INTO banned_hosts (host) VALUES (?1)", [hostname])?;
        tracing::warn!(%hostname, "banned PDS host");
        Ok(())
    }

    fn unban_host(&self, hostname: &str) -> Result<()> {
        self.admin_conn.execute("DELETE FROM banned_hosts WHERE host = ?1", [hostname])?;
        tracing::warn!(%hostname, "unbanned PDS host");
        Ok(())
    }

    fn list_bans(&self) -> Result<ListBans> {
        let mut stmt = self
            .admin_conn
            .prepare_cached("SELECT host, created_at FROM banned_hosts ORDER BY created_at")?;
        let banned_hosts = stmt
            .query_map([], |row| {
                Ok(BannedHost { host: row.get("host")?, created_at: row.get("created_at")? })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ListBans { banned_hosts })
    }

    fn is_host_banned(&self, hostname: &str) -> bool {
        self.admin_conn
            .prepare_cached("SELECT 1 FROM banned_hosts WHERE host = ?1")
            .and_then(|mut stmt| stmt.exists([hostname]))
            .unwrap_or(false)
    }

    fn get_query_param(url: &Url, key: &str) -> Option<String> {
        url.query_pairs().find(|(k, _)| k == key).map(|(_, v)| v.to_string())
    }
}

fn check_admin_auth(headers: &[httparse::Header<'_>]) -> bool {
    let Some(password) = ADMIN_PASSWORD.as_ref() else {
        return false;
    };
    headers.iter().any(|h| {
        h.name.eq_ignore_ascii_case("Authorization")
            && std::str::from_utf8(h.value)
                .ok()
                .and_then(|v| v.strip_prefix("Bearer "))
                .is_some_and(|token| token == password.as_str())
    })
}

#[cfg(all(test, not(feature = "labeler")))]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct ScriptedFetcher {
        script: Vec<Result<ListHosts, &'static str>>,
        idx: Cell<usize>,
    }

    impl ScriptedFetcher {
        const fn new(script: Vec<Result<ListHosts, &'static str>>) -> Self {
            Self { script, idx: Cell::new(0) }
        }
        fn calls(&self) -> usize {
            self.idx.get()
        }
    }

    impl HostListFetcher for ScriptedFetcher {
        fn fetch_page(&self, _cursor: Option<&str>) -> Result<ListHosts> {
            let i = self.idx.get();
            self.idx.set(i + 1);
            let entry = self.script.get(i).ok_or_else(|| eyre!("script exhausted"))?;
            match entry {
                Ok(page) => Ok(ListHosts {
                    cursor: page.cursor.clone(),
                    hosts: page
                        .hosts
                        .iter()
                        .map(|h| Host {
                            account_count: h.account_count,
                            hostname: h.hostname.clone(),
                            seq: h.seq,
                            status: match h.status {
                                HostStatus::Active => HostStatus::Active,
                                HostStatus::Idle => HostStatus::Idle,
                                HostStatus::Offline => HostStatus::Offline,
                                HostStatus::Throttled => HostStatus::Throttled,
                                HostStatus::Banned => HostStatus::Banned,
                            },
                        })
                        .collect(),
                }),
                Err(msg) => Err(eyre!(*msg)),
            }
        }
    }

    fn page(cursor: Option<&str>, hosts: Vec<&str>) -> ListHosts {
        ListHosts {
            cursor: cursor.map(str::to_owned),
            hosts: hosts
                .into_iter()
                .map(|h| Host {
                    account_count: 1,
                    hostname: h.to_owned(),
                    seq: 0,
                    status: HostStatus::Active,
                })
                .collect(),
        }
    }

    #[test]
    fn fetch_page_with_retry_succeeds_on_first_try() {
        let fetcher = ScriptedFetcher::new(vec![Ok(page(None, vec!["a", "b"]))]);
        let res = fetch_page_with_retry(&fetcher, None, |_| {});
        assert!(res.is_ok());
        assert_eq!(fetcher.calls(), 1);
    }

    #[test]
    fn fetch_page_with_retry_succeeds_after_transient_failure() {
        let fetcher = ScriptedFetcher::new(vec![Err("transient"), Ok(page(None, vec!["a"]))]);
        let collector = std::cell::RefCell::new(Vec::<Duration>::new());
        let sleep_fn = |d: Duration| collector.borrow_mut().push(d);
        let res = fetch_page_with_retry(&fetcher, None, sleep_fn);
        assert!(res.is_ok());
        assert_eq!(fetcher.calls(), 2);
        assert_eq!(collector.borrow().len(), 1);
    }

    #[test]
    fn fetch_page_with_retry_returns_err_after_exhausting_attempts() {
        let fetcher = ScriptedFetcher::new(vec![Err("e1"), Err("e2"), Err("e3")]);
        let res = fetch_page_with_retry(&fetcher, None, |_| {});
        assert!(res.is_err());
        assert_eq!(fetcher.calls(), 3);
    }

    #[test]
    fn fetch_page_with_retry_doubles_backoff_between_attempts() {
        let fetcher = ScriptedFetcher::new(vec![Err("a"), Err("b"), Err("c")]);
        let collector = std::cell::RefCell::new(Vec::<Duration>::new());
        let sleep_fn = |d: Duration| collector.borrow_mut().push(d);
        drop(fetch_page_with_retry(&fetcher, None, sleep_fn));
        let sleeps = collector.borrow();
        assert_eq!(sleeps.len(), 2, "sleep between attempts only");
        assert_eq!(sleeps[0], Duration::from_secs(1));
        assert_eq!(sleeps[1], Duration::from_secs(2));
    }

    #[test]
    fn discover_hosts_filters_sorts_and_dedupes() {
        let fetcher = ScriptedFetcher::new(vec![
            Ok(ListHosts {
                cursor: Some("1".to_owned()),
                hosts: vec![
                    Host {
                        account_count: 5,
                        hostname: "small".to_owned(),
                        seq: 1,
                        status: HostStatus::Active,
                    },
                    Host {
                        account_count: 50,
                        hostname: "big".to_owned(),
                        seq: 1,
                        status: HostStatus::Idle,
                    },
                    Host {
                        account_count: 0,
                        hostname: "empty".to_owned(),
                        seq: 1,
                        status: HostStatus::Active,
                    },
                    Host {
                        account_count: 9,
                        hostname: "off".to_owned(),
                        seq: 1,
                        status: HostStatus::Offline,
                    },
                ],
            }),
            Ok(ListHosts {
                cursor: None,
                hosts: vec![Host {
                    account_count: 7,
                    hostname: "big".to_owned(),
                    seq: 2,
                    status: HostStatus::Active,
                }],
            }),
        ]);
        let mut seen = hashbrown::HashSet::new();
        let hosts = discover_hosts(&fetcher, |_| {}, &mut seen, "up");
        assert_eq!(hosts, vec!["big".to_owned(), "small".to_owned()]);
        assert_eq!(fetcher.calls(), 2);
        // a failing upstream yields partial or empty results without panicking
        let failing = ScriptedFetcher::new(vec![Err("boom"), Err("boom"), Err("boom")]);
        let mut seen = hashbrown::HashSet::new();
        assert!(discover_hosts(&failing, |_| {}, &mut seen, "down").is_empty());
    }

    fn test_server() -> (Server, rtrb::Consumer<RequestCrawl>, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let relay_db = tmp.path().join("relay.db");
        Connection::open(&relay_db)
            .unwrap()
            .execute_batch(
                "CREATE TABLE hosts (host TEXT PRIMARY KEY, cursor INTEGER NOT NULL, latest TEXT NOT NULL);
                 CREATE TABLE banned_hosts (host TEXT PRIMARY KEY, created_at TEXT NOT NULL DEFAULT (datetime('now')));",
            )
            .unwrap();
        let (request_crawl_tx, request_crawl_rx) = rtrb::RingBuffer::new(2);
        let (subscribe_repos_tx, _subscribe_repos_rx) = rtrb::RingBuffer::new(2);
        let server = Server::with_paths(
            None,
            request_crawl_tx,
            subscribe_repos_tx,
            "127.0.0.1:0",
            &relay_db,
            &tmp.path().join("plc.db"),
        )
        .unwrap();
        (server, request_crawl_rx, tmp)
    }

    #[test]
    fn drain_discovery_buffers_overflow_and_skips_banned_hosts() {
        let _lock = SERVER_TEST_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (mut server, mut rx, _tmp) = test_server();
        server.drain_discovery();
        let (tx, rx_hosts) = std::sync::mpsc::channel();
        server.discovery = Some(rx_hosts);
        server.ban_host("banned").unwrap();
        tx.send(vec!["banned".to_owned(), "a".to_owned(), "b".to_owned(), "c".to_owned()]).unwrap();
        server.drain_discovery();
        assert_eq!(rx.pop().unwrap().hostname, "a");
        assert_eq!(rx.pop().unwrap().hostname, "b");
        assert!(rx.pop().is_err(), "ring of two holds two");
        assert_eq!(server.discovery_pending.len(), 1, "c waits for the next tick");
        assert!(server.discovery.is_some(), "sender still alive");
        drop(tx);
        server.drain_discovery();
        assert_eq!(rx.pop().unwrap().hostname, "c");
        assert!(server.discovery.is_none(), "finished round is released");
        server.unban_host("banned").unwrap();
    }

    #[test]
    fn query_hosts_spawns_one_round_at_a_time() {
        let _lock = SERVER_TEST_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (mut server, _rx, _tmp) = test_server();
        let (_tx, rx_hosts) = std::sync::mpsc::channel();
        server.discovery = Some(rx_hosts);
        server.query_hosts().unwrap();
        assert!(server.discovery.is_some());
    }

    #[test]
    fn health_route_returns_json_with_validator_state() {
        let _lock = SERVER_TEST_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (mut server, _rx, _tmp) = test_server();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = thread::spawn(move || {
            let mut stream = TcpStream::connect(addr).unwrap();
            stream.write_all(b"GET /xrpc/_health HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
            let mut body = String::new();
            std::io::Read::read_to_string(&mut stream, &mut body).unwrap();
            body
        });
        let (stream, peer) = listener.accept().unwrap();
        server
            .handle_stream(ErrorOnDropTcpStream(Some(MaybeTlsStream::Plain(stream))), peer)
            .unwrap();
        let body = client.join().unwrap();
        assert!(body.contains("application/json"), "{body}");
        assert!(body.contains("\"validator\""), "{body}");
    }

    static SERVER_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
}
