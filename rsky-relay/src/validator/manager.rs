use std::convert::Infallible;
use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTimeError, UNIX_EPOCH};

use chrono::{DateTime, Utc};
use hashbrown::HashMap;
use hashbrown::hash_map::Entry;
use sled::{Batch, Tree};
use thiserror::Error;
use zerocopy::{CastError, FromBytes, Immutable, KnownLayout, SizeError, Unaligned};

use crate::SHUTDOWN;
use crate::types::{Cursor, DB, MessageReceiver, TimedMessage};
use crate::validator::event::{ParseError, SerializeError, SubscribeReposEvent};
use crate::validator::resolver::{Resolver, ResolverError};
use crate::validator::types::RepoState;
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
    #[error("decode error: {0}")]
    DecodeError(#[from] serde_ipld_dagcbor::DecodeError<Infallible>),
}

impl<T: KnownLayout + Immutable + Unaligned + ?Sized> From<CastError<&[u8], T>> for ManagerError {
    fn from(v: CastError<&[u8], T>) -> Self {
        let _: SizeError<&[u8], T> = v.into();
        Self::SizeError
    }
}

pub struct Manager {
    message_rx: MessageReceiver,
    hosts: HashMap<String, (Cursor, DateTime<Utc>)>,
    repos: HashMap<String, RepoState>,
    resolver: Resolver,
    queue: Tree,
    firehose: Tree,
}

impl Manager {
    pub fn new(message_rx: MessageReceiver) -> Result<Self, ManagerError> {
        let hosts = HashMap::new();
        let repos = HashMap::new();
        let resolver = Resolver::new()?;
        let queue = DB.open_tree("queue")?;
        let firehose = DB.open_tree("firehose")?;
        let this = Self { message_rx, hosts, repos, resolver, queue, firehose };
        this.expire()?;
        Ok(this)
    }

    pub async fn run(mut self) -> Result<(), ManagerError> {
        for res in &DB.open_tree("hosts")? {
            let (host, state) = res?;
            let host = unsafe { String::from_utf8_unchecked(host.to_vec()) };
            self.hosts.insert(host, (state.into(), DateTime::default()));
        }
        for res in &DB.open_tree("repos")? {
            let (did, state) = res?;
            let did = unsafe { String::from_utf8_unchecked(did.to_vec()) };
            let state = serde_ipld_dagcbor::from_slice(&state)?;
            self.repos.insert(did, state);
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
                    let host = &msg.hostname;
                    let event = match SubscribeReposEvent::parse(&msg.data, host) {
                        Ok(Some(event)) => event,
                        Ok(None) => continue,
                        Err(err) => {
                            tracing::debug!("[{host}] parse error: {err}");
                            continue;
                        }
                    };

                    // TODO: move parsing/cursor management to the crawler
                    let curr = event.seq();
                    let mut time = event.time();
                    if let Some((prev, old)) = self.hosts.get(host) {
                        time = time.max(*old);
                        let prev: u64 = (*prev).into();
                        let curr: u64 = curr.into();
                        if prev >= curr {
                            if prev > curr {
                                tracing::debug!(
                                    "[{host}] old msg: {curr} -> {prev} ({})",
                                    prev - curr
                                );
                            }
                            continue;
                        } else if prev + 1 != curr {
                            tracing::trace!(
                                "[{host}] seq gap: {prev} -> {curr} ({})",
                                curr - prev - 1
                            );
                        }
                    }

                    let did = event.did();
                    let (commit, head) = match event.commit() {
                        Ok(Some((commit, head, rev))) => {
                            // run basic commit validation
                            if let SubscribeReposEvent::Commit(commit) = &event {
                                if commit.too_big {
                                    tracing::debug!("[{host}] commit too big: {did}");
                                    continue;
                                }
                                if commit.rebase {
                                    tracing::debug!("[{host}] commit rebase: {did}");
                                    continue;
                                }
                            }
                            if commit.did != did {
                                tracing::debug!(
                                    "[{host}] mismatch inner commit did: {did} -> {}",
                                    commit.did
                                );
                                continue;
                            }
                            if &commit.rev != rev {
                                tracing::debug!(
                                    "[{host}] mismatch inner commit rev: {rev} -> {}",
                                    commit.rev
                                );
                                continue;
                            }
                            (commit, head)
                        }
                        Ok(None) => {
                            if let SubscribeReposEvent::Identity(_) = &event {
                                self.resolver.expire(did, time);
                            }
                            let data = event.serialize(msg.data.len(), seq.next())?;
                            self.firehose.insert(*seq, data)?;
                            self.hosts.insert(host.clone(), (curr, time));
                            continue;
                        }
                        Err(err) => {
                            tracing::debug!("commit decode error: {err}");
                            continue;
                        }
                    };

                    let Some((pds, key)) = self.resolver.resolve(did)? else {
                        self.queue.insert(format!("{did}>{host}>{curr}"), msg.data.to_vec())?;
                        self.hosts.insert(host.clone(), (curr, time));
                        continue;
                    };

                    if let Some(pds) = pds {
                        if host != pds {
                            tracing::debug!("[{did}] hostname mismatch: {host} (expected: {pds})");
                            continue;
                        }
                    }

                    match utils::verify_commit_sig(&commit, key) {
                        Ok(valid) => {
                            if !valid {
                                tracing::debug!("invalid signature: {commit:?} ({key:?})");
                                continue;
                            }
                        }
                        Err(err) => {
                            tracing::debug!("signature error: {err} ({key:?})");
                            continue;
                        }
                    }

                    let rev = commit.rev;
                    let data = commit.data;
                    let entry = self.repos.entry(commit.did);
                    if let Entry::Occupied(prev) = &entry {
                        if !utils::verify_commit_msg(&event, &rev, data, prev.get()) {
                            continue;
                        }
                    }

                    let msg = event.serialize(msg.data.len(), seq.next())?;
                    self.firehose.insert(*seq, msg)?;
                    entry.insert(RepoState { rev, head, data });
                    self.hosts.insert(host.clone(), (curr, time));
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
                    let (k, input) = res?;
                    #[expect(clippy::unwrap_used)]
                    let host =
                        unsafe { std::str::from_utf8_unchecked(&k) }.split('>').nth(1).unwrap();

                    #[expect(clippy::unwrap_used)]
                    let event = SubscribeReposEvent::parse(&input, "")?.unwrap(); // Already tried parsing
                    #[expect(clippy::unwrap_used)]
                    let (commit, head, _) = event.commit()?.unwrap(); // Already tried parsing

                    if let Some(pds) = pds {
                        if host != pds {
                            tracing::debug!("[{did}] hostname mismatch: {host} (expected: {pds})");
                            continue;
                        }
                    }

                    match utils::verify_commit_sig(&commit, key) {
                        Ok(valid) => {
                            if !valid {
                                tracing::debug!("invalid signature: {commit:?} ({key:?})");
                                continue;
                            }
                        }
                        Err(err) => {
                            tracing::debug!("signature error: {err} ({key:?})");
                            continue;
                        }
                    }

                    let rev = commit.rev;
                    let data = commit.data;
                    let entry = self.repos.entry(commit.did);
                    if let Entry::Occupied(prev) = &entry {
                        if !utils::verify_commit_msg(&event, &rev, data, prev.get()) {
                            continue;
                        }
                    }

                    let msg = event.serialize(input.len(), seq.next())?;
                    self.firehose.insert(*seq, msg)?;
                    entry.insert(RepoState { rev, head, data });

                    batch.get_or_insert_default().remove(k);
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
        for (host, (cursor, time)) in &self.hosts {
            tracing::info!("[{host}] persisting cursor: {time}");
            batch.insert(host.as_bytes(), *cursor);
        }
        match DB.open_tree("hosts") {
            Ok(hosts) => {
                if let Err(err) = hosts.apply_batch(batch) {
                    tracing::warn!("unable to persist host state: {err}\n{:#?}", self.hosts);
                }
            }
            Err(err) => {
                tracing::warn!("unable to open hosts tree: {err}\n{:#?}", self.hosts);
            }
        }

        let mut batch = sled::Batch::default();
        for (did, state) in self.repos.drain() {
            #[expect(clippy::unwrap_used)]
            batch.insert(did.into_bytes(), serde_ipld_dagcbor::to_vec(&state).unwrap());
        }
        match DB.open_tree("repos") {
            Ok(repos) => {
                if let Err(err) = repos.apply_batch(batch) {
                    tracing::warn!("unable to persist repo state: {err}");
                }
            }
            Err(err) => {
                tracing::warn!("unable to open repos tree: {err}");
            }
        }

        if let Err(err) = DB.flush() {
            tracing::warn!("unable to flush db: {err}");
        }
    }
}
