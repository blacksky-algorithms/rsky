use std::io::{self, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::str::FromStr;
use std::sync::atomic::Ordering;
use std::thread;

use bstr::BStr;
use color_eyre::Result;
use color_eyre::eyre::eyre;
use http::Uri;
use httparse::{EMPTY_HEADER, Status};
use serde::Deserialize;
use thiserror::Error;
use tungstenite::stream::MaybeTlsStream;
use url::Url;

use crate::types::{Cursor, RequestCrawlSender, SubscribeRepos, SubscribeReposSender};
use crate::{RequestCrawl, SHUTDOWN};

#[derive(Debug, Error)]
pub enum ServerError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("url parse error: {0}")]
    UrlParse(#[from] url::ParseError),
}

#[derive(Debug, Deserialize)]
struct RequestCrawlParams {
    hostname: String,
}

#[derive(Debug)]
struct ErrorOnDropTcpStream(Option<TcpStream>);

impl Drop for ErrorOnDropTcpStream {
    #[cold]
    fn drop(&mut self) {
        let Some(mut stream) = self.0.take() else {
            return;
        };
        let _err = stream.write_all(b"HTTP/1.1 400 Bad Request\n");
        let _err = stream.flush();
        let _err = stream.shutdown(Shutdown::Both);
    }
}

#[derive(Debug)]
pub struct Server {
    listener: TcpListener,
    base_url: Url,
    buf: Vec<u8>,
    request_crawl_tx: RequestCrawlSender,
    subscribe_repos_tx: SubscribeReposSender,
}

impl Server {
    pub fn new(
        request_crawl_tx: RequestCrawlSender, subscribe_repos_tx: SubscribeReposSender,
    ) -> Result<Self, ServerError> {
        let listener = TcpListener::bind("127.0.0.1:9000")?;
        listener.set_nonblocking(true)?;
        let base_url = Url::parse("http://example.com")?;
        Ok(Server { listener, base_url, buf: vec![0; 1024], request_crawl_tx, subscribe_repos_tx })
    }

    pub fn run(mut self) -> Result<(), ServerError> {
        while self.update()? {
            thread::yield_now();
        }
        Ok(())
    }

    fn update(&mut self) -> Result<bool, ServerError> {
        if SHUTDOWN.load(Ordering::Relaxed) {
            tracing::debug!("shutting down server");
            return Ok(false);
        }

        let stream = match self.listener.accept() {
            Ok((stream, addr)) => {
                tracing::trace!("received request from: {addr}");
                stream
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                return Ok(true);
            }
            Err(e) => Err(e)?,
        };

        // TODO: TLS support
        if let Err(err) = self.handle_stream(ErrorOnDropTcpStream(Some(stream))) {
            tracing::info!("invalid request: {err:?}");
        }
        Ok(true)
    }

    fn handle_stream(&mut self, mut stream: ErrorOnDropTcpStream) -> Result<()> {
        // only peek to allow tungstenite to complete the handshake
        let len = stream.0.as_ref().unwrap().peek(&mut self.buf)?;
        let mut headers = [EMPTY_HEADER; 16];
        let mut parser = httparse::Request::new(&mut headers);
        // try parsing as an HTTP request
        let res = parser.parse(&self.buf)?;
        let method = parser.method.ok_or(eyre!("method missing"))?;
        let path = parser.path.ok_or(eyre!("path missing"))?;
        let url = Url::options().base_url(Some(&self.base_url)).parse(path)?;
        match (method, url.path()) {
            ("GET", "/xrpc/com.atproto.sync.subscribeRepos") => {
                let mut cursor = None;
                for (key, value) in url.query_pairs() {
                    if key == "cursor" {
                        cursor = u64::from_str(&value).ok();
                    }
                }

                tracing::debug!("received subscribeRepos: {cursor:?}");
                self.subscribe_repos_tx.push(SubscribeRepos {
                    stream: MaybeTlsStream::Plain(stream.0.take().unwrap()),
                    cursor: Cursor(cursor.unwrap_or_default()),
                })?;
                Ok(())
            }
            ("POST", "/xrpc/com.atproto.sync.requestCrawl") => {
                let mut hostname = None;
                tracing::trace!("requestCrawl: {res:?}");
                if let Status::Complete(offset) = res {
                    if let Ok(params) =
                        serde_json::from_reader::<_, RequestCrawlParams>(&self.buf[offset..len])
                    {
                        hostname = Some(params.hostname);
                    } else {
                        tracing::debug!("invalid body: {}", BStr::new(&self.buf[offset..len]));
                    }
                }
                for (key, value) in url.query_pairs() {
                    if key == "hostname" {
                        hostname = Some(value.into());
                    }
                }

                if let Some(hostname) = hostname {
                    if let Ok(uri) = Uri::from_str(&format!(
                        "wss://{hostname}/xrpc/com.atproto.sync.subscribeRepos"
                    )) {
                        tracing::debug!("received requestCrawl: {hostname}");
                        self.request_crawl_tx.push(RequestCrawl { uri })?;
                        let mut stream = stream.0.take().unwrap();
                        stream.write_all(b"HTTP/1.1 200 OK\n")?;
                        stream.flush()?;
                        stream.shutdown(Shutdown::Both)?;
                        return Ok(());
                    }
                }
                Err(eyre!("unknown hostname"))
            }
            _ => Err(eyre!("unknown request")),
        }
    }
}
