use std::time::Duration;
use std::{io, thread};

use magnetic::Producer;
use polling::{Event, Events, PollMode, Poller};
use thiserror::Error;

use crate::crawler::connection::{Connection, ConnectionError};
use crate::crawler::types::{Command, CommandReceiver, Status, StatusSender};
use crate::types::MessageSender;

#[derive(Debug, Error)]
pub enum WorkerError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("connection error: {0}")]
    ConnectionError(#[from] ConnectionError),
}

pub struct Worker {
    id: usize,
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
        let span = tracing::debug_span!("crawler", id = %self.id);
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

    fn handle_command(&mut self, command: Command) -> bool {
        match command {
            Command::Connect(config) => {
                tracing::info!(host = %config.hostname, cursor = ?config.cursor, "starting crawl");
                match Connection::connect(
                    config.hostname.clone(),
                    config.cursor,
                    self.message_tx.clone(),
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
                        unsafe {
                            self.poller
                                .add_with_mode(&conn, Event::all(idx), PollMode::Level)
                                .expect("unable to register");
                        }
                        self.connections[idx] = Some(conn);
                    }
                    Err(err) => {
                        tracing::warn!(%err, "unable to requestCrawl");
                        #[expect(clippy::expect_used)]
                        self.status_tx
                            .push(Status::Disconnected(self.id, config.hostname))
                            .expect("unable to send status");
                    }
                }
            }
            Command::Shutdown => {
                return false;
            }
        }
        true
    }

    fn update(&mut self) -> bool {
        for _ in 0..32 {
            if let Ok(command) = self.command_rx.pop() {
                if !self.handle_command(command) {
                    return false;
                }
            }

            if self.message_tx.remaining() < 16 {
                break;
            }

            let mut events = std::mem::take(&mut self.events);
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
                        .push(Status::Disconnected(self.id, conn.hostname.clone()))
                        .expect("unable to send status");
                    self.connections[idx] = None;
                }
            }
        }

        true
    }
}
