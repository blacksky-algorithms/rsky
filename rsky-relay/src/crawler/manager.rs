use std::sync::atomic::Ordering;
use std::time::Duration;
use std::{io, thread};

use thiserror::Error;

use crate::SHUTDOWN;
use crate::crawler::types::{Command, CommandSender, RequestCrawlReceiver};
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
    request_crawl_rx: RequestCrawlReceiver,
}

impl Manager {
    pub fn new(
        n_workers: usize, message_tx: &MessageSender, request_crawl_rx: RequestCrawlReceiver,
    ) -> Result<Self, ManagerError> {
        let workers = (0..n_workers)
            .map(|worker_id| -> Result<_, ManagerError> {
                let message_tx = message_tx.clone();
                let (command_tx, command_rx) = rtrb::RingBuffer::new(CAPACITY);
                let thread_handle = thread::Builder::new()
                    .name(format!("rsky-crawl-{worker_id}"))
                    .spawn(move || Worker::new(worker_id, message_tx, command_rx)?.run())?;
                Ok(WorkerHandle { command_tx, thread_handle })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { workers: workers.into_boxed_slice(), next_id: 0, request_crawl_rx })
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
            if let Err(err) = worker.thread_handle.join().map_err(|_| ManagerError::Join)? {
                tracing::warn!("crawler worker {id} error: {err}");
            }
        }
        Ok(())
    }

    fn update(&mut self) -> Result<bool, ManagerError> {
        if SHUTDOWN.load(Ordering::Relaxed) {
            return Ok(false);
        }

        if let Ok(request_crawl) = self.request_crawl_rx.pop() {
            self.workers[self.next_id].command_tx.push(Command::Connect(request_crawl))?;
            self.next_id = (self.next_id + 1) % self.workers.len();
        }

        Ok(true)
    }
}
