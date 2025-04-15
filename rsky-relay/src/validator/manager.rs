use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTimeError, UNIX_EPOCH};

use chrono::{DateTime, Utc};
use hashbrown::HashMap;
use sled::{Batch, Tree};
use thiserror::Error;
use zerocopy::{CastError, FromBytes, Immutable, KnownLayout, SizeError, Unaligned};

use rsky_common::tid::TID;

use crate::SHUTDOWN;
use crate::types::{Cursor, DB, MessageReceiver, TimedMessage};
use crate::validator::resolver::{Resolver, ResolverError};
use crate::validator::types::{ParseError, SerializeError, SubscribeReposEvent};
use crate::validator::utils;

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
    cursors: HashMap<String, (Cursor, DateTime<Utc>)>,
    commits: HashMap<String, TID>,
    resolver: Resolver,
    queue: Tree,
    crawlers: Tree,
    did_revs: Tree,
    firehose: Tree,
}

impl Manager {
    pub fn new(message_rx: MessageReceiver) -> Result<Self, ManagerError> {
        let cursors = HashMap::new();
        let commits = HashMap::new();
        let resolver = Resolver::new()?;
        let queue = DB.open_tree("queue")?;
        let crawlers = DB.open_tree("crawlers")?;
        let did_revs = DB.open_tree("did_revs")?;
        let firehose = DB.open_tree("firehose")?;
        let this =
            Self { message_rx, cursors, commits, resolver, queue, crawlers, did_revs, firehose };
        this.expire()?;
        Ok(this)
    }

    pub async fn run(mut self) -> Result<(), ManagerError> {
        for res in &self.crawlers {
            let (hostname, cursor) = res?;
            let hostname = unsafe { String::from_utf8_unchecked(hostname.to_vec()) };
            self.cursors.insert(hostname, (cursor.into(), DateTime::default()));
        }
        for res in &self.did_revs {
            let (did, rev) = res?;
            let did = unsafe { String::from_utf8_unchecked(did.to_vec()) };
            let rev = unsafe { String::from_utf8_unchecked(rev.to_vec()) };
            self.commits.insert(did, TID(rev));
        }
        for res in &self.queue {
            let (key, _) = res?;
            let key = unsafe { std::str::from_utf8_unchecked(&key) };
            #[expect(clippy::unwrap_used)]
            self.resolver.resolve(key.split('>').next().unwrap())?;
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
                    let Ok(Some(event)) = SubscribeReposEvent::parse(&msg.data, &msg.hostname)
                    else {
                        continue;
                    };

                    let curr = event.seq();
                    let mut time = event.time();
                    if let Some((prev, old)) = self.cursors.get(&msg.hostname) {
                        time = time.max(*old);
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
                            tracing::trace!(
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
                            if let SubscribeReposEvent::Identity(event) = &event {
                                tracing::trace!("identity event: {event:?}");
                                self.resolver.expire(did, time);
                            }
                            let data = event.serialize(msg.data.len(), seq.next())?;
                            self.firehose.insert(*seq, data)?;
                            self.cursors.insert(msg.hostname.clone(), (curr, time));
                            continue;
                        }
                        Err(err) => {
                            tracing::debug!("commit decode error: {err}");
                            continue;
                        }
                    };

                    let rev = &commit.rev;
                    if let Some(prev) = self.commits.get(did) {
                        if !prev.older_than(rev) {
                            tracing::debug!(
                                "[{did}] old msg: {rev} -> {prev} ({})",
                                rev.timestamp() - prev.timestamp()
                            );
                            continue;
                        }
                    }

                    if let Some((pds, key)) = self.resolver.resolve(did)? {
                        if let Some(pds) = pds {
                            if msg.hostname != pds {
                                tracing::debug!(
                                    "[{did}] hostname mismatch: {} (expected: {pds})",
                                    msg.hostname
                                );
                                continue;
                            }
                        }
                        match utils::verify_commit_sig(&commit, key) {
                            Ok(valid) => {
                                if valid {
                                    let data = event.serialize(msg.data.len(), seq.next())?;
                                    self.firehose.insert(*seq, data)?;
                                    self.commits.insert(commit.did, commit.rev);
                                    self.cursors.insert(msg.hostname.clone(), (curr, time));
                                } else {
                                    tracing::debug!("invalid signature: {commit:?} ({key:?})");
                                }
                            }
                            Err(err) => {
                                tracing::debug!("signature error: {err} ({key:?})");
                            }
                        }
                    } else {
                        self.queue
                            .insert(format!("{did}>{}>{curr}", msg.hostname), msg.data.to_vec())?;
                        self.commits.insert(commit.did, commit.rev);
                        self.cursors.insert(msg.hostname.clone(), (curr, time));
                    }
                }
                Err(thingbuf::mpsc::errors::TryRecvError::Empty) => {}
                Err(thingbuf::mpsc::errors::TryRecvError::Closed) => return Ok(false),
                Err(_) => unreachable!(),
            }

            let mut batch: Option<Batch> = None;
            for did in self.resolver.poll().await? {
                #[expect(clippy::unwrap_used)]
                let (pds, key) = self.resolver.resolve(&did)?.unwrap();
                for res in self.queue.scan_prefix(&did) {
                    let (k, data) = res?;
                    #[expect(clippy::unwrap_used)]
                    let hostname =
                        unsafe { std::str::from_utf8_unchecked(&k) }.split('>').nth(1).unwrap();
                    if let Some(pds) = pds {
                        if hostname != pds {
                            tracing::debug!(
                                "[{did}] hostname mismatch: {hostname} (expected: {pds})"
                            );
                            continue;
                        }
                    }
                    batch.get_or_insert_default().remove(k);

                    #[expect(clippy::unwrap_used)]
                    let event = SubscribeReposEvent::parse(&data, "")?.unwrap(); // Already tried parsing
                    #[expect(clippy::unwrap_used)]
                    let commit = event.commit()?.unwrap(); // Already tried parsing
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
                }
            }
            if let Some(batch) = batch {
                self.queue.apply_batch(batch)?;
            }
        }

        Ok(true)
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        let mut batch = sled::Batch::default();
        for (hostname, (cursor, time)) in &self.cursors {
            tracing::info!("[{hostname}] persisting cursor: {time}");
            batch.insert(hostname.as_bytes(), *cursor);
        }
        if let Err(err) = self.crawlers.apply_batch(batch) {
            tracing::warn!("unable to persist cursors: {err}\n{:#?}", self.cursors);
        }

        let mut batch = sled::Batch::default();
        for (did, commit) in self.commits.drain() {
            batch.insert(did.into_bytes(), commit.0.into_bytes());
        }
        if let Err(err) = self.did_revs.apply_batch(batch) {
            tracing::warn!("unable to persist commits: {err}");
        }

        if let Err(err) = DB.flush() {
            tracing::warn!("unable to flush db: {err}");
        }
    }
}
