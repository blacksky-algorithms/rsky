use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

use magnetic::Consumer;
use magnetic::buffer::dynamic::DynamicBufferP2;
use thiserror::Error;

use crate::publisher::types::{
    Command, CommandSender, Config, LocalId, Status, StatusReceiver, WorkerId,
};
use crate::publisher::worker::{Worker, WorkerError};
use crate::types::SubscribeReposReceiver;
use crate::{SHUTDOWN, ValidatorManager};

const CAPACITY: usize = 1024;
const SLEEP: Duration = Duration::from_millis(10);

#[derive(Debug, Error)]
pub enum ManagerError {
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
    status_rx: StatusReceiver,
    subscribe_repos_rx: SubscribeReposReceiver,
}

impl Manager {
    pub fn new(
        n_workers: usize, validator: &mut ValidatorManager,
        subscribe_repos_rx: SubscribeReposReceiver,
    ) -> Result<Self, ManagerError> {
        let (status_tx, status_rx) =
            magnetic::mpsc::mpsc_queue(DynamicBufferP2::new(CAPACITY).unwrap());
        let workers = (0..n_workers)
            .map(|worker_id| {
                let message_rx = validator.subscribe();
                let status_tx = status_tx.clone();
                let (command_tx, command_rx) = rtrb::RingBuffer::new(CAPACITY);
                let thread_handle = thread::spawn(move || {
                    Worker::new(WorkerId(worker_id), message_rx, status_tx, command_rx).run()
                });
                WorkerHandle { configs: Vec::new(), command_tx, thread_handle }
            })
            .collect::<Vec<_>>();
        Ok(Self {
            workers: workers.into_boxed_slice(),
            next_id: WorkerId(0),
            status_rx,
            subscribe_repos_rx,
        })
    }

    pub fn run(mut self) -> Result<(), ManagerError> {
        while self.update()? {
            thread::sleep(SLEEP);
        }
        self.shutdown()
    }

    pub fn shutdown(mut self) -> Result<(), ManagerError> {
        for worker in &mut self.workers {
            worker.command_tx.push(Command::Shutdown)?;
        }
        for (id, worker) in self.workers.into_iter().enumerate() {
            if let Err(err) = worker.thread_handle.join().map_err(|_| ManagerError::JoinError)? {
                tracing::warn!("publisher worker {id} error: {err}");
            }
        }
        Ok(())
    }

    fn handle_status(&mut self, status: Status) -> Result<bool, ManagerError> {
        match status {}
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

        if let Ok(subscribe_repos) = self.subscribe_repos_rx.pop() {
            let config = Config {
                stream: subscribe_repos.stream,
                cursor: subscribe_repos.cursor,
                worker_id: self.next_id,
                local_id: LocalId(self.workers[self.next_id.0].configs.len()),
            };
            self.next_id = WorkerId((self.next_id.0 + 1) % self.workers.len());
            self.workers[config.worker_id.0].command_tx.push(Command::Connect(config)).unwrap();
        }

        Ok(true)
    }
}
