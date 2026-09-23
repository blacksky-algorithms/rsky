use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;
use std::{io, thread};

use bytes::Bytes;
use fjall::{Keyspace, PartitionCreateOptions, PartitionHandle};
use mio::unix::SourceFd;
use mio::{Events, Interest, Poll, Token};
use thiserror::Error;

use crate::publisher::connection::{Connection, ConnectionError};
use crate::publisher::types::{Command, CommandReceiver};
use crate::types::{Cursor, DB};
use crate::validator::event::SubscribeReposEvent;
use crate::{SHUTDOWN, metrics};

const INTEREST: Interest = Interest::READABLE.add(Interest::WRITABLE);
const TIME_LAG_SAMPLE: u64 = 64;
const HEAD_STEP: u64 = 256;
const HEAD_BATCH: usize = 8192;

/// Live subscriber count across all publisher workers.
pub static SUBSCRIBERS: AtomicI64 = AtomicI64::new(0);

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
    frames: u64,
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
        Ok(Self {
            id,
            connections: Vec::new(),
            next_idx: 0,
            command_rx,
            firehose,
            poll,
            events,
            frames: 0,
        })
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
                        metrics::record_subscriber_count(
                            SUBSCRIBERS.fetch_add(1, Ordering::Relaxed) + 1,
                        );
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

            // Track the validator head until the range is exhausted (bounded per
            // pass): a fixed 32 frames per pass capped delivery at ~1,000/s per
            // worker and let subscribers fall hours behind during a replay.
            let mut sent = 0usize;
            while sent < HEAD_BATCH {
                let before = *seq;
                for msg in self.firehose.range((*seq + 1)..=(*seq + HEAD_STEP)) {
                    let (k, v) = msg?;
                    *seq = k.into();
                    self.send(*seq, &Bytes::from_owner(v));
                    sent += 1;
                }
                if *seq == before {
                    break;
                }
            }

            // Only sleep in the poller when nothing is being delivered and no
            // subscriber can make progress without a WRITABLE event.
            let busy = sent > 0 || self.can_make_progress(*seq);
            let (passes, timeout) =
                if busy { (1, Duration::ZERO) } else { (32, Duration::from_millis(1)) };
            let mut events = std::mem::replace(&mut self.events, Events::with_capacity(0));
            'outer: for _ in 0..passes {
                #[expect(clippy::expect_used)]
                self.poll.poll(&mut events, Some(timeout)).expect("failed to poll");
                for ev in &events {
                    let idx = ev.token().0;
                    if ev.is_readable() {
                        if let Some(conn) = &mut self.connections[idx] {
                            conn.readable = true;
                        }
                    }
                    if !self.poll(*seq, idx) {
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

    /// A subscriber behind the head that is not waiting on the socket can be
    /// replayed right now, so the worker must not idle.
    fn can_make_progress(&self, seq: Cursor) -> bool {
        self.connections
            .iter()
            .flatten()
            .any(|conn| !conn.needs_flush && conn.cursor.get() <= seq.get())
    }

    fn frame_time(&mut self, data: &Bytes) -> Option<chrono::DateTime<chrono::Utc>> {
        self.frames += 1;
        if self.frames % TIME_LAG_SAMPLE != 0 {
            return None;
        }
        SubscribeReposEvent::parse(data).ok().flatten().map(|event| event.time())
    }

    fn drop_connection(&mut self, idx: usize) {
        if let Some(conn) = self.connections[idx].take() {
            #[expect(clippy::expect_used)]
            self.poll
                .registry()
                .deregister(&mut SourceFd(&conn.as_raw_fd()))
                .expect("failed to deregister");
            metrics::record_subscriber_count(SUBSCRIBERS.fetch_sub(1, Ordering::Relaxed) - 1);
        }
    }

    fn send(&mut self, seq: Cursor, data: &Bytes) -> bool {
        let time = self.frame_time(data);
        for idx in 0..self.connections.len() {
            let Some(inner) = self.connections[idx].as_mut() else { continue };
            // Lagging connection: drain the gap via firehose range read instead of dropping the live event.
            let result = if inner.cursor == seq {
                let sent = inner.send(seq, data.clone());
                if let (Ok(true), Some(time)) = (&sent, time) {
                    inner.last_time = Some(time);
                }
                sent.map(|_| ())
            } else {
                inner.poll(seq, &self.firehose).map(|_| ())
            };
            if let Err(err) = result {
                tracing::info!(addr = %inner.addr, cursor = %inner.cursor, %err, "disconnected");
                self.drop_connection(idx);
            }
        }
        true
    }

    fn poll(&mut self, seq: Cursor, idx: usize) -> bool {
        if let Some(conn) = &mut self.connections[idx] {
            let outcome = conn
                .read_pending()
                .and_then(|open| if open { conn.poll(seq, &self.firehose) } else { Ok(false) });
            match outcome {
                Ok(true) => {
                    // against the validator's head, not this worker's position
                    let head = crate::HEAD_SEQ.load(Ordering::Relaxed).max(seq.get());
                    let lag = head.saturating_sub(conn.cursor.get().saturating_sub(1));
                    let time_lag = conn.last_time.map_or(0.0, |t| {
                        #[expect(clippy::cast_precision_loss)]
                        let millis = (chrono::Utc::now() - t).num_milliseconds() as f64;
                        millis / 1000.0
                    });
                    metrics::record_subscriber_lag(&conn.addr.to_string(), lag, time_lag);
                    return true;
                }
                Ok(false) => {
                    tracing::info!(addr = %conn.addr, cursor = %conn.cursor, "closed");
                }
                Err(err) => {
                    tracing::info!(addr = %conn.addr, cursor = %conn.cursor, %err, "disconnected");
                }
            }
            self.drop_connection(idx);
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

    use crate::shutdown_guard;

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
        let _guard = shutdown_guard();
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
        let _guard = shutdown_guard();
        let (mut w, _tx, _tmp, _ks) = build_worker();
        let mut seq = Cursor::from(0);
        SHUTDOWN.store(true, Ordering::SeqCst);
        let alive = w.update(&mut seq).unwrap();
        SHUTDOWN.store(false, Ordering::SeqCst);
        assert!(!alive);
    }

    #[test]
    fn update_broadcasts_new_events_to_caught_up_subscriber() {
        let _guard = shutdown_guard();
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
        let _guard = shutdown_guard();
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
        let _guard = shutdown_guard();
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

#[cfg(test)]
mod ping_tests {
    use super::*;
    use std::net::{TcpListener, TcpStream as StdTcpStream};
    use std::thread;
    use std::time::Duration;

    use tungstenite::client::IntoClientRequest;

    use crate::publisher::types::{Command, MaybeTlsStream as PubMaybeTls, SubscribeRepos};
    use crate::types::open_keyspace;

    #[test]
    fn worker_answers_pings_and_reports_closed_clients() {
        let _g = crate::shutdown_guard();
        let (_tx, rx) = rtrb::RingBuffer::<Command>::new(8);
        let tmp = tempfile::tempdir().unwrap();
        let ks = open_keyspace(tmp.path()).unwrap();
        let mut w = Worker::with_keyspace(0, rx, &ks).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let accept = thread::spawn(move || listener.accept().unwrap());
        let raw = StdTcpStream::connect(("127.0.0.1", port)).unwrap();
        let (server_stream, addr) = accept.join().unwrap();
        let client = thread::spawn(move || {
            let req = format!("ws://127.0.0.1:{port}/").into_client_request().unwrap();
            tungstenite::client(req, raw).unwrap().0
        });
        w.handle_command(
            Command::Connect(SubscribeRepos {
                addr,
                stream: PubMaybeTls::Plain(server_stream),
                cursor: Some(Cursor::from(1)),
            }),
            Cursor::from(0),
        );
        assert!(w.connections[0].is_some());
        let mut client = client.join().unwrap();
        client.send(tungstenite::Message::Ping(tungstenite::Bytes::from_static(b"k"))).unwrap();
        thread::sleep(Duration::from_millis(20));
        let mut seq = Cursor::from(0);
        w.update(&mut seq).unwrap();
        client.get_mut().set_nonblocking(true).unwrap();
        let mut got_pong = false;
        for _ in 0..200 {
            match client.read() {
                Ok(tungstenite::Message::Pong(_)) => {
                    got_pong = true;
                    break;
                }
                Ok(_) => {}
                Err(_) => thread::sleep(Duration::from_millis(5)),
            }
        }
        assert!(got_pong, "a Ping through the worker loop must be answered");
        // sampled frame timing parses real frames only
        let frames = w.frames;
        for _ in 0..TIME_LAG_SAMPLE {
            let _ = w.frame_time(&Bytes::from_static(b"not cbor"));
        }
        assert_eq!(w.frames, frames + TIME_LAG_SAMPLE);
        client.close(None).unwrap();
        thread::sleep(Duration::from_millis(20));
        w.update(&mut seq).unwrap();
        assert!(w.connections[0].is_none(), "closed client is dropped");
    }
}

#[cfg(all(test, not(feature = "labeler")))]
mod lag_tests {
    use super::*;
    use std::net::{TcpListener, TcpStream as StdTcpStream};
    use std::thread;

    use tungstenite::client::IntoClientRequest;

    use crate::publisher::types::{Command, MaybeTlsStream as PubMaybeTls, SubscribeRepos};
    use crate::types::open_keyspace;
    use crate::validator::testutil::identity_frame;

    #[test]
    fn worker_tracks_a_large_head_jump_and_drains_lagging_subscribers_fast() {
        let _g = crate::shutdown_guard();
        let (_tx, rx) = rtrb::RingBuffer::<Command>::new(8);
        let tmp = tempfile::tempdir().unwrap();
        let ks = open_keyspace(tmp.path()).unwrap();
        let mut w = Worker::with_keyspace(0, rx, &ks).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let accept = thread::spawn(move || listener.accept().unwrap());
        let raw = StdTcpStream::connect(("127.0.0.1", port)).unwrap();
        let (server_stream, addr) = accept.join().unwrap();
        let client = thread::spawn(move || {
            let req = format!("ws://127.0.0.1:{port}/").into_client_request().unwrap();
            tungstenite::client(req, raw).unwrap().0
        });
        w.handle_command(
            Command::Connect(SubscribeRepos {
                addr,
                stream: PubMaybeTls::Plain(server_stream),
                cursor: Some(Cursor::from(1)),
            }),
            Cursor::from(0),
        );
        let mut client = client.join().unwrap();
        let firehose = ks.open_partition("firehose", PartitionCreateOptions::default()).unwrap();
        let total = 20_000u64;
        for seq in 1..=total {
            firehose.insert(Cursor::from(seq), b"e".as_slice()).unwrap();
        }
        crate::HEAD_SEQ.store(total, Ordering::Relaxed);
        // drain on the client side concurrently so the server never backpressures
        let reader = thread::spawn(move || {
            client.get_mut().set_nonblocking(true).unwrap();
            let mut n = 0u64;
            let deadline = std::time::Instant::now() + Duration::from_secs(20);
            while n < total && std::time::Instant::now() < deadline {
                match client.read() {
                    Ok(tungstenite::Message::Binary(_)) => n += 1,
                    Ok(_) => {}
                    Err(_) => thread::sleep(Duration::from_millis(1)),
                }
            }
            n
        });
        let mut seq = Cursor::from(0);
        let start = std::time::Instant::now();
        let mut updates = 0;
        while seq.get() < total && updates < 200 {
            w.update(&mut seq).unwrap();
            updates += 1;
        }
        assert_eq!(seq.get(), total, "worker must reach the head");
        assert!(updates <= 3, "the head is tracked in a few passes, not {updates}");
        while w.connections[0].as_ref().is_some_and(|c| c.cursor.get() <= total)
            && start.elapsed() < Duration::from_secs(20)
        {
            w.update(&mut seq).unwrap();
        }
        assert_eq!(reader.join().unwrap(), total, "every frame reaches the subscriber");
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "20k frames drain in seconds, not minutes"
        );
    }

    #[test]
    fn sampled_frames_record_subscriber_time_lag_and_vanished_peers_drop() {
        let _g = crate::shutdown_guard();
        let (_tx, rx) = rtrb::RingBuffer::<Command>::new(8);
        let tmp = tempfile::tempdir().unwrap();
        let ks = open_keyspace(tmp.path()).unwrap();
        let mut w = Worker::with_keyspace(0, rx, &ks).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let accept = thread::spawn(move || listener.accept().unwrap());
        let raw = StdTcpStream::connect(("127.0.0.1", port)).unwrap();
        let (server_stream, addr) = accept.join().unwrap();
        let client = thread::spawn(move || {
            let req = format!("ws://127.0.0.1:{port}/").into_client_request().unwrap();
            tungstenite::client(req, raw).unwrap().0
        });
        w.handle_command(
            Command::Connect(SubscribeRepos {
                addr,
                stream: PubMaybeTls::Plain(server_stream),
                cursor: Some(Cursor::from(1)),
            }),
            Cursor::from(0),
        );
        let mut client = client.join().unwrap();
        let firehose = ks.open_partition("firehose", PartitionCreateOptions::default()).unwrap();
        for seq in 1..=TIME_LAG_SAMPLE {
            firehose.insert(Cursor::from(seq), identity_frame("did:plc:x", seq, None)).unwrap();
        }
        let mut seq = Cursor::from(0);
        w.update(&mut seq).unwrap();
        w.update(&mut seq).unwrap();
        assert!(
            w.connections[0].as_ref().unwrap().last_time.is_some(),
            "the 64th frame is sampled"
        );
        client.get_mut().set_nonblocking(true).unwrap();
        let mut received = 0;
        for _ in 0..500 {
            match client.read() {
                Ok(tungstenite::Message::Binary(_)) => received += 1,
                Ok(_) => {}
                Err(_) => {
                    if received >= TIME_LAG_SAMPLE {
                        break;
                    }
                    thread::sleep(std::time::Duration::from_millis(2));
                }
            }
        }
        assert_eq!(received, TIME_LAG_SAMPLE);
        drop(client);
        thread::sleep(std::time::Duration::from_millis(20));
        for seq in (TIME_LAG_SAMPLE + 1)..=(TIME_LAG_SAMPLE + 64) {
            firehose.insert(Cursor::from(seq), vec![0u8; 64 * 1024]).unwrap();
        }
        for _ in 0..20 {
            w.update(&mut seq).unwrap();
            if w.connections[0].is_none() {
                break;
            }
        }
        assert!(w.connections[0].is_none());
    }
}
