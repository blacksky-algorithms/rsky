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
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use thiserror::Error;
use url::Url;

use crate::SHUTDOWN;
use crate::crawler::{RequestCrawl, RequestCrawlSender};
use crate::publisher::{MaybeTlsStream, SubscribeRepos, SubscribeReposSender};
use crate::server::types::{HostStatus, ListHosts};

const SLEEP: Duration = Duration::from_millis(10);

const HOSTS_INTERVAL: Duration = Duration::from_secs(60 * 60);
const HOSTS_TIMEOUT: Duration = Duration::from_secs(30);

const BSKY_RELAY: &str = "relay1.us-west.bsky.network";
const LIST_HOSTS: &str = "xrpc/com.atproto.sync.listHosts";

const INDEX_ASCII: &str = r#"
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
"#;

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

        let listener = TcpListener::bind("127.0.0.1:9000")?;
        listener.set_nonblocking(true)?;
        let base_url = Url::parse("http://example.com")?;
        let now = Instant::now();
        let last = now.checked_sub(HOSTS_INTERVAL).unwrap_or(now);
        Ok(Self {
            listener,
            tls_config,
            base_url,
            buf: vec![0; 1024],
            last,
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
                tracing::info!(%err, "unable to query hosts");
            }
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
        let mut headers = [EMPTY_HEADER; 16];
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
                return Ok(());
            }
            ("GET", "/xrpc/com.atproto.sync.subscribeRepos") => {
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
            ("POST", "/xrpc/com.atproto.sync.requestCrawl") => {
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

    fn query_hosts(&mut self) -> Result<()> {
        let client = reqwest::blocking::Client::builder()
            .user_agent("rsky-relay")
            .timeout(HOSTS_TIMEOUT)
            .https_only(true)
            .build()?;
        let mut cursor: Option<String> = None;
        loop {
            let mut params = vec![("limit", "1000")];
            if let Some(cursor) = &cursor {
                params.push(("cursor", cursor));
            }
            let url =
                Url::parse_with_params(&format!("https://{BSKY_RELAY}/{LIST_HOSTS}"), params)?;
            let hosts: ListHosts = client.get(url).send()?.json()?;
            for host in hosts.hosts {
                if host.account_count > 100
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
        self.last = Instant::now();
        Ok(())
    }
}
