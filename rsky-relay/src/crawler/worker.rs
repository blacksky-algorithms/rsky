use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use std::{io, thread};

use magnetic::Producer;
use polling::{Event, Events, PollMode, Poller};
use thiserror::Error;

use crate::SHUTDOWN;
use crate::crawler::connection::{Connection, ConnectionError};
use crate::crawler::types::{
    Command, CommandReceiver, DecomposeError, HandshakeResult, Handshaking, Status, StatusSender,
};
use crate::types::MessageSender;

const TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Error)]
pub enum WorkerError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("connection error: {0}")]
    ConnectionError(#[from] ConnectionError),
}

pub struct Worker {
    id: usize,
    pending: VecDeque<(Instant, String, Handshaking)>,
    connections: Vec<Option<Connection>>,
    next_idx: usize,
    message_tx: MessageSender,
    command_rx: CommandReceiver,
    status_tx: StatusSender,
    poller: Poller,
    events: Events,
}

impl Worker {
    pub fn new(
        id: usize, message_tx: MessageSender, command_rx: CommandReceiver, status_tx: StatusSender,
    ) -> Result<Self, WorkerError> {
        let poller = Poller::new()?;
        let events = Events::new();
        Ok(Self {
            id,
            pending: VecDeque::new(),
            connections: Vec::new(),
            next_idx: 0,
            message_tx,
            command_rx,
            status_tx,
            poller,
            events,
        })
    }

    #[expect(clippy::unnecessary_wraps)]
    pub fn run(mut self) -> Result<(), WorkerError> {
        let span = tracing::info_span!("crawler", id = %self.id);
        let _enter = span.enter();
        while self.update() {
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

    fn handle_command(&mut self, command: Command) {
        match command {
            Command::Connect(config) => {
                tracing::info!(host = %config.hostname, cursor = ?config.cursor, "starting crawl");
                let res = Connection::connect(&config.hostname, config.cursor);
                self.handle_connect(Instant::now(), config.hostname, res);
            }
        }
    }

    fn handle_connect(&mut self, start: Instant, hostname: String, result: HandshakeResult) {
        match result {
            Ok(Ok(client)) => {
                let idx = self.connections.iter().position(Option::is_none).unwrap_or_else(|| {
                    let idx = self.connections.len();
                    self.connections.push(None);
                    idx
                });
                let conn = Connection::new(hostname, client, self.message_tx.clone());
                #[expect(clippy::expect_used)]
                unsafe {
                    self.poller
                        .add_with_mode(&conn, Event::readable(idx), PollMode::Level)
                        .expect("unable to register");
                }
                self.connections[idx] = Some(conn);
                return;
            }
            Ok(Err(handshaking)) if start.elapsed() < TIMEOUT => {
                self.pending.push_back((Instant::now(), hostname, handshaking));
                return;
            }
            Ok(Err(_)) => {
                tracing::warn!(host = %hostname, "requestCrawl timeout");
            }
            Err(err) => {
                tracing::warn!(host = %hostname, %err, "unable to requestCrawl");
            }
        }

        #[expect(clippy::expect_used)]
        self.status_tx
            .push(Status::Disconnected { worker_id: self.id, hostname, connected: false })
            .expect("unable to send status");
    }

    fn update(&mut self) -> bool {
        if SHUTDOWN.load(Ordering::Relaxed) {
            return false;
        }

        for _ in 0..32 {
            if let Some((start, hostname, handshaking)) = self.pending.pop_front() {
                let res = handshaking.handshake().decompose();
                self.handle_connect(start, hostname, res);
            }

            if self.pending.len() < 16 {
                if let Ok(command) = self.command_rx.pop() {
                    self.handle_command(command);
                }
            }

            if self.message_tx.remaining() < 16 {
                break;
            }

            let mut events = std::mem::take(&mut self.events);
            events.clear();
            'outer: for _ in 0..32 {
                #[expect(clippy::expect_used)]
                self.poller
                    .wait(&mut events, Some(Duration::from_millis(1)))
                    .expect("failed to poll");
                for ev in events.iter() {
                    if !self.poll(ev.key) {
                        break 'outer;
                    }
                }
            }
            self.events = events;
        }

        for _ in 0..self.connections.len() {
            self.next_idx = (self.next_idx + 1) % self.connections.len();
            if !self.poll(self.next_idx) {
                break;
            }
        }

        true
    }

    fn poll(&mut self, idx: usize) -> bool {
        if let Some(conn) = &mut self.connections[idx] {
            match conn.poll() {
                Ok(true) => {}
                Ok(false) => return false,
                Err(err) => {
                    tracing::info!(host = %conn.hostname, %err, "disconnected");
                    #[expect(clippy::expect_used)]
                    self.poller.delete(&mut *conn).expect("failed to deregister");
                    #[expect(clippy::expect_used)]
                    self.status_tx
                        .push(Status::Disconnected {
                            worker_id: self.id,
                            hostname: conn.hostname.clone(),
                            connected: true,
                        })
                        .expect("unable to send status");
                    self.connections[idx] = None;
                }
            }
        }

        true
    }
}
