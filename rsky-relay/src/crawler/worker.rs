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
        Ok(Self { id, connections: Vec::new(), message_tx, command_rx, poll, events })
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

    pub fn shutdown(mut self) {
        for conn in self.connections.iter_mut().filter_map(|x| x.as_mut()) {
            if let Err(err) = conn.close() {
                tracing::warn!("crawler conn close error: {err}");
            }
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

            let mut events = std::mem::replace(&mut self.events, Events::with_capacity(0));
            for _ in 0..32 {
                #[expect(clippy::expect_used)]
                self.poll
                    .poll(&mut events, Some(Duration::from_millis(1)))
                    .expect("failed to poll");
                for ev in &events {
                    if !self.poll(ev.token().0) {
                        return false;
                    }
                }
            }
            self.events = events;
        }

        for idx in 0..self.connections.len() {
            if !self.poll(idx) {
                return false;
            }
        }

        true
    }

    fn poll(&mut self, idx: usize) -> bool {
        if let Some(conn) = &mut self.connections[idx] {
            if let Err(err) = conn.poll() {
                tracing::info!("[{}] disconnected: {err}", conn.hostname);
                #[expect(clippy::expect_used)]
                self.poll
                    .registry()
                    .deregister(&mut SourceFd(&conn.as_raw_fd()))
                    .expect("failed to deregister");
                self.connections[idx] = None;
            }
        }
        true
    }
}
