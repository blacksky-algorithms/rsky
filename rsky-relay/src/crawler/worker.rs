use std::os::fd::AsRawFd;
use std::time::Duration;
use std::{io, thread};

use mio::unix::SourceFd;
use mio::{Events, Interest, Poll, Token};
use thiserror::Error;

use crate::crawler::connection::{Connection, ConnectionError};
use crate::crawler::types::{Command, CommandReceiver};
use crate::types::MessageSender;

const INTEREST: Interest = Interest::READABLE;

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
    poll: Poll,
    events: Events,
}

impl Worker {
    pub fn new(
        id: usize, message_tx: MessageSender, command_rx: CommandReceiver,
    ) -> Result<Self, WorkerError> {
        let poll = Poll::new()?;
        let events = Events::with_capacity(1024);
        Ok(Self { id, connections: Vec::new(), next_idx: 0, message_tx, command_rx, poll, events })
    }

    #[expect(clippy::unnecessary_wraps)]
    pub fn run(mut self) -> Result<(), WorkerError> {
        while self.update() {
            thread::yield_now();
        }
        tracing::info!("shutting down crawler: {}", self.id);
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
                tracing::info!(
                    "[{}] starting crawl: {} ({:?})",
                    self.id,
                    config.hostname,
                    config.cursor
                );
                match Connection::connect(config.hostname, config.cursor, self.message_tx.clone()) {
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
                        tracing::warn!("unable to requestCrawl: {err}");
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

            let mut events = std::mem::replace(&mut self.events, Events::with_capacity(0));
            'outer: for _ in 0..32 {
                #[expect(clippy::expect_used)]
                self.poll
                    .poll(&mut events, Some(Duration::from_millis(1)))
                    .expect("failed to poll");
                for ev in &events {
                    if !self.poll(ev.token().0) {
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
                    tracing::info!("[{}] disconnected: {err}", conn.hostname);
                    #[expect(clippy::expect_used)]
                    self.poll
                        .registry()
                        .deregister(&mut SourceFd(&conn.as_raw_fd()))
                        .expect("failed to deregister");
                    self.connections[idx] = None;
                }
            }
        }

        true
    }
}
