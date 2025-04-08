use std::sync::atomic::Ordering;
use std::time::Duration;
use std::{io, thread};

use hashbrown::HashMap;
use http::Uri;
use magnetic::Consumer;
use magnetic::buffer::dynamic::DynamicBufferP2;
use thiserror::Error;

use crate::SHUTDOWN;
use crate::crawler::types::{
    Command, CommandSender, Config, LocalId, Status, StatusReceiver, WorkerId,
};
use crate::crawler::worker::{Worker, WorkerError};
use crate::types::{MessageSender, RequestCrawlReceiver};

const CAPACITY: usize = 1024;
const SLEEP: Duration = Duration::from_millis(10);

#[derive(Debug, Error)]
pub enum ManagerError {
    #[error("spawn error: {0}")]
    SpawnError(#[from] io::Error),
    #[error("worker error: {0}")]
    WorkerError(#[from] WorkerError),
    #[error("rtrb error: {0}")]
    PushError(#[from] rtrb::PushError<Command>),
    #[error("join error")]
    JoinError,
}

#[derive(Debug)]
struct WorkerHandle {
    pub configs: Vec<Config>,
    pub command_tx: CommandSender,
    pub thread_handle: thread::JoinHandle<Result<(), WorkerError>>,
}

pub struct Manager {
    workers: Box<[WorkerHandle]>,
    next_id: WorkerId,
    configs: HashMap<Uri, Config>,
    status_rx: StatusReceiver,
    request_crawl_rx: RequestCrawlReceiver,
}

impl Manager {
    pub fn new(
        n_workers: usize, message_tx: MessageSender, request_crawl_rx: RequestCrawlReceiver,
    ) -> Result<Self, ManagerError> {
        let (status_tx, status_rx) =
            magnetic::mpsc::mpsc_queue(DynamicBufferP2::new(CAPACITY).unwrap());
        let workers = (0..n_workers)
            .map(|worker_id| -> Result<_, ManagerError> {
                let message_tx = message_tx.clone();
                let status_tx = status_tx.clone();
                let (command_tx, command_rx) = rtrb::RingBuffer::new(CAPACITY);
                let thread_handle = thread::Builder::new()
                    .name(format!("rsky-crawl-{worker_id}"))
                    .spawn(move || {
                        Worker::new(WorkerId(worker_id), message_tx, status_tx, command_rx).run()
                    })?;
                Ok(WorkerHandle { configs: Vec::new(), command_tx, thread_handle })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            workers: workers.into_boxed_slice(),
            next_id: WorkerId(0),
            configs: HashMap::new(),
            status_rx,
            request_crawl_rx,
        })
    }

    pub fn run(mut self) -> Result<(), ManagerError> {
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
            if let Err(err) = worker.thread_handle.join().map_err(|_| ManagerError::JoinError)? {
                tracing::warn!("crawler worker {id} error: {err}");
            }
        }
        Ok(())
    }

    fn handle_status(&mut self, _status: Status) -> Result<bool, ManagerError> {
        Ok(true)
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
            if !self.configs.contains_key(&request_crawl.uri) {
                let config = Config {
                    uri: request_crawl.uri.clone(),
                    hostname: request_crawl.hostname.clone(),
                    worker_id: self.next_id,
                    local_id: LocalId(self.workers[self.next_id.0].configs.len()),
                };
                self.next_id = WorkerId((self.next_id.0 + 1) % self.workers.len());
                self.configs.insert(request_crawl.uri, config.clone());
                self.workers[config.worker_id.0].command_tx.push(Command::Connect(config)).unwrap();
            }
        }

        Ok(true)
    }
}
