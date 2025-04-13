use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTimeError, UNIX_EPOCH};

use hashbrown::HashMap;
use sled::{Batch, Tree};
use thiserror::Error;
use zerocopy::{CastError, FromBytes, Immutable, KnownLayout, SizeError, Unaligned};

use crate::SHUTDOWN;
use crate::types::{Cursor, DB, MessageReceiver, TimedMessage};
use crate::validator::resolver::{Resolver, ResolverError};
use crate::validator::types::{ParseError, SerializeError, SubscribeReposEvent};
use crate::validator::utils;

type Usize = zerocopy::Usize<zerocopy::BigEndian>;

const KEY_TTL: Duration = Duration::from_secs(60 * 60 * 24);

#[derive(Debug, Error)]
pub enum ManagerError {
    #[error("parse error: {0}")]
    Parse(#[from] ParseError),
    #[error("serialize error: {0}")]
    Serialize(#[from] SerializeError),
    #[error("resolver error: {0}")]
    Resolver(#[from] ResolverError),
    #[error("size error")]
    SizeError,
    #[error("time error: {0}")]
    Time(#[from] SystemTimeError),
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
    cursors: HashMap<String, Cursor>,
    resolver: Resolver,
    queue: Tree,
    crawlers: Tree,
    firehose: Tree,
}

impl Manager {
    pub fn new(message_rx: MessageReceiver) -> Result<Self, ManagerError> {
        let cursors = HashMap::new();
        let resolver = Resolver::new()?;
        let queue = DB.open_tree("queue")?;
        let crawlers = DB.open_tree("crawlers")?;
        let firehose = DB.open_tree("firehose")?;
        let this = Self { message_rx, cursors, resolver, queue, crawlers, firehose };
        this.expire()?;
        Ok(this)
    }

    pub async fn run(mut self) -> Result<(), ManagerError> {
        for res in &self.crawlers {
            let (hostname, cursor) = res?;
            let hostname = unsafe { String::from_utf8_unchecked(hostname.to_vec()) };
            self.cursors.insert(hostname, cursor.into());
        }
        for res in &self.queue {
            let (did, _) = res?;
            let did = unsafe { std::str::from_utf8_unchecked(&did) };
            self.resolver.request(did);
        }
        let mut seq = self.firehose.last()?.map(|(k, _)| k.into()).unwrap_or_default();
        while self.update(&mut seq).await? {}
        tracing::info!("shutting down validator");
        SHUTDOWN.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn expire(&self) -> Result<(), ManagerError> {
        let mut batch: Option<Batch> = None;
        for res in &self.firehose {
            let (cursor, data) = res?;
            let msg = TimedMessage::ref_from_bytes(&data)?;
            let time = UNIX_EPOCH + Duration::from_secs(msg.timestamp.get());
            if time.elapsed()? > KEY_TTL {
                batch.get_or_insert_default().remove(cursor);
            } else {
                break;
            }
        }
        if let Some(batch) = batch {
            self.firehose.apply_batch(batch)?;
        }
        Ok(())
    }

    #[expect(clippy::too_many_lines)]
    async fn update(&mut self, seq: &mut Cursor) -> Result<bool, ManagerError> {
        if SHUTDOWN.load(Ordering::Relaxed) {
            return Ok(false);
        }

        self.expire()?;

        for _ in 0..1024 {
            match self.message_rx.try_recv_ref() {
                Ok(msg) => {
                    let Ok(Some(event)) = SubscribeReposEvent::parse(&msg.data) else {
                        continue;
                    };
                    let curr = event.seq();
                    if let Some(prev) = self.cursors.get(&msg.hostname) {
                        let prev: u64 = (*prev).into();
                        let curr: u64 = curr.into();
                        if prev >= curr {
                            if prev > curr {
                                tracing::debug!(
                                    "[{}] old msg: {curr} -> {prev} ({})",
                                    msg.hostname,
                                    prev - curr
                                );
                            }
                            continue;
                        } else if prev + 1 != curr {
                            tracing::debug!(
                                "[{}] seq gap: {prev} -> {curr} ({})",
                                msg.hostname,
                                curr - prev - 1
                            );
                        }
                    }
                    let did = event.did();
                    let commit = match event.commit() {
                        Ok(Some(commit)) => commit,
                        Ok(None) => {
                            self.resolver.expire(did)?;
                            let data = event.serialize(msg.data.len(), seq.next())?;
                            self.firehose.insert(*seq, data)?;
                            self.cursors.insert(msg.hostname.clone(), curr);
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
                                    let data = event.serialize(msg.data.len(), seq.next())?;
                                    self.firehose.insert(*seq, data)?;
                                    self.cursors.insert(msg.hostname.clone(), curr);
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
                        self.cursors.insert(msg.hostname.clone(), curr);
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
                let Some(event) = SubscribeReposEvent::parse(data)? else {
                    continue;
                };
                #[expect(clippy::unwrap_used)]
                let commit = event.commit().unwrap().unwrap(); // Already tried parsing
                match utils::verify_commit_sig(&commit, key) {
                    Ok(res) => {
                        if res {
                            let data = event.serialize(data.len(), seq.next())?;
                            self.firehose.insert(*seq, data)?;
                        } else {
                            tracing::debug!("invalid signature: {commit:?} ({key:?})");
                        }
                    }
                    Err(err) => {
                        tracing::debug!("signature error: {err} ({key:?})");
                    }
                }
                if bytes.is_empty() {
                    break;
                }
            }
        }

        Ok(true)
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        let mut batch = sled::Batch::default();
        for (hostname, cursor) in &self.cursors {
            batch.insert(hostname.as_bytes(), *cursor);
        }
        if let Err(err) = self.crawlers.apply_batch(batch) {
            tracing::warn!("unable to persist cursors: {err}\n{:#?}", self.cursors);
        }
        if let Err(err) = DB.flush() {
            tracing::warn!("unable to persist cursors: {err}\n{:#?}", self.cursors);
        }
    }
}
