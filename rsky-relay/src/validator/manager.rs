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
        this.expire_persist()?;
        Ok(this)
    }

    pub async fn run(mut self) -> Result<(), ManagerError> {
        // TODO: move this to sqlite
        let mut hosts = 0;
        for res in &DB.open_tree("hosts")? {
            let (host, state) = res?;
            #[expect(clippy::unwrap_used)]
            let host = String::from_utf8(host.to_vec()).unwrap();
            self.hosts.insert(host, (state.into(), DateTime::default()));
            hosts += 1;
        }
        // TODO: move this to sqlite
        let mut repos = 0;
        for res in &DB.open_tree("repos")? {
            let (did, state) = res?;
            #[expect(clippy::unwrap_used)]
            let did = String::from_utf8(did.to_vec()).unwrap();
            let state = serde_ipld_dagcbor::from_slice(&state)?;
            self.repos.insert(did, state);
            repos += 1;
        }
        let mut queue = 0;
        for res in &self.queue {
            let (key, _) = res?;
            #[expect(clippy::unwrap_used)]
            let key = std::str::from_utf8(&key).unwrap();
            #[expect(clippy::unwrap_used)]
            self.resolver.resolve(key.split('>').next().unwrap())?;
            queue += 1;
        }
        tracing::info!(%hosts, %repos, %queue, "loaded state");
        let mut seq = self.firehose.last()?.map(|(k, _)| k.into()).unwrap_or_default();
        while self.update(&mut seq).await? {}
        tracing::info!("shutting down validator");
        SHUTDOWN.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn expire_persist(&self) -> Result<(), ManagerError> {
        // expire old firehose data
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
            return Ok(());
        }

        // persist hosts data
        let mut batch = sled::Batch::default();
        for (host, (cursor, _)) in &self.hosts {
            batch.insert(host.as_bytes(), *cursor);
        }
        match DB.open_tree("hosts") {
            Ok(hosts) => {
                if let Err(err) = hosts.apply_batch(batch) {
                    tracing::warn!(%err, "unable to persist host state\n{:#?}", self.hosts);
                }
            }
            Err(err) => {
                tracing::warn!(%err, "unable to open hosts tree\n{:#?}", self.hosts);
            }
        }

        Ok(())
    }

    #[expect(clippy::too_many_lines)]
    async fn update(&mut self, cursor: &mut Cursor) -> Result<bool, ManagerError> {
        if SHUTDOWN.load(Ordering::Relaxed) {
            return Ok(false);
        }

        self.expire_persist()?;

        for _ in 0..1024 {
            match self.message_rx.try_recv_ref() {
                Ok(msg) => {
                    let host = &msg.hostname;
                    let span = tracing::debug_span!("msg_recv", %host, len = %msg.data.len());
                    let _enter = span.enter();
                    let event = match SubscribeReposEvent::parse(&msg.data) {
                        Ok(Some(event)) => event,
                        Ok(None) => continue,
                        Err(err) => {
                            tracing::debug!(%err, "parse error");
                            continue;
                        }
                    };

                    // check/record per-host seq/time
                    let type_ = event.type_();
                    let seq = event.seq();
                    let mut time = event.time();
                    let did = event.did();
                    let span = tracing::debug_span!("msg_data", type = %type_, %seq, %time, %did);
                    let _enter = span.enter();
                    if let Some((prev, old)) = self.hosts.get(host) {
                        time = time.max(*old);
                        let prev: u64 = (*prev).into();
                        let curr: u64 = seq.into();
                        if prev >= curr {
                            if prev > curr {
                                tracing::trace!(%prev, diff = %prev - curr, "old seq");
                            }
                            continue;
                        } else if prev + 1 != curr {
                            tracing::trace!(%prev, diff = %curr - prev - 1, "seq gap");
                        }
                    }

                    // get commit object for #commit/#sync or add to the firehose
                    let span;
                    let _enter;
                    let (commit, head) = match event.commit() {
                        Ok(Some((commit, head))) => {
                            span = tracing::debug_span!("validate", rev = %commit.rev, data = %commit.data, %head);
                            _enter = span.enter();

                            if !event.validate(&commit, &head) {
                                continue;
                            }
                            (commit, head)
                        }
                        Ok(None) => {
                            if let SubscribeReposEvent::Identity(_) = &event {
                                self.resolver.expire(did, event.time());
                            }
                            let data = event.serialize(msg.data.len(), cursor.next())?;
                            self.firehose.insert(*cursor, data)?;
                            self.hosts.insert(host.clone(), (seq, time));
                            continue;
                        }
                        Err(err) => {
                            tracing::debug!(%err, "commit decode error");
                            continue;
                        }
                    };

                    // resolve identity & check pds
                    let Some((pds, key)) = self.resolver.resolve(did)? else {
                        self.queue.insert(format!("{did}>{host}>{seq}"), msg.data.to_vec())?;
                        self.hosts.insert(host.clone(), (seq, time));
                        continue;
                    };

                    if let Some(pds) = pds {
                        if host != pds {
                            // expire the identity & queue message in case the user has migrated
                            self.resolver.expire(did, time);
                            self.queue.insert(format!("{did}>{host}>{seq}"), msg.data.to_vec())?;
                            self.hosts.insert(host.clone(), (seq, time));
                            continue;
                        }
                    }

                    // verify signature
                    match utils::verify_commit_sig(&commit, key) {
                        Ok(valid) => {
                            if !valid {
                                tracing::debug!(?key, "signature mismatch");
                                continue;
                            }
                        }
                        Err(err) => {
                            tracing::debug!(%err, ?key, "signature check error");
                            continue;
                        }
                    }

                    // verify commit message
                    let rev = commit.rev;
                    let data = commit.data;
                    let entry = self.repos.entry(commit.did);
                    if let SubscribeReposEvent::Commit(commit) = &event {
                        // TODO: should still validate records existing in blocks, etc
                        if let Entry::Occupied(prev) = &entry {
                            let prev = prev.get();
                            let span = tracing::debug_span!("previous", rev = %prev.rev, data = %prev.data, head = %prev.head);
                            let _enter = span.enter();
                            if !utils::verify_commit_event(commit, data, prev) {
                                continue;
                            }
                        }
                    }

                    let msg = event.serialize(msg.data.len(), cursor.next())?;
                    self.firehose.insert(*cursor, msg)?;
                    entry.insert(RepoState { rev, data, head });
                    self.hosts.insert(host.clone(), (seq, time));
                }
                Err(thingbuf::mpsc::errors::TryRecvError::Empty) => {}
                Err(thingbuf::mpsc::errors::TryRecvError::Closed) => return Ok(false),
                Err(_) => unreachable!(),
            }

            let mut batch: Option<Batch> = None;
            for did in self.resolver.poll().await? {
                let Some((pds, key)) = self.resolver.resolve(&did)? else {
                    continue;
                };

                for res in self.queue.scan_prefix(&did) {
                    let (k, input) = res?;
                    #[expect(clippy::unwrap_used)]
                    let host = std::str::from_utf8(&k).unwrap().split('>').nth(1).unwrap();
                    let span = tracing::debug_span!("msg_read", %host, len = %input.len());
                    let _enter = span.enter();

                    #[expect(clippy::unwrap_used)]
                    let event = SubscribeReposEvent::parse(&input)?.unwrap(); // already parsed
                    let type_ = event.type_();
                    let seq = event.seq();
                    let time = event.time();
                    let did = event.did();
                    let span = tracing::debug_span!("msg_data", type = %type_, %seq, %time, %did);
                    let _enter = span.enter();

                    #[expect(clippy::unwrap_used)]
                    let (commit, head) = event.commit()?.unwrap(); // already parsed
                    let span = tracing::debug_span!("validate", rev = %commit.rev, data = %commit.data, %head);
                    let _enter = span.enter();

                    if let Some(pds) = pds {
                        if host != pds {
                            tracing::debug!(%pds, "hostname pds mismatch");
                            continue;
                        }
                    }

                    // verify signature
                    match utils::verify_commit_sig(&commit, key) {
                        Ok(valid) => {
                            if !valid {
                                tracing::debug!(?key, "signature mismatch");
                                continue;
                            }
                        }
                        Err(err) => {
                            tracing::debug!(%err, ?key, "signature check error");
                            continue;
                        }
                    }

                    // verify commit message
                    let rev = commit.rev;
                    let data = commit.data;
                    let entry = self.repos.entry(commit.did);
                    if let SubscribeReposEvent::Commit(commit) = &event {
                        // TODO: should still validate records existing in blocks, etc
                        if let Entry::Occupied(prev) = &entry {
                            let prev = prev.get();
                            let span = tracing::debug_span!("previous", rev = %prev.rev, data = %prev.data, head = %prev.head);
                            let _enter = span.enter();
                            if !utils::verify_commit_event(commit, data, prev) {
                                continue;
                            }
                        }
                    }

                    let msg = event.serialize(input.len(), cursor.next())?;
                    self.firehose.insert(*cursor, msg)?;
                    entry.insert(RepoState { rev, data, head });

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
        SHUTDOWN.store(true, Ordering::Relaxed);

        let mut batch = sled::Batch::default();
        for (host, (cursor, time)) in &self.hosts {
            tracing::info!(%time, %cursor, %host, "persisting cursor");
            batch.insert(host.as_bytes(), *cursor);
        }
        match DB.open_tree("hosts") {
            Ok(hosts) => {
                if let Err(err) = hosts.apply_batch(batch) {
                    tracing::warn!(%err, "unable to persist host state\n{:#?}", self.hosts);
                }
            }
            Err(err) => {
                tracing::warn!(%err, "unable to open hosts tree\n{:#?}", self.hosts);
            }
        }

        let len = self.repos.len();
        let mut batch = sled::Batch::default();
        for (did, state) in self.repos.drain() {
            #[expect(clippy::unwrap_used)]
            batch.insert(did.into_bytes(), serde_ipld_dagcbor::to_vec(&state).unwrap());
        }
        tracing::info!(%len, "persisting repos");
        match DB.open_tree("repos") {
            Ok(repos) => {
                if let Err(err) = repos.apply_batch(batch) {
                    tracing::warn!(%err, "unable to persist repo state");
                }
            }
            Err(err) => {
                tracing::warn!(%err, "unable to open repos tree");
            }
        }

        if let Err(err) = DB.flush() {
            tracing::warn!(%err, "unable to flush db");
        }
    }
}
