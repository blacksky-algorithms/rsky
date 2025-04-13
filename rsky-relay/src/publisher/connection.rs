use std::io;
use std::net::{SocketAddr, TcpStream};
use std::os::fd::{AsRawFd, RawFd};

use sled::Tree;
use thiserror::Error;
use tungstenite::handshake::server::NoCallback;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Bytes, HandshakeError, Message, ServerHandshake, WebSocket};

use crate::types::Cursor;

const OUTDATED: &[u8] = b"\xa2ate#infobop\x01\xa2dnamenOutdatedCursorgmessagex8Requested cursor exceeded limit. Possibly missing events.";
const FUTURE: &[u8] = b"\xa1bop \xa2eerrorlFutureCursorgmessageuCursor in the future.";

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
            _ => todo!(),
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
            _ => {}
        }
        Ok(Self { addr, client, cursor })
    }

    pub fn close(&mut self) -> Result<(), ConnectionError> {
        self.client.close(None)?;
        self.client.flush()?;
        Ok(())
    }

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

    pub fn poll(&mut self, mut seq: Cursor, firehose: &Tree) -> Result<(), ConnectionError> {
        if self.cursor.get() > seq.get() {
            self.send(self.cursor, Bytes::from_static(FUTURE))?;
            return self.close();
        }
        for msg in firehose.range(self.cursor..=seq) {
            let (k, v) = msg?;
            seq = k.into();
            if self.cursor != seq {
                self.send(self.cursor, Bytes::from_static(OUTDATED))?;
                self.cursor = seq;
            }
            if !self.send(seq, Bytes::from_owner(v).slice(8..))? {
                break;
            }
        }
        Ok(())
    }
}
