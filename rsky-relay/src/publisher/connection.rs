use std::io;
use std::net::{SocketAddr, TcpStream};
use std::os::fd::{AsRawFd, RawFd};

use fjall::PartitionHandle;
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
    #[error("fjall error: {0}")]
    Fjall(#[from] fjall::Error),
}

pub struct Connection {
    pub(crate) addr: SocketAddr,
    client: WebSocket<MaybeTlsStream<TcpStream>>,
    pub(crate) cursor: Cursor,
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

    /// `Ok(true)` = delivered, cursor advanced. `Ok(false)` = backpressured / cursor mismatch, no advance.
    pub fn send(&mut self, seq: Cursor, data: Bytes) -> Result<bool, ConnectionError> {
        if self.cursor != seq {
            return Ok(false);
        }
        match self.client.send(Message::Binary(data)) {
            Ok(()) => {
                self.cursor = seq.successor();
                Ok(true)
            }
            Err(err) if is_backpressure(&err) => Ok(false),
            Err(err) => Err(err)?,
        }
    }

    /// false: closed
    /// true: not closed
    pub fn poll(
        &mut self, mut seq: Cursor, firehose: &PartitionHandle,
    ) -> Result<bool, ConnectionError> {
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
            if !self.send(seq, Bytes::from_owner(v))? {
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

/// True if `err` means "byte never reached the wire" (cursor must NOT advance).
#[inline]
fn is_backpressure(err: &tungstenite::Error) -> bool {
    matches!(err, tungstenite::Error::Io(e) if e.kind() == io::ErrorKind::WouldBlock)
        || matches!(err, tungstenite::Error::WriteBufferFull(_))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tungstenite::error::CapacityError;

    #[test]
    fn wouldblock_io_is_backpressure() {
        let err = tungstenite::Error::Io(io::Error::new(io::ErrorKind::WouldBlock, "x"));
        assert!(is_backpressure(&err));
    }

    #[test]
    fn write_buffer_full_is_backpressure() {
        let err = tungstenite::Error::WriteBufferFull(Message::Binary(Bytes::new()));
        assert!(is_backpressure(&err));
    }

    #[test]
    fn other_io_kinds_are_not_backpressure() {
        for kind in [
            io::ErrorKind::ConnectionReset,
            io::ErrorKind::BrokenPipe,
            io::ErrorKind::UnexpectedEof,
            io::ErrorKind::TimedOut,
        ] {
            let err = tungstenite::Error::Io(io::Error::new(kind, "x"));
            assert!(!is_backpressure(&err), "{kind:?} should not be backpressure");
        }
    }

    #[test]
    fn connection_closed_is_not_backpressure() {
        assert!(!is_backpressure(&tungstenite::Error::ConnectionClosed));
        assert!(!is_backpressure(&tungstenite::Error::AlreadyClosed));
    }

    #[test]
    fn capacity_message_too_long_is_not_backpressure() {
        let err =
            tungstenite::Error::Capacity(CapacityError::MessageTooLong { size: 100, max_size: 50 });
        assert!(!is_backpressure(&err));
    }

    #[test]
    fn outdated_msg_static_has_minimum_len() {
        assert!(OUTDATED_MSG.len() > 16, "wire frame must be longer than CBOR header");
        assert!(FUTURE_MSG.len() > 16);
    }

    #[test]
    fn close_frames_have_expected_codes() {
        assert_eq!(FUTURE_CLOSE.code, CloseCode::Policy);
        assert_eq!(SHUTDOWN_FRAME.code, CloseCode::Restart);
    }
}
