use std::io::Write;
use std::net::{Shutdown, TcpListener};
use std::str::FromStr;
use std::sync::Arc;
use std::{io, thread};

use bus::Bus;
use color_eyre::Result;
use http::Uri;
use httparse::{EMPTY_HEADER, Status};
use serde::Deserialize;
use tungstenite::Message;
use tungstenite::handshake::headers::MAX_HEADERS;
use url::Url;

use rsky_relay::{CrawlRequest, MessageRecycle, client};

const CAPACITY1: usize = 1 << 20;
const CAPACITY2: usize = 1 << 10;
const WORKERS: usize = 4;

pub fn main() -> Result<()> {
    let (message_tx, message_rx) =
        thingbuf::mpsc::blocking::with_recycle(CAPACITY1, MessageRecycle);
    let (mut request_tx, request_rx) = rtrb::RingBuffer::new(CAPACITY2);
    let client = client::Manager::new(WORKERS, message_tx, request_rx)?;
    thread::scope(move |s| {
        s.spawn(move || client.run());
        s.spawn(move || -> Result<()> {
            #[derive(Debug, Deserialize)]
            struct Params {
                hostname: String,
            }
            let dummy = Some(Url::parse("http://example.com")?);
            let listener = TcpListener::bind("127.0.0.1:9000")?;
            listener.set_nonblocking(true)?;
            let mut bus = Bus::new(CAPACITY1);
            loop {
                for _ in 0..1024 {
                    match message_rx.try_recv_ref() {
                        Ok(msg) => {
                            bus.try_broadcast(Arc::new(msg.data.clone())).unwrap();
                        }
                        Err(thingbuf::mpsc::errors::TryRecvError::Closed) => break,
                        Err(_) => (),
                    }
                }
                let mut stream = match listener.accept() {
                    Ok((stream, _)) => stream,
                    Err(err) if err.kind() == io::ErrorKind::WouldBlock => continue,
                    Err(err) => Err(err)?,
                };
                let mut buf = [0; 1024];
                let len = stream.peek(&mut buf)?;
                let mut headers = [EMPTY_HEADER; MAX_HEADERS];
                let mut parser = httparse::Request::new(&mut headers);
                let res = parser.parse(&buf)?;
                let (Some(method), Some(path)) = (parser.method, parser.path) else {
                    continue;
                };
                let Ok(url) = Url::options().base_url(dummy.as_ref()).parse(path) else {
                    continue;
                };
                match (method, url.path()) {
                    ("GET", "/xrpc/com.atproto.sync.subscribeRepos") => {
                        let mut cursor = None;
                        for (key, value) in url.query_pairs() {
                            if key == "cursor" {
                                cursor = u64::from_str(&value).ok();
                            }
                        }
                        stream.set_nonblocking(true)?;
                        let mut client = tungstenite::accept(stream)?;
                        client.send(Message::text(format!("{cursor:?}")))?;
                        let mut reader = bus.add_rx();
                        s.spawn(move || -> Result<()> {
                            loop {
                                match reader.try_recv() {
                                    Ok(msg) => {
                                        client.send(Message::binary(Arc::unwrap_or_clone(msg)))?;
                                    }
                                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                                        break;
                                    }
                                    Err(_) => (),
                                }
                            }
                            Ok(())
                        });
                        continue;
                    }
                    ("POST", "/xrpc/com.atproto.sync.requestCrawl") => {
                        let mut uri = None;
                        if let Status::Complete(offset) = res {
                            if let Ok(params) =
                                serde_json::from_reader::<_, Params>(&buf[offset..len])
                            {
                                uri = Some(params.hostname);
                            }
                        }
                        for (key, value) in url.query_pairs() {
                            if key == "hostname" {
                                uri = Some(value.into());
                            }
                        }
                        if let Some(uri) = uri {
                            if let Ok(uri) = Uri::from_str(&uri) {
                                request_tx.push(CrawlRequest { uri })?;
                                stream.write_all(b"HTTP/1.1 200 OK\n")?;
                                stream.flush()?;
                                stream.shutdown(Shutdown::Both)?;
                                continue;
                            }
                        }
                    }
                    _ => (),
                }
            }
        });
        Ok(())
    })
}
