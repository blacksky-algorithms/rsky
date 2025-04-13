use std::io;
use std::net::TcpStream;
use std::os::fd::{AsRawFd, RawFd};

use thingbuf::mpsc;
use thiserror::Error;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};
use url::Url;

use crate::types::{Cursor, MessageSender};

#[derive(Debug, Error)]
pub enum ConnectionError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("tungstenite error: {0}")]
    Tungstenite(#[from] tungstenite::Error),
    #[error("thingbuf error: {0}")]
    Thingbuf(#[from] mpsc::errors::TrySendError),
}

pub struct Connection {
    pub(crate) hostname: String,
    client: WebSocket<MaybeTlsStream<TcpStream>>,
    message_tx: MessageSender,
}

impl AsRawFd for Connection {
    #[inline]
    fn as_raw_fd(&self) -> RawFd {
        match self.client.get_ref() {
            MaybeTlsStream::Plain(stream) => stream.as_raw_fd(),
            MaybeTlsStream::Rustls(stream) => stream.get_ref().as_raw_fd(),
            _ => todo!(),
        }
    }
}

impl Connection {
    pub fn connect(
        hostname: String, cursor: Option<Cursor>, message_tx: MessageSender,
    ) -> Result<Self, ConnectionError> {
        #[expect(clippy::unwrap_used)]
        let mut url =
            Url::parse(&format!("wss://{hostname}/xrpc/com.atproto.sync.subscribeRepos")).unwrap();
        if let Some(cursor) = cursor {
            url.query_pairs_mut().append_pair("cursor", &cursor.to_string());
        }
        let (client, _) = tungstenite::connect(url)?;
        match client.get_ref() {
            MaybeTlsStream::Rustls(stream) => {
                stream.get_ref().set_nonblocking(true)?;
            }
            MaybeTlsStream::Plain(stream) => {
                stream.set_nonblocking(true)?;
            }
            _ => {}
        }
        Ok(Self { hostname, client, message_tx })
    }

    pub fn close(&mut self) -> Result<(), ConnectionError> {
        self.client.close(None)?;
        self.client.flush()?;
        Ok(())
    }

    pub fn poll(&mut self) -> Result<(), ConnectionError> {
        for _ in 0..1024 {
            let msg = match self.client.read() {
                Ok(msg) => msg,
                Err(tungstenite::Error::Io(e)) if e.kind() == io::ErrorKind::WouldBlock => {
                    break;
                }
                Err(err) => Err(err)?,
            };

            let bytes = match msg {
                Message::Binary(bytes) => bytes,
                Message::Ping(_) | Message::Pong(_) => {
                    continue;
                }
                Message::Close(close) => {
                    tracing::debug!("[{}] received close: {close:?}", self.hostname);
                    continue;
                }
                _ => {
                    tracing::debug!("[{}] unknown message: {msg:?}", self.hostname);
                    continue;
                }
            };

            let mut slot = self.message_tx.try_send_ref()?;
            slot.data = bytes;
            slot.hostname.clone_from(&self.hostname);
        }
        Ok(())
    }
}
