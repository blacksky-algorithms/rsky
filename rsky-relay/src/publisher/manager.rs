use std::sync::atomic::Ordering;
use std::time::Duration;
use std::{io, thread};

use thiserror::Error;

use crate::SHUTDOWN;
use crate::publisher::types::{Command, CommandSender, SubscribeReposReceiver};
use crate::publisher::worker::{Worker, WorkerError};

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
    subscribe_repos_rx: SubscribeReposReceiver,
}

impl Manager {
    pub fn new(
        n_workers: usize, subscribe_repos_rx: SubscribeReposReceiver,
    ) -> Result<Self, ManagerError> {
        let workers = (0..n_workers)
            .map(|worker_id| -> Result<_, ManagerError> {
                let (command_tx, command_rx) = rtrb::RingBuffer::new(CAPACITY);
                let thread_handle = thread::Builder::new()
                    .name(format!("rsky-pub-{worker_id}"))
                    .spawn(move || Worker::new(worker_id, command_rx)?.run())?;
                Ok(WorkerHandle { command_tx, thread_handle })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { workers: workers.into_boxed_slice(), next_id: 0, subscribe_repos_rx })
    }

    pub fn run(mut self) -> Result<(), ManagerError> {
        while self.update()? {
            thread::sleep(SLEEP);
        }
        tracing::info!("shutting down publisher");
        SHUTDOWN.store(true, Ordering::Relaxed);
        self.shutdown()
    }

    pub fn shutdown(mut self) -> Result<(), ManagerError> {
        for worker in &mut self.workers {
            worker.command_tx.push(Command::Shutdown)?;
        }
        for (id, worker) in self.workers.into_iter().enumerate() {
            if let Err(err) = worker.thread_handle.join().map_err(|_| ManagerError::Join)? {
                tracing::warn!(%id, %err, "publisher worker error");
            }
        }
        Ok(())
    }

    fn update(&mut self) -> Result<bool, ManagerError> {
        if SHUTDOWN.load(Ordering::Relaxed) {
            return Ok(false);
        }

        if let Ok(subscribe_repos) = self.subscribe_repos_rx.pop() {
            self.workers[self.next_id].command_tx.push(Command::Connect(subscribe_repos))?;
            self.next_id = (self.next_id + 1) % self.workers.len();
        }

        Ok(true)
    }
}
