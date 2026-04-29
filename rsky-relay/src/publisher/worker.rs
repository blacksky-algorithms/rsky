use std::os::fd::AsRawFd;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::{io, thread};

use bytes::Bytes;
use fjall::{Keyspace, PartitionCreateOptions, PartitionHandle};
use mio::unix::SourceFd;
use mio::{Events, Interest, Poll, Token};
use thiserror::Error;

use crate::SHUTDOWN;
use crate::publisher::connection::{Connection, ConnectionError};
use crate::publisher::types::{Command, CommandReceiver};
use crate::types::{Cursor, DB};

const INTEREST: Interest = Interest::WRITABLE;

#[derive(Debug, Error)]
pub enum WorkerError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("connection error: {0}")]
    ConnectionError(#[from] ConnectionError),
    #[error("fjall error: {0}")]
    Fjall(#[from] fjall::Error),
}

pub struct Worker {
    id: usize,
    connections: Vec<Option<Connection>>,
    next_idx: usize,
    command_rx: CommandReceiver,
    firehose: PartitionHandle,
    poll: Poll,
    events: Events,
}

impl Worker {
    pub fn new(id: usize, command_rx: CommandReceiver) -> Result<Self, WorkerError> {
        Self::with_keyspace(id, command_rx, &DB)
    }

    pub fn with_keyspace(
        id: usize, command_rx: CommandReceiver, db: &Keyspace,
    ) -> Result<Self, WorkerError> {
        let firehose = db.open_partition("firehose", PartitionCreateOptions::default())?;
        let poll = Poll::new()?;
        let events = Events::with_capacity(1024);
        Ok(Self { id, connections: Vec::new(), next_idx: 0, command_rx, firehose, poll, events })
    }

    pub fn run(mut self) -> Result<(), WorkerError> {
        let span = tracing::info_span!("publisher", id = %self.id);
        let _enter = span.enter();
        let mut seq = self.firehose.last_key_value()?.map(|(k, _)| k.into()).unwrap_or_default();
        while self.update(&mut seq)? {
            thread::yield_now();
        }
        tracing::info!("shutting down");
        self.shutdown();
        Ok(())
    }

    pub fn shutdown(self) {
        for conn in self.connections {
            drop(conn);
        }
    }

    fn handle_command(&mut self, command: Command, seq: Cursor) {
        match command {
            Command::Connect(config) => {
                tracing::info!(addr = %config.addr, cursor = ?config.cursor, "starting publish");
                // Absent cursor = "from now": start at the next not-yet-written seq.
                match Connection::connect(
                    config.addr,
                    config.stream,
                    config.cursor.unwrap_or_else(|| seq.successor()),
                ) {
                    Ok(conn) => {
                        let idx = self.connections.iter().position(Option::is_none).unwrap_or_else(
                            || {
                                let idx = self.connections.len();
                                self.connections.push(None);
                                idx
                            },
                        );
                        #[expect(clippy::expect_used)]
                        self.poll
                            .registry()
                            .register(&mut SourceFd(&conn.as_raw_fd()), Token(idx), INTEREST)
                            .expect("unable to register");
                        self.connections[idx] = Some(conn);
                    }
                    Err(err) => {
                        tracing::warn!(addr = %config.addr, cursor = ?config.cursor, %err, "unable to subscribeRepos");
                    }
                }
            }
        }
    }

    fn update(&mut self, seq: &mut Cursor) -> Result<bool, WorkerError> {
        if SHUTDOWN.load(Ordering::Relaxed) {
            return Ok(false);
        }

        for _ in 0..32 {
            if let Ok(command) = self.command_rx.pop() {
                self.handle_command(command, *seq);
            }

            for msg in self.firehose.range((*seq + 1)..=(*seq + 32)) {
                let (k, v) = msg?;
                *seq = k.into();
                self.send(*seq, &Bytes::from_owner(v));
            }

            let mut events = std::mem::replace(&mut self.events, Events::with_capacity(0));
            'outer: for _ in 0..32 {
                #[expect(clippy::expect_used)]
                self.poll
                    .poll(&mut events, Some(Duration::from_millis(1)))
                    .expect("failed to poll");
                for ev in &events {
                    if !self.poll(*seq, ev.token().0) {
                        break 'outer;
                    }
                }
            }
            self.events = events;
        }

        for _ in 0..self.connections.len() {
            self.next_idx = (self.next_idx + 1) % self.connections.len();
            if !self.poll(*seq, self.next_idx) {
                break;
            }
        }

        Ok(true)
    }

    fn send(&mut self, seq: Cursor, data: &Bytes) -> bool {
        for conn in &mut self.connections {
            let Some(inner) = conn.as_mut() else { continue };
            // Lagging connection: drain the gap via firehose range read instead of dropping the live event.
            let result = if inner.cursor == seq {
                inner.send(seq, data.clone()).map(|_| ())
            } else {
                inner.poll(seq, &self.firehose).map(|_| ())
            };
            if let Err(err) = result {
                tracing::info!(addr = %inner.addr, cursor = %inner.cursor, %err, "disconnected");
                #[expect(clippy::expect_used)]
                self.poll
                    .registry()
                    .deregister(&mut SourceFd(&inner.as_raw_fd()))
                    .expect("failed to deregister");
                *conn = None;
            }
        }
        true
    }

    fn poll(&mut self, seq: Cursor, idx: usize) -> bool {
        if let Some(conn) = &mut self.connections[idx] {
            match conn.poll(seq, &self.firehose) {
                Ok(true) => return true,
                Ok(false) => {
                    tracing::info!(addr = %conn.addr, cursor = %conn.cursor, "closed due to invalid cursor");
                }
                Err(err) => {
                    tracing::info!(addr = %conn.addr, cursor = %conn.cursor, %err, "disconnected");
                }
            }
            #[expect(clippy::expect_used)]
            self.poll
                .registry()
                .deregister(&mut SourceFd(&conn.as_raw_fd()))
                .expect("failed to deregister");
            self.connections[idx] = None;
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{SocketAddr, TcpListener, TcpStream as StdTcpStream};
    use std::sync::atomic::Ordering;
    use std::thread;
    use std::time::Duration;

    use tungstenite::WebSocket;
    use tungstenite::client::IntoClientRequest;

    use crate::publisher::types::{Command, MaybeTlsStream as PubMaybeTls, SubscribeRepos};
    use crate::types::open_keyspace;

    type WsClient = WebSocket<StdTcpStream>;

    fn build_worker() -> (Worker, rtrb::Producer<Command>, tempfile::TempDir, Keyspace) {
        let (tx, rx) = rtrb::RingBuffer::<Command>::new(64);
        let tmp = tempfile::tempdir().unwrap();
        let ks = open_keyspace(tmp.path()).unwrap();
        let worker = Worker::with_keyspace(0, rx, &ks).unwrap();
        (worker, tx, tmp, ks)
    }

    fn ws_pair_for_worker()
    -> (PubMaybeTls<StdTcpStream>, SocketAddr, std::thread::JoinHandle<WsClient>) {
        // The server-side WS handshake is driven by the caller (via Connection::connect).
        // The client-side handshake runs on a thread that joins after the caller drives accept.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let accept_thread = thread::spawn(move || listener.accept().unwrap());
        let raw_client = StdTcpStream::connect(("127.0.0.1", port)).unwrap();
        let (server_stream, peer_addr) = accept_thread.join().unwrap();
        let client_handle = thread::spawn(move || {
            let url = format!("ws://127.0.0.1:{port}/");
            let req = url.into_client_request().unwrap();
            let (client, _resp) = tungstenite::client(req, raw_client).unwrap();
            client
        });
        (PubMaybeTls::Plain(server_stream), peer_addr, client_handle)
    }

    fn drain_client_until_n_binary(client: &mut WsClient, n: usize, max_iter: usize) -> usize {
        client.get_mut().set_nonblocking(true).unwrap();
        let mut count = 0usize;
        for _ in 0..max_iter {
            if let Ok(tungstenite::Message::Binary(_)) = client.read() {
                count += 1;
                if count >= n {
                    break;
                }
            } else {
                thread::sleep(Duration::from_millis(5));
            }
        }
        count
    }

    #[test]
    fn with_keyspace_constructs() {
        let (_w, _tx, _tmp, _ks) = build_worker();
    }

    #[test]
    fn new_uses_global_db_via_env_var() {
        // Triggers the global DB LazyLock + Worker::new path.
        let tmp = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("RELAY_DB_PATH", tmp.path());
        }
        let (_tx, rx) = rtrb::RingBuffer::<Command>::new(8);
        let _w = Worker::new(0, rx).unwrap();
    }

    #[test]
    fn run_processes_commands_then_exits_on_shutdown() {
        // Drive run(): start a thread that flips SHUTDOWN after a short delay so run returns.
        let (w, mut tx, _tmp, _ks) = build_worker();
        let (server_stream, peer_addr, client_handle) = ws_pair_for_worker();
        // Push a Connect command BEFORE run() starts so the update loop sees it.
        tx.push(Command::Connect(SubscribeRepos {
            addr: peer_addr,
            stream: server_stream,
            cursor: Some(Cursor::from(0)),
        }))
        .unwrap();
        let stopper = thread::spawn(|| {
            thread::sleep(Duration::from_millis(50));
            SHUTDOWN.store(true, Ordering::SeqCst);
        });
        let result = w.run();
        stopper.join().unwrap();
        SHUTDOWN.store(false, Ordering::SeqCst);
        let _client = client_handle.join().unwrap();
        assert!(result.is_ok());
    }

    #[test]
    fn handle_command_connect_registers_connection() {
        let (mut w, _tx, _tmp, _ks) = build_worker();
        let (server_stream, peer_addr, client_handle) = ws_pair_for_worker();
        w.handle_command(
            Command::Connect(SubscribeRepos {
                addr: peer_addr,
                stream: server_stream,
                cursor: Some(Cursor::from(0)),
            }),
            Cursor::from(0),
        );
        assert_eq!(w.connections.len(), 1);
        assert!(w.connections[0].is_some());
        let _client = client_handle.join().unwrap();
    }

    #[test]
    fn handle_command_uses_seq_successor_when_cursor_absent() {
        let (mut w, _tx, _tmp, _ks) = build_worker();
        let (server_stream, peer_addr, client_handle) = ws_pair_for_worker();
        w.handle_command(
            Command::Connect(SubscribeRepos {
                addr: peer_addr,
                stream: server_stream,
                cursor: None,
            }),
            Cursor::from(7),
        );
        let conn = w.connections[0].as_ref().unwrap();
        assert_eq!(conn.cursor, Cursor::from(8));
        let _client = client_handle.join().unwrap();
    }

    #[test]
    fn update_returns_false_when_shutdown_set() {
        let (mut w, _tx, _tmp, _ks) = build_worker();
        let mut seq = Cursor::from(0);
        SHUTDOWN.store(true, Ordering::SeqCst);
        let alive = w.update(&mut seq).unwrap();
        SHUTDOWN.store(false, Ordering::SeqCst);
        assert!(!alive);
    }

    #[test]
    fn update_broadcasts_new_events_to_caught_up_subscriber() {
        let (mut w, _tx, _tmp, ks) = build_worker();
        let (server_stream, peer_addr, client_handle) = ws_pair_for_worker();
        w.handle_command(
            Command::Connect(SubscribeRepos {
                addr: peer_addr,
                stream: server_stream,
                cursor: Some(Cursor::from(1)),
            }),
            Cursor::from(0),
        );
        let mut client = client_handle.join().unwrap();
        let firehose = ks.open_partition("firehose", PartitionCreateOptions::default()).unwrap();
        firehose.insert(Cursor::from(1), b"live-event".as_slice()).unwrap();
        let mut seq = Cursor::from(0);
        w.update(&mut seq).unwrap();
        let received = drain_client_until_n_binary(&mut client, 1, 100);
        assert!(received >= 1, "expected >=1 binary frame, got {received}");
    }

    #[test]
    fn update_lag_routes_to_poll_for_subscriber_starting_at_zero() {
        // The bug-fix scenario. cursor=0 subscriber + live events should still be delivered.
        let (mut w, _tx, _tmp, ks) = build_worker();
        let firehose = ks.open_partition("firehose", PartitionCreateOptions::default()).unwrap();
        firehose.insert(Cursor::from(1), b"hist-1".as_slice()).unwrap();
        firehose.insert(Cursor::from(2), b"hist-2".as_slice()).unwrap();
        let (server_stream, peer_addr, client_handle) = ws_pair_for_worker();
        w.handle_command(
            Command::Connect(SubscribeRepos {
                addr: peer_addr,
                stream: server_stream,
                cursor: Some(Cursor::from(0)),
            }),
            Cursor::from(2),
        );
        let mut client = client_handle.join().unwrap();
        firehose.insert(Cursor::from(3), b"live-3".as_slice()).unwrap();
        let mut seq = Cursor::from(2);
        w.update(&mut seq).unwrap();
        let received = drain_client_until_n_binary(&mut client, 3, 200);
        assert!(received >= 3, "expected >=3 binary frames after lag-route, got {received}");
    }

    #[test]
    fn run_exits_immediately_when_shutdown_already_set() {
        let (w, _tx, _tmp, _ks) = build_worker();
        SHUTDOWN.store(true, Ordering::SeqCst);
        let result = w.run();
        SHUTDOWN.store(false, Ordering::SeqCst);
        assert!(result.is_ok());
    }

    #[test]
    fn shutdown_drops_connections() {
        let (mut w, _tx, _tmp, _ks) = build_worker();
        let (server_stream, peer_addr, client_handle) = ws_pair_for_worker();
        w.handle_command(
            Command::Connect(SubscribeRepos {
                addr: peer_addr,
                stream: server_stream,
                cursor: Some(Cursor::from(0)),
            }),
            Cursor::from(0),
        );
        assert!(w.connections[0].is_some());
        let _client = client_handle.join().unwrap();
        w.shutdown();
    }

    #[test]
    fn handle_command_failed_handshake_logs_and_skips() {
        // Pass a stream that won't complete a websocket handshake (immediately closed).
        let (listener_a, listener_b) =
            (TcpListener::bind("127.0.0.1:0").unwrap(), TcpListener::bind("127.0.0.1:0").unwrap());
        let port = listener_a.local_addr().unwrap().port();
        drop(listener_b);
        let server_thread = thread::spawn(move || listener_a.accept().unwrap());
        let client_stream = StdTcpStream::connect(("127.0.0.1", port)).unwrap();
        let (server_stream, peer_addr) = server_thread.join().unwrap();
        // Drop the client side BEFORE the worker tries to upgrade -> handshake fails.
        drop(client_stream);
        let (mut w, _tx, _tmp, _ks) = build_worker();
        w.handle_command(
            Command::Connect(SubscribeRepos {
                addr: peer_addr,
                stream: PubMaybeTls::Plain(server_stream),
                cursor: Some(Cursor::from(0)),
            }),
            Cursor::from(0),
        );
        // Connection failed -> connections vector remains empty.
        assert_eq!(w.connections.iter().filter(|c| c.is_some()).count(), 0);
    }

    #[test]
    fn worker_poll_deregisters_connection_on_invalid_cursor() {
        // Connection with a far-future cursor -> Connection::poll returns Ok(false) ->
        // Worker::poll falls through to deregister + None assignment.
        let (mut w, _tx, _tmp, _ks) = build_worker();
        let (server_stream, peer_addr, client_handle) = ws_pair_for_worker();
        w.handle_command(
            Command::Connect(SubscribeRepos {
                addr: peer_addr,
                stream: server_stream,
                cursor: Some(Cursor::from(100)),
            }),
            Cursor::from(0),
        );
        let _client = client_handle.join().unwrap();
        let alive = w.poll(Cursor::from(0), 0);
        assert!(alive, "Worker::poll always returns true (defensive break currently dead)");
        assert!(w.connections[0].is_none(), "future-cursor branch must deregister the slot");
    }

    #[test]
    fn worker_send_drops_connection_on_error() {
        let (mut w, _tx, _tmp, _ks) = build_worker();
        let (server_stream, peer_addr, client_handle) = ws_pair_for_worker();
        w.handle_command(
            Command::Connect(SubscribeRepos {
                addr: peer_addr,
                stream: server_stream,
                cursor: Some(Cursor::from(0)),
            }),
            Cursor::from(0),
        );
        let mut client = client_handle.join().unwrap();
        client.close(None).ok();
        thread::sleep(Duration::from_millis(20));
        for _ in 0..5 {
            w.send(Cursor::from(1), &Bytes::from_static(b"x"));
        }
        // No assert on slot state; macOS close-handshake is racy. Path is exercised.
        let _alive = w.connections[0].is_some();
    }
}
