use std::sync::atomic::Ordering;
use std::time::Duration;
use std::{io, thread};

use magnetic::Consumer;
use magnetic::buffer::dynamic::DynamicBufferP2;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use thiserror::Error;

use crate::SHUTDOWN;
use crate::crawler::RequestCrawl;
use crate::crawler::types::{Command, CommandSender, RequestCrawlReceiver, Status, StatusReceiver};
use crate::crawler::worker::{Worker, WorkerError};
use crate::types::MessageSender;

const CAPACITY: usize = 1024;
const SLEEP: Duration = Duration::from_millis(10);

#[derive(Debug, Error)]
pub enum ManagerError {
    #[error("spawn error: {0}")]
    Spawn(#[from] io::Error),
    #[error("worker error: {0}")]
    Worker(#[from] WorkerError),
    #[error("rtrb error: {0}")]
    Push(#[from] Box<rtrb::PushError<Command>>),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("join error")]
    Join,
}

impl From<rtrb::PushError<Command>> for ManagerError {
    fn from(value: rtrb::PushError<Command>) -> Self {
        Box::new(value).into()
    }
}

#[derive(Debug)]
struct WorkerHandle {
    pub command_tx: CommandSender,
    pub thread_handle: thread::JoinHandle<Result<(), WorkerError>>,
}

pub struct Manager {
    workers: Box<[WorkerHandle]>,
    next_id: usize,
    conn: Connection,
    request_crawl_rx: RequestCrawlReceiver,
    status_rx: StatusReceiver,
}

impl Manager {
    pub fn new(
        n_workers: usize, message_tx: &MessageSender, request_crawl_rx: RequestCrawlReceiver,
    ) -> Result<Self, ManagerError> {
        #[expect(clippy::unwrap_used)]
        let (status_tx, status_rx) =
            magnetic::mpsc::mpsc_queue(DynamicBufferP2::new(CAPACITY).unwrap());
        let workers = (0..n_workers)
            .map(|worker_id| -> Result<_, ManagerError> {
                let message_tx = message_tx.clone();
                let status_tx = status_tx.clone();
                let (command_tx, command_rx) = rtrb::RingBuffer::new(CAPACITY);
                let thread_handle =
                    thread::Builder::new().name(format!("rsky-crawl-{worker_id}")).spawn(
                        move || Worker::new(worker_id, message_tx, command_rx, status_tx)?.run(),
                    )?;
                Ok(WorkerHandle { command_tx, thread_handle })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let conn = Connection::open_with_flags(
            "relay.db",
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        Ok(Self {
            workers: workers.into_boxed_slice(),
            next_id: 0,
            conn,
            request_crawl_rx,
            status_rx,
        })
    }

    pub fn run(mut self) -> Result<(), ManagerError> {
        let mut requests = Vec::new();
        {
            let mut stmt = self.conn.prepare_cached("SELECT host, cursor FROM hosts")?;
            let mut rows = stmt.query(())?;
            while let Some(row) = rows.next()? {
                let hostname = row.get_unwrap("host");
                let cursor: u64 = row.get_unwrap("cursor");
                requests.push(RequestCrawl { hostname, cursor: Some(cursor.into()) });
            }
        }
        for request in requests {
            self.handle_connect(request)?;
        }
        while self.update()? {
            thread::sleep(SLEEP);
        }
        tracing::info!("shutting down crawler");
        SHUTDOWN.store(true, Ordering::Relaxed);
        self.shutdown()
    }

    pub fn shutdown(mut self) -> Result<(), ManagerError> {
        for worker in &mut self.workers {
            worker.command_tx.push(Command::Shutdown)?;
        }
        for (id, worker) in self.workers.into_iter().enumerate() {
            if let Err(err) = worker.thread_handle.join().map_err(|_| ManagerError::Join)? {
                tracing::warn!(%id, %err, "crawler worker error");
            }
        }
        Ok(())
    }

    fn update(&mut self) -> Result<bool, ManagerError> {
        if SHUTDOWN.load(Ordering::Relaxed) {
            return Ok(false);
        }

        if let Ok(status) = self.status_rx.try_pop() {
            if !self.handle_status(status)? {
                return Ok(false);
            }
        }

        if let Ok(request_crawl) = self.request_crawl_rx.pop() {
            let exists = {
                let mut stmt = self.conn.prepare_cached("SELECT * FROM hosts WHERE host = ?1")?;
                stmt.exists((&request_crawl.hostname,))?
            };
            if !exists {
                thread::sleep(SLEEP);
                self.handle_connect(request_crawl)?;
            }
        }

        Ok(true)
    }

    fn handle_status(&mut self, status: Status) -> Result<bool, ManagerError> {
        match status {
            Status::Disconnected(id, hostname) => {
                // TODO: add proper backoff
                thread::sleep(SLEEP * 1000);
                let prev = self.next_id;
                self.next_id = id;
                self.handle_connect(RequestCrawl { hostname, cursor: None })?;
                self.next_id = prev;
            }
        }
        Ok(true)
    }

    fn handle_connect(&mut self, mut request_crawl: RequestCrawl) -> Result<(), ManagerError> {
        let cursor: Option<u64> = {
            let mut stmt = self.conn.prepare_cached("SELECT * FROM hosts WHERE host = ?1")?;
            stmt.query_row((&request_crawl.hostname,), |row| Ok(row.get_unwrap("cursor")))
                .optional()?
        };
        if let Some(cursor) = cursor {
            request_crawl.cursor = Some(cursor.into());
        }
        self.workers[self.next_id].command_tx.push(Command::Connect(request_crawl))?;
        self.next_id = (self.next_id + 1) % self.workers.len();
        Ok(())
    }
}
