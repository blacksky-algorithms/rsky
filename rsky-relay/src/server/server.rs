use std::fs::File;
use std::io::{self, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
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
use rusqlite::{Connection, OpenFlags};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use thiserror::Error;
use url::Url;

use crate::SHUTDOWN;
use crate::config::{HOSTS_INTERVAL, PORT};
#[cfg(not(feature = "labeler"))]
use crate::config::{HOSTS_MIN_ACCOUNTS, HOSTS_RELAY};
use crate::crawler::{RequestCrawl, RequestCrawlSender};
use crate::publisher::{MaybeTlsStream, SubscribeRepos, SubscribeReposSender};
#[cfg(not(feature = "labeler"))]
use crate::server::types::{Host, HostStatus, ListHosts};

const SLEEP: Duration = Duration::from_millis(10);

#[cfg(not(feature = "labeler"))]
const PATH_LIST_HOSTS: &str = "/xrpc/com.atproto.sync.listHosts";

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
    request_crawl_tx: RequestCrawlSender,
    subscribe_repos_tx: SubscribeReposSender,
}

impl Server {
    pub fn new(
        ssl_configs: Option<(PathBuf, PathBuf)>, request_crawl_tx: RequestCrawlSender,
        subscribe_repos_tx: SubscribeReposSender,
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

        let listener = TcpListener::bind(format!("127.0.0.1:{PORT}"))?;
        listener.set_nonblocking(true)?;
        let base_url = Url::parse("http://example.com")?;
        let now = Instant::now();
        let last = now.checked_sub(HOSTS_INTERVAL).unwrap_or(now);
        // Created by `ValidatorManager::new`.
        #[cfg(not(feature = "labeler"))]
        let relay_conn = Connection::open_with_flags(
            "relay.db",
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        #[cfg(feature = "labeler")]
        let conn = Connection::open_with_flags(
            "plc_directory.db",
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
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
            request_crawl_tx,
            subscribe_repos_tx,
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

        match self.listener.accept() {
            Ok((mut stream, addr)) => {
                tracing::trace!(%addr, "received request");
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
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                return Ok(true);
            }
            Err(e) => Err(e)?,
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
        let url = Url::options().base_url(Some(&self.base_url)).parse(path)?;
        match (method, url.path()) {
            ("GET", "/") => {
                let body = INDEX_ASCII;
                let response = format!(
                    "HTTP/1.1 200 OK\r\n\
                     Content-Type: text/plain; charset=utf-8\r\n\
                     Content-Length: {}\r\n\
                     Connection: close\r\n\
                     \r\n\
                     {}",
                    body.len(),
                    body
                );

                #[expect(clippy::unwrap_used)]
                let mut stream = stream.0.take().unwrap();
                stream.write_all(response.as_bytes())?;
                stream.flush()?;
                stream.shutdown()?;
                Ok(())
            }
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

                let response = format!(
                    "HTTP/1.1 {}\r\n\
                     Content-Type: application/json; charset=utf-8\r\n\
                     Content-Length: {}\r\n\
                     Connection: close\r\n\
                     \r\n\
                     {}",
                    status,
                    body.len(),
                    body
                );

                #[expect(clippy::unwrap_used)]
                let mut stream = stream.0.take().unwrap();
                stream.write_all(response.as_bytes())?;
                stream.flush()?;
                stream.shutdown()?;
                Ok(())
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
                        self.request_crawl_tx.push(request_crawl)?;
                        #[expect(clippy::unwrap_used)]
                        let mut stream = stream.0.take().unwrap();
                        stream.write_all(b"HTTP/1.1 200 OK\n")?;
                        stream.flush()?;
                        stream.shutdown()?;
                        return Ok(());
                    }
                }

                Err(eyre!("unknown hostname"))
            }
            _ => Err(eyre!("unknown request")),
        }
    }

    #[cfg(not(feature = "labeler"))]
    fn list_hosts(&mut self, url: &Url) -> Result<ListHosts> {
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
                |row| Ok((row.get::<_, i64>("rowid")?, row.get("host")?, row.get("cursor")?)),
            )?
            .collect::<Result<Vec<_>, _>>()?;

        let cursor = hosts.last().map(|(rowid, ..)| rowid.to_string());

        let hosts = hosts
            .into_iter()
            .map(|(_, hostname, seq)| Host {
                // TODO: Track host account counts.
                account_count: 0,
                hostname,
                seq,
                // TODO: Track status of hosts.
                status: HostStatus::Active,
            })
            .collect();

        Ok(ListHosts { cursor, hosts })
    }

    #[cfg(not(feature = "labeler"))]
    fn query_hosts(&mut self) -> Result<()> {
        let client = reqwest::blocking::Client::builder()
            .user_agent("rsky-relay")
            .https_only(true)
            .build()?;
        let mut cursor: Option<String> = None;
        loop {
            let mut params = vec![("limit", "1000")];
            if let Some(cursor) = &cursor {
                params.push(("cursor", cursor));
            }
            let url =
                Url::parse_with_params(&format!("https://{HOSTS_RELAY}{PATH_LIST_HOSTS}"), params)?;
            let mut hosts: ListHosts = client.get(url).send()?.json()?;
            hosts.hosts.sort_unstable_by_key(|host| host.account_count);
            for host in hosts.hosts.into_iter().rev() {
                if host.account_count > HOSTS_MIN_ACCOUNTS
                    && matches!(host.status, HostStatus::Active | HostStatus::Idle)
                {
                    self.request_crawl_tx
                        .push(RequestCrawl { hostname: host.hostname, cursor: None })?;
                }
            }
            cursor = hosts.cursor;
            if cursor.is_none() {
                break;
            }
        }
        Ok(())
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
}
