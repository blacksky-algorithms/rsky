use std::thread;

#[cfg(target_os = "linux")]
use nix::sys::epoll::{Epoll, EpollCreateFlags, EpollEvent, EpollFlags};
use thiserror::Error;

use crate::crawler::connection::{Connection, ConnectionError};
use crate::crawler::types::{Command, CommandReceiver, Config, LocalId, StatusSender, WorkerId};
use crate::types::MessageSender;

#[cfg(target_os = "linux")]
const EPOLL_FLAGS: EpollFlags = EpollFlags::EPOLLIN;

#[derive(Debug, Error)]
pub enum WorkerError {
    #[error("connection error: {0}")]
    ConnectionError(#[from] ConnectionError),
}

pub struct Worker {
    worker_id: WorkerId,
    connections: Vec<Option<Connection>>,
    configs: Vec<Config>,
    message_tx: MessageSender,
    status_tx: StatusSender,
    command_rx: CommandReceiver,
    #[cfg(target_os = "linux")]
    epoll: Epoll,
    #[cfg(target_os = "linux")]
    events: Vec<EpollEvent>,
}

impl Worker {
    pub fn new(
        worker_id: WorkerId, message_tx: MessageSender, status_tx: StatusSender,
        command_rx: CommandReceiver,
    ) -> Self {
        Self {
            worker_id,
            connections: Vec::new(),
            configs: Vec::new(),
            message_tx,
            status_tx,
            command_rx,
            #[cfg(target_os = "linux")]
            #[expect(clippy::expect_used)]
            epoll: Epoll::new(EpollCreateFlags::empty()).expect("failed to create epoll"),
            #[cfg(target_os = "linux")]
            events: vec![EpollEvent::empty(); 1024],
        }
    }

    pub fn run(mut self) -> Result<(), WorkerError> {
        while self.update() {
            thread::yield_now();
        }
        tracing::info!("shutting down crawler: {}", self.worker_id.0);
        self.shutdown()
    }

    pub fn shutdown(mut self) -> Result<(), WorkerError> {
        for conn in self.connections.iter_mut().filter_map(|x| x.as_mut()) {
            if let Err(err) = conn.close() {
                tracing::warn!("crawler conn close error: {err}");
            }
        }
        Ok(())
    }

    fn handle_command(&mut self, command: Command) -> bool {
        match command {
            Command::Connect(config) => {
                match Connection::connect(
                    config.clone(),
                    self.message_tx.clone(),
                    self.status_tx.clone(),
                ) {
                    Ok(conn) => {
                        #[cfg(target_os = "linux")]
                        #[expect(clippy::expect_used)]
                        self.epoll
                            .add(&conn, EpollEvent::new(EPOLL_FLAGS, config.local_id.0 as _))
                            .expect("failed to add connection");
                        self.connections.push(Some(conn));
                        self.configs.push(config);
                    }
                    Err(err) => {
                        tracing::warn!("[{}] unable to requestCrawl: {err}", config.uri);
                    }
                }
            }
            Command::Shutdown => {
                return false;
            }
        }
        true
    }

    #[cfg(target_os = "linux")]
    fn update(&mut self) -> bool {
        for _ in 0..32 {
            if let Ok(command) = self.command_rx.pop() {
                if !self.handle_command(command) {
                    return false;
                }
            }

            let mut events = std::mem::take(&mut self.events);
            for _ in 0..32 {
                unsafe { events.set_len(events.capacity()) }
                #[expect(clippy::expect_used)]
                let len = self.epoll.wait(&mut events, 1u8).expect("failed to wait for epoll");
                if len == 0 {
                    continue;
                }
                unsafe { events.set_len(len) }

                for ev in &events {
                    #[expect(clippy::cast_possible_truncation)]
                    if !self.poll(LocalId(ev.data() as usize)) {
                        return false;
                    }
                }
            }
            self.events = events;
        }

        for local_id in 0..self.connections.len() {
            if !self.poll(LocalId(local_id)) {
                return false;
            }
        }

        true
    }

    #[cfg(not(target_os = "linux"))]
    fn update(&mut self) -> bool {
        if let Ok(command) = self.command_rx.pop() {
            if !self.handle_command(command) {
                return false;
            }
        }

        for _ in 0..32 {
            for local_id in 0..self.connections.len() {
                if !self.poll(LocalId(local_id)) {
                    return false;
                }
            }
        }

        true
    }

    fn poll(&mut self, local_id: LocalId) -> bool {
        if let Some(conn) = &mut self.connections[local_id.0] {
            if let Err(_) = conn.poll() {
                #[cfg(target_os = "linux")]
                #[expect(clippy::expect_used)]
                self.epoll.delete(conn).expect("failed to delete connection");
                self.connections[local_id.0] = None;
            }
        }
        true
    }
}
