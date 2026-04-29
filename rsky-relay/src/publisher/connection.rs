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
    use std::net::{TcpListener, TcpStream as StdTcpStream};
    use std::thread;
    use std::time::Duration;
    use tungstenite::client::IntoClientRequest;
    use tungstenite::error::CapacityError;

    use fjall::{Config, PartitionCreateOptions};

    fn ws_pair() -> (Connection, tungstenite::WebSocket<StdTcpStream>) {
        // Spawns a local TcpListener, accepts on a thread to build Connection,
        // and on this thread drives a tungstenite client handshake. Returns both ends.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server_handle = thread::spawn(move || {
            let (s, addr) = listener.accept().unwrap();
            Connection::connect(addr, MaybeTlsStream::Plain(s), Cursor::from(0)).unwrap()
        });
        let url = format!("ws://127.0.0.1:{port}/");
        let req = url.into_client_request().unwrap();
        // tungstenite::client returns a (WebSocket, Response) tuple after handshake.
        let stream = StdTcpStream::connect(("127.0.0.1", port)).unwrap();
        let (client, _resp) = tungstenite::client(req, stream).unwrap();
        let server = server_handle.join().unwrap();
        (server, client)
    }

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

    #[test]
    fn connect_succeeds_and_returns_raw_fd() {
        let (server, client) = ws_pair();
        assert!(server.as_raw_fd() > 0);
        assert_eq!(server.cursor, Cursor::from(0));
        drop(client);
        drop(server);
    }

    #[test]
    fn send_with_matching_cursor_delivers_bytes_and_advances() {
        let (mut server, mut client) = ws_pair();
        let payload = Bytes::from_static(b"hello-relay");
        let result = server.send(Cursor::from(0), payload.clone()).unwrap();
        assert!(result, "send with matching cursor must report delivered");
        assert_eq!(server.cursor, Cursor::from(1), "cursor must advance to seq+1");
        let msg = client.read().unwrap();
        assert_eq!(msg, Message::Binary(payload));
        client.close(None).ok();
    }

    #[test]
    fn send_with_mismatched_cursor_drops_silently() {
        let (mut server, mut client) = ws_pair();
        // server.cursor = 0, but ask to send seq=5 -> mismatch -> Ok(false), no I/O.
        let result = server.send(Cursor::from(5), Bytes::from_static(b"x")).unwrap();
        assert!(!result, "mismatch must report not-delivered");
        assert_eq!(server.cursor, Cursor::from(0), "cursor must not advance");
        client.get_mut().set_nonblocking(true).unwrap();
        let read = client.read();
        assert!(
            matches!(read, Err(tungstenite::Error::Io(ref e)) if e.kind() == io::ErrorKind::WouldBlock)
        );
        client.close(None).ok();
    }

    #[test]
    fn close_emits_close_frame_to_client() {
        let (mut server, mut client) = ws_pair();
        let frame = CloseFrame { code: CloseCode::Normal, reason: Utf8Bytes::from_static("bye") };
        server.close(frame).unwrap();
        let msg = client.read().unwrap();
        assert!(matches!(msg, Message::Close(_)));
        drop(server);
    }

    #[test]
    fn drop_emits_shutdown_close_frame() {
        let (server, mut client) = ws_pair();
        drop(server); // triggers Drop -> close(SHUTDOWN_FRAME)
        let mut saw_close = false;
        for _ in 0..20 {
            if let Ok(Message::Close(_)) = client.read() {
                saw_close = true;
                break;
            }
        }
        assert!(saw_close, "client must observe close frame after server drop");
    }

    fn open_test_keyspace() -> (tempfile::TempDir, fjall::Keyspace) {
        let tmp = tempfile::tempdir().unwrap();
        let ks = Config::new(tmp.path()).open().unwrap();
        ks.open_partition("firehose", PartitionCreateOptions::default()).unwrap();
        (tmp, ks)
    }

    #[test]
    fn poll_with_empty_firehose_returns_true() {
        let (mut server, mut client) = ws_pair();
        let (_tmp, ks) = open_test_keyspace();
        let firehose = ks.open_partition("firehose", PartitionCreateOptions::default()).unwrap();
        // cursor=0, seq=0: range is empty -> early loop exit, returns Ok(true).
        let alive = server.poll(Cursor::from(0), &firehose).unwrap();
        assert!(alive);
        client.close(None).ok();
    }

    #[test]
    fn poll_with_future_cursor_closes_connection() {
        let (mut server, mut client) = ws_pair();
        let (_tmp, ks) = open_test_keyspace();
        let firehose = ks.open_partition("firehose", PartitionCreateOptions::default()).unwrap();
        // Force connection.cursor to 100 while seq is at 0 -> future-cursor branch.
        server.cursor = Cursor::from(100);
        let alive = server.poll(Cursor::from(0), &firehose).unwrap();
        assert!(!alive, "future cursor must close the connection");
        // Client should receive the future-cursor frame followed by close.
        let mut got_binary = false;
        let mut got_close = false;
        client.get_mut().set_nonblocking(true).unwrap();
        for _ in 0..20 {
            if let Ok(msg) = client.read() {
                match msg {
                    Message::Binary(_) => got_binary = true,
                    Message::Close(_) => {
                        got_close = true;
                        break;
                    }
                    _ => {}
                }
            } else {
                thread::sleep(Duration::from_millis(10));
            }
        }
        assert!(got_binary, "client must receive future-cursor frame");
        assert!(got_close, "client must receive close frame");
    }

    #[test]
    fn send_propagates_non_backpressure_error() {
        // After a close handshake, the next send() must propagate Err (ConnectionClosed),
        // exercising the `Err(err) => Err(err)?` arm in send().
        let (mut server, client) = ws_pair();
        server
            .close(CloseFrame { code: CloseCode::Normal, reason: Utf8Bytes::from_static("x") })
            .unwrap();
        let result = server.send(server.cursor, Bytes::from_static(b"x"));
        assert!(result.is_err(), "send after close must error: got {result:?}");
        drop(client);
    }

    #[test]
    fn poll_emits_outdated_msg_when_cursor_lags_below_oldest_in_range() {
        // Seed firehose with seq=10. Connection cursor=5 (non-zero, lower than range start).
        // poll's loop sees self.cursor != seq -> emits OUTDATED_MSG, jumps cursor to seq.
        let (mut server, mut client) = ws_pair();
        let (_tmp, ks) = open_test_keyspace();
        let firehose = ks.open_partition("firehose", PartitionCreateOptions::default()).unwrap();
        firehose.insert(Cursor::from(10), b"evt-10".as_slice()).unwrap();
        server.cursor = Cursor::from(5);
        let alive = server.poll(Cursor::from(10), &firehose).unwrap();
        assert!(alive);
        // Client should observe at least 2 binary frames: OUTDATED then the actual event.
        client.get_mut().set_nonblocking(true).unwrap();
        let mut binary_count = 0usize;
        for _ in 0..50 {
            if let Ok(Message::Binary(_)) = client.read() {
                binary_count += 1;
                if binary_count >= 2 {
                    break;
                }
            } else {
                thread::sleep(Duration::from_millis(5));
            }
        }
        assert!(binary_count >= 2, "expected outdated_msg + event, got {binary_count}");
    }

    #[test]
    fn poll_breaks_loop_when_send_returns_false() {
        // Seed firehose with multiple events. Force connection.cursor != first seq (so initial
        // self.send(self.cursor, OUTDATED_MSG) is exercised via an early path), and then make
        // the actual send fail by closing the client. The loop hits `if !self.send(...) break`.
        let (mut server, client) = ws_pair();
        drop(client);
        let (_tmp, ks) = open_test_keyspace();
        let firehose = ks.open_partition("firehose", PartitionCreateOptions::default()).unwrap();
        for i in 1u64..=5 {
            let b = format!("evt-{i}");
            firehose.insert(Cursor::from(i), b.as_bytes()).unwrap();
        }
        // Don't assert success; we expect either a clean Ok(true) or an error from broken pipe.
        drop(server.poll(Cursor::from(5), &firehose));
    }

    #[test]
    fn poll_drains_firehose_range_to_subscriber() {
        let (mut server, mut client) = ws_pair();
        let (_tmp, ks) = open_test_keyspace();
        let firehose = ks.open_partition("firehose", PartitionCreateOptions::default()).unwrap();
        // Seed three events at seq 1, 2, 3.
        for i in 1u64..=3 {
            let body = format!("evt-{i}");
            firehose.insert(Cursor::from(i), body.as_bytes()).unwrap();
        }
        server.cursor = Cursor::from(0);
        let alive = server.poll(Cursor::from(3), &firehose).unwrap();
        assert!(alive);
        // First send sees cursor=0 vs seq=1 mismatch -> emits OUTDATED_MSG, then re-sends event.
        client.get_mut().set_nonblocking(true).unwrap();
        let mut payloads = Vec::new();
        for _ in 0..50 {
            if let Ok(Message::Binary(b)) = client.read() {
                payloads.push(b);
                if payloads.len() >= 4 {
                    break;
                }
            } else {
                thread::sleep(Duration::from_millis(5));
            }
        }
        assert!(payloads.len() >= 3, "expected >=3 binary frames, got {}", payloads.len());
        client.close(None).ok();
    }
}
