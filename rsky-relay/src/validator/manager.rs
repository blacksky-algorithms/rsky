use std::sync::Arc;
use std::sync::atomic::Ordering;

use bus::{Bus, BusReader};
use sled::Tree;
use thiserror::Error;
use zerocopy::{CastError, FromBytes, Immutable, KnownLayout, SizeError, Unaligned};

use crate::SHUTDOWN;
use crate::types::{DB, MessageReceiver};
use crate::validator::resolver::{Resolver, ResolverError};
use crate::validator::types::{ParseError, SubscribeReposEvent};
use crate::validator::utils;

type Usize = zerocopy::Usize<zerocopy::BigEndian>;

const CAPACITY: usize = 1 << 16;

#[derive(Debug, Error)]
pub enum ManagerError {
    #[error("parse error: {0}")]
    Parse(#[from] ParseError),
    #[error("resolver error: {0}")]
    Resolver(#[from] ResolverError),
    #[error("size error")]
    SizeError,
    #[error("sled error: {0}")]
    Sled(#[from] sled::Error),
}

impl<T: KnownLayout + Immutable + Unaligned + ?Sized> From<CastError<&[u8], T>> for ManagerError {
    fn from(v: CastError<&[u8], T>) -> Self {
        let _: SizeError<&[u8], T> = v.into();
        Self::SizeError
    }
}

pub struct Manager {
    message_rx: MessageReceiver,
    resolver: Resolver,
    queue: Tree,
    bus: Bus<Arc<Vec<u8>>>,
}

impl Manager {
    pub async fn new(message_rx: MessageReceiver) -> Result<Self, ManagerError> {
        let resolver = Resolver::new().await?;
        let queue = DB.open_tree("queue")?;
        let bus = Bus::new(CAPACITY);
        Ok(Self { message_rx, resolver, queue, bus })
    }

    pub fn subscribe(&mut self) -> BusReader<Arc<Vec<u8>>> {
        self.bus.add_rx()
    }

    pub async fn run(mut self) -> Result<(), ManagerError> {
        while self.update().await? {}
        tracing::info!("shutting down validator");
        self.resolver.shutdown().await?;
        Ok(())
    }

    async fn update(&mut self) -> Result<bool, ManagerError> {
        if SHUTDOWN.load(Ordering::Relaxed) {
            return Ok(false);
        }

        for _ in 0..32 {
            match self.message_rx.try_recv_ref() {
                Ok(msg) => {
                    let event = SubscribeReposEvent::parse(&msg.data)?;
                    let did = event.did();
                    let commit = match event.commit() {
                        Ok(Some(commit)) => commit,
                        Ok(None) => {
                            self.resolver.expire(did)?;
                            self.bus.try_broadcast(Arc::new(msg.data.clone().into())).unwrap();
                            continue;
                        }
                        Err(err) => {
                            tracing::debug!("commit decode error: {err}");
                            continue;
                        }
                    };
                    if let Some(key) = self.resolver.resolve(did)? {
                        match utils::verify_commit_sig(&commit, key) {
                            Ok(res) => {
                                if res {
                                    self.bus
                                        .try_broadcast(Arc::new(msg.data.clone().into()))
                                        .unwrap();
                                } else {
                                    tracing::debug!("invalid signature: {commit:?} ({key:?})");
                                }
                            }
                            Err(err) => {
                                tracing::debug!("signature error: {err} ({key:?})");
                            }
                        }
                    } else {
                        self.queue.fetch_and_update(did, |prev| {
                            let prev = prev.unwrap_or_default();
                            let mut buf = Vec::with_capacity(prev.len() + 8 + msg.data.len());
                            buf.extend_from_slice(prev);
                            buf.extend_from_slice(&msg.data.len().to_be_bytes());
                            buf.extend_from_slice(&msg.data);
                            Some(buf)
                        })?;
                    }
                }
                Err(thingbuf::mpsc::errors::TryRecvError::Empty) => {}
                Err(thingbuf::mpsc::errors::TryRecvError::Closed) => return Ok(false),
                Err(_) => unreachable!(),
            }

            let Some((did, key)) = self.resolver.poll().await? else {
                continue;
            };
            let Some(msgs) = self.queue.remove(&did)? else {
                tracing::debug!("missing queue for did: {did}");
                continue;
            };
            let mut bytes = msgs.as_ref();
            loop {
                let (len, rest) = Usize::ref_from_prefix(bytes)?;
                let (data, rest) = <[u8]>::ref_from_prefix_with_elems(rest, len.get())?;
                bytes = rest;
                let event = SubscribeReposEvent::parse(data)?;
                let commit = event.commit().unwrap().unwrap(); // Already tried parsing
                match utils::verify_commit_sig(&commit, key) {
                    Ok(res) => {
                        if res {
                            self.bus.try_broadcast(Arc::new(data.into())).unwrap();
                        } else {
                            tracing::debug!("invalid signature: {commit:?} ({key:?})");
                        }
                    }
                    Err(err) => {
                        tracing::debug!("signature error: {err} ({key:?})");
                    }
                }
                if bytes.len() == 0 {
                    break;
                }
            }
        }

        Ok(true)
    }
}
