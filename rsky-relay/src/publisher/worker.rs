use std::sync::Arc;
use std::thread;

use bus::BusReader;
#[cfg(target_os = "linux")]
use nix::sys::epoll::{Epoll, EpollCreateFlags, EpollEvent, EpollFlags};
use thiserror::Error;

use crate::publisher::connection::{Connection, ConnectionError};
use crate::publisher::types::{Command, CommandReceiver, LocalId, StatusSender, WorkerId};

#[cfg(target_os = "linux")]
const EPOLL_FLAGS: EpollFlags = EpollFlags::EPOLLOUT;

#[derive(Debug, Error)]
pub enum WorkerError {
    #[error("connection error: {0}")]
    ConnectionError(#[from] ConnectionError),
}

pub struct Worker {
    worker_id: WorkerId,
    connections: Vec<Option<Connection>>,
    message_rx: BusReader<Arc<Vec<u8>>>,
    status_tx: StatusSender,
    command_rx: CommandReceiver,
    #[cfg(target_os = "linux")]
    epoll: Epoll,
    #[cfg(target_os = "linux")]
    events: Vec<EpollEvent>,
}

impl Worker {
    pub fn new(
        worker_id: WorkerId, message_rx: BusReader<Arc<Vec<u8>>>, status_tx: StatusSender,
        command_rx: CommandReceiver,
    ) -> Self {
        Self {
            worker_id,
            connections: Vec::new(),
            message_rx,
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
        tracing::info!("shutting down publisher: {}", self.worker_id.0);
        self.shutdown()
    }

    pub fn shutdown(mut self) -> Result<(), WorkerError> {
        for conn in self.connections.iter_mut().filter_map(|x| x.as_mut()) {
            if let Err(err) = conn.close() {
                tracing::warn!("publisher conn close error: {err}");
            }
        }
        Ok(())
    }

    fn handle_command(&mut self, command: Command) -> bool {
        match command {
            Command::Connect(config) => {
                let local_id = config.local_id;
                match Connection::connect(config, self.status_tx.clone()) {
                    Ok(conn) => {
                        #[cfg(target_os = "linux")]
                        #[expect(clippy::expect_used)]
                        self.epoll
                            .add(&conn, EpollEvent::new(EPOLL_FLAGS, local_id.0 as _))
                            .expect("failed to add connection");
                        self.connections.push(Some(conn));
                    }
                    Err(err) => {
                        tracing::warn!("unable to subscribeRepos: {err}");
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

            for _ in 0..32 {
                match self.message_rx.try_recv() {
                    Ok(msg) => {
                        self.send(&*msg);
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => return false,
                }
            }

            #[cfg(target_os = "linux")]
            {
                if !self.connections.iter().any(|c| c.is_some()) {
                    continue;
                }

                let mut events = std::mem::take(&mut self.events);
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
                self.events = events;
            }

            #[cfg(not(target_os = "linux"))]
            {
                for local_id in 0..self.connections.len() {
                    if !self.poll(LocalId(local_id)) {
                        return false;
                    }
                }
            }
        }

        for local_id in 0..self.connections.len() {
            if !self.poll(LocalId(local_id)) {
                return false;
            }
        }

        true
    }

    fn send(&mut self, input: &[u8]) -> bool {
        for conn in self.connections.iter_mut() {
            if let Some(inner) = conn.as_mut() {
                if let Err(_) = inner.send(input) {
                    #[cfg(target_os = "linux")]
                    #[expect(clippy::expect_used)]
                    self.epoll.delete(inner).expect("failed to delete connection");
                    *conn = None;
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
