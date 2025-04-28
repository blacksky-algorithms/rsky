use std::io;
use std::net::{SocketAddr, TcpStream};
use std::os::fd::{AsRawFd, RawFd};

use sled::Tree;
use thiserror::Error;
use tungstenite::handshake::server::NoCallback;
use tungstenite::protocol::CloseFrame;
use tungstenite::protocol::frame::coding::CloseCode;
use tungstenite::{Bytes, HandshakeError, Message, ServerHandshake, Utf8Bytes, WebSocket};

use crate::publisher::types::MaybeTlsStream;
use crate::types::Cursor;

const OUTDATED_MSG: &[u8] = b"\xa2ate#infobop\x01\xa2dnamenOutdatedCursorgmessagex8Requested cursor exceeded limit. Possibly missing events.";
const FUTURE_MSG: &[u8] = b"\xa1bop \xa2eerrorlFutureCursorgmessageuCursor in the future.";
const FUTURE_CLOSE: CloseFrame =
    CloseFrame { code: CloseCode::Policy, reason: Utf8Bytes::from_static("FutureCursor") };
const SHUTDOWN_FRAME: CloseFrame =
    CloseFrame { code: CloseCode::Restart, reason: Utf8Bytes::from_static("RelayRestart") };

#[derive(Debug, Error)]
pub enum ConnectionError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("handshake error: {0}")]
    Handshake(#[from] HandshakeError<ServerHandshake<MaybeTlsStream<TcpStream>, NoCallback>>),
    #[error("tungstenite error: {0}")]
    Tungstenite(#[from] tungstenite::Error),
    #[error("sled error: {0}")]
    Sled(#[from] sled::Error),
}

pub struct Connection {
    pub(crate) addr: SocketAddr,
    client: WebSocket<MaybeTlsStream<TcpStream>>,
    cursor: Cursor,
}

impl AsRawFd for Connection {
    #[inline]
    fn as_raw_fd(&self) -> RawFd {
        match self.client.get_ref() {
            MaybeTlsStream::Plain(stream) => stream.as_raw_fd(),
            MaybeTlsStream::Rustls(stream) => stream.get_ref().as_raw_fd(),
        }
    }
}

impl Connection {
    pub fn connect(
        addr: SocketAddr, stream: MaybeTlsStream<TcpStream>, cursor: Cursor,
    ) -> Result<Self, ConnectionError> {
        let client = tungstenite::accept(stream)?;
        match client.get_ref() {
            MaybeTlsStream::Rustls(stream) => {
                stream.get_ref().set_nonblocking(true)?;
            }
            MaybeTlsStream::Plain(stream) => {
                stream.set_nonblocking(true)?;
            }
        }
        Ok(Self { addr, client, cursor })
    }

    pub fn close(&mut self, code: CloseFrame) -> Result<(), ConnectionError> {
        self.client.close(Some(code))?;
        self.client.flush()?;
        Ok(())
    }

    /// false: not sent
    /// true: sent
    pub fn send(&mut self, mut seq: Cursor, data: Bytes) -> Result<bool, ConnectionError> {
        if self.cursor != seq {
            return Ok(false);
        }
        match self.client.send(Message::Binary(data)) {
            Ok(()) => {
                self.cursor = seq.next();
                Ok(true)
            }
            Err(tungstenite::Error::Io(e)) if e.kind() == io::ErrorKind::WouldBlock => {
                self.cursor = seq.next();
                Ok(false)
            }
            Err(tungstenite::Error::WriteBufferFull(_)) => Ok(false),
            Err(err) => Err(err)?,
        }
    }

    /// false: closed
    /// true: not closed
    pub fn poll(&mut self, mut seq: Cursor, firehose: &Tree) -> Result<bool, ConnectionError> {
        if self.cursor.get() != 0 && self.cursor.get() > seq.get() + 1 {
            self.send(self.cursor, Bytes::from_static(FUTURE_MSG))?;
            self.close(FUTURE_CLOSE)?;
            return Ok(false);
        }
        for msg in firehose.range(self.cursor..=seq) {
            let (k, v) = msg?;
            seq = k.into();
            if self.cursor != seq {
                if self.cursor.get() != 0 {
                    self.send(self.cursor, Bytes::from_static(OUTDATED_MSG))?;
                }
                self.cursor = seq;
            }
            if !self.send(seq, Bytes::from_owner(v).slice(8..))? {
                break;
            }
        }
        Ok(true)
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        drop(self.close(SHUTDOWN_FRAME));
    }
}
