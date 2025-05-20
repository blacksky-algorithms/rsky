use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use std::{io, thread};

use exponential_backoff::{Backoff, IntoIter as BackoffIter};
use hashbrown::HashMap;
use magnetic::Consumer;
use magnetic::buffer::dynamic::DynamicBufferP2;
use rusqlite::{Connection, ErrorCode, OpenFlags, OptionalExtension};
use thiserror::Error;

use crate::SHUTDOWN;
use crate::crawler::RequestCrawl;
use crate::crawler::types::{Command, CommandSender, RequestCrawlReceiver, Status, StatusReceiver};
use crate::crawler::worker::{Worker, WorkerError};
use crate::types::{Cursor, MessageSender};

const CAPACITY: usize = 1 << 12;
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
    hosts: HashMap<String, [BackoffIter; 2]>,
    retries: BTreeMap<Instant, (usize, String)>,
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
            hosts: HashMap::new(),
            retries: BTreeMap::new(),
            conn,
            request_crawl_rx,
            status_rx,
        })
    }

    pub fn run(mut self) -> Result<(), ManagerError> {
        while self.update()? {
            thread::sleep(SLEEP);
        }
        tracing::info!("shutting down crawler");
        self.shutdown()
    }

    pub fn shutdown(self) -> Result<(), ManagerError> {
        SHUTDOWN.store(true, Ordering::Relaxed);
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

        if let Some(entry) = self.retries.first_entry() {
            if *entry.key() < Instant::now() {
                let (id, hostname) = entry.remove();
                let prev = self.next_id;
                self.next_id = id;
                self.handle_connect(RequestCrawl { hostname, cursor: None })?;
                self.next_id = prev;
            }
        }

        if let Ok(status) = self.status_rx.try_pop() {
            self.handle_status(status);
        }

        if let Ok(request_crawl) = self.request_crawl_rx.pop() {
            if !self.hosts.contains_key(&request_crawl.hostname) {
                self.handle_connect(request_crawl)?;
            }
        }

        Ok(true)
    }

    fn handle_status(&mut self, status: Status) {
        match status {
            Status::Disconnected { worker_id: id, hostname, connected } => {
                #[expect(clippy::unwrap_used)]
                let backoff =
                    self.hosts.get_mut(&hostname).unwrap().get_mut(usize::from(connected)).unwrap();
                let Some(Some(delay)) = backoff.next() else { unreachable!() };
                let next = Instant::now() + delay;
                assert!(self.retries.insert(next, (id, hostname)).is_none());
            }
        }
    }

    fn handle_connect(&mut self, mut request_crawl: RequestCrawl) -> Result<(), ManagerError> {
        self.hosts.entry(request_crawl.hostname.clone()).or_insert_with(|| {
            let backoff_connect =
                Backoff::new(u32::MAX, Duration::from_secs(60), Duration::from_secs(60 * 60 * 6));
            let backoff_reconnect =
                Backoff::new(u32::MAX, Duration::from_secs(1), Duration::from_secs(60 * 60));
            [backoff_connect.iter(), backoff_reconnect.iter()]
        });
        if request_crawl.cursor.is_none() {
            request_crawl.cursor = loop {
                match self.get_cursor(&request_crawl.hostname) {
                    Ok(cursor) => break cursor,
                    Err(ManagerError::Sqlite(err))
                        if err.sqlite_error_code() == Some(ErrorCode::DatabaseLocked) =>
                    {
                        continue;
                    }
                    Err(err) => Err(err)?,
                }
            };
        }
        self.workers[self.next_id].command_tx.push(Command::Connect(request_crawl))?;
        self.next_id = (self.next_id + 1) % self.workers.len();
        thread::sleep(SLEEP);
        Ok(())
    }

    fn get_cursor(&self, host: &str) -> Result<Option<Cursor>, ManagerError> {
        let mut stmt = self.conn.prepare_cached("SELECT * FROM hosts WHERE host = ?1")?;
        Ok(stmt
            .query_row((&host,), |row| Ok(row.get_unwrap::<_, u64>("cursor")))
            .optional()?
            .map(Into::into))
    }
}
