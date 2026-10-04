use std::convert::Infallible;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTimeError};

use bytes::Bytes;
use chrono::{DateTime, Utc};
use fjall::{Keyspace, PartitionCreateOptions, PartitionHandle, PersistMode};
use hashbrown::HashMap;
use rusqlite::Connection;
use thingbuf::mpsc::errors::{RecvTimeoutError, TryRecvError};
use thiserror::Error;

use crate::config::{HOSTS_WRITE_INTERVAL, LENIENT_VALIDATION, QUEUE_MAX_AGE};
use crate::metrics;
use crate::types::{
    Cursor, DB, HostCursor, META_ADMISSION, MessageReceiver, PARTITION_FIREHOSE,
    PARTITION_HOST_CURSORS, PARTITION_META, PARTITION_PUBLISHED, PARTITION_QUEUE, QueueEntry,
    meta_u64,
};
#[cfg(not(feature = "labeler"))]
use crate::types::{PARTITION_REPOS, published_key};
use crate::validator::event::{Frame, ParseError, SerializeError, SubscribeReposEvent};
use crate::validator::resolver::{IdentityResolver, Resolver, ResolverError};
#[cfg(not(feature = "labeler"))]
use crate::validator::types::{Provenance, RepoState};
use crate::validator::utils;
use crate::{
    HEAD_PROGRESS_AT, HEAD_SEQ, HEAD_TIME, Health, SHUTDOWN, VALIDATOR_LOOPS, set_health, unix_now,
};

const RECV_TIMEOUT: Duration = Duration::from_millis(1);
const BATCH_MESSAGES: usize = 1024;

#[cfg(not(feature = "labeler"))]
const fn commit_ref(commit: &crate::validator::event::Commit) -> &crate::validator::event::Commit {
    commit
}
#[cfg(feature = "labeler")]
const fn commit_ref<'a>(
    commit: &&'a [crate::validator::event::SubscribeLabel],
) -> &'a [crate::validator::event::SubscribeLabel] {
    commit
}

#[cfg(not(feature = "labeler"))]
type RepoUpdate = (String, RepoState);
#[cfg(feature = "labeler")]
type RepoUpdate = ();

#[derive(Debug, Error)]
pub enum ManagerError {
    #[error("parse error: {0}")]
    Parse(#[from] ParseError),
    #[error("serialize error: {0}")]
    Serialize(#[from] SerializeError),
    #[error("resolver error: {0}")]
    Resolver(#[from] ResolverError),
    #[error("time error: {0}")]
    Time(#[from] SystemTimeError),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("fjall error: {0}")]
    Fjall(#[from] fjall::Error),
    #[error("decode error: {0}")]
    DecodeError(#[from] serde_ipld_dagcbor::DecodeError<Infallible>),
    #[error("encode error: {0}")]
    EncodeError(#[from] serde_ipld_dagcbor::EncodeError<std::collections::TryReserveError>),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostState {
    pub generation: u64,
    pub seq: Cursor,
    pub time: DateTime<Utc>,
}

enum Next {
    Message(String, Bytes),
    Empty,
    Closed,
}

/// Where a frame came from: the live ring, or the deferred queue (with its key).
#[derive(Clone)]
enum Origin {
    Live,
    Queued(Vec<u8>),
}

pub struct Manager<R: IdentityResolver = Resolver> {
    message_rx: MessageReceiver,
    keyspace: Keyspace,
    hosts: HashMap<String, HostState>,
    #[cfg(not(feature = "labeler"))]
    repos: HashMap<String, RepoState>,
    resolver: R,
    last: Instant,
    conn: Connection,
    queue: PartitionHandle,
    firehose: PartitionHandle,
    host_cursors: PartitionHandle,
    published: PartitionHandle,
    meta: PartitionHandle,
    #[cfg(not(feature = "labeler"))]
    repos_part: PartitionHandle,
    /// DIDs with deferred entries and when the first one was queued.
    queued_dids: HashMap<String, Instant>,
    admission: u64,
    lenient: bool,
}

impl Manager<Resolver> {
    pub fn new(message_rx: MessageReceiver) -> Result<Self, ManagerError> {
        Self::with_resolver(message_rx, Resolver::new()?)
    }
}

impl<R: IdentityResolver> Manager<R> {
    pub fn with_resolver(message_rx: MessageReceiver, resolver: R) -> Result<Self, ManagerError> {
        Self::with_keyspace(message_rx, resolver, DB.clone(), Path::new("relay.db"))
    }

    pub fn with_keyspace(
        message_rx: MessageReceiver, resolver: R, keyspace: Keyspace, relay_db: &Path,
    ) -> Result<Self, ManagerError> {
        let now = Instant::now();
        let last = now.checked_sub(HOSTS_WRITE_INTERVAL).unwrap_or(now);
        let conn = Connection::open(relay_db)?;
        conn.execute_batch("PRAGMA journal_mode = WAL")?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS hosts (
                host TEXT PRIMARY KEY,
                cursor INTEGER NOT NULL,
                latest TEXT NOT NULL
            )",
            (),
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS banned_hosts (
                host TEXT PRIMARY KEY,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            (),
        )?;
        let queue = keyspace.open_partition(PARTITION_QUEUE, PartitionCreateOptions::default())?;
        let firehose =
            keyspace.open_partition(PARTITION_FIREHOSE, PartitionCreateOptions::default())?;
        let host_cursors =
            keyspace.open_partition(PARTITION_HOST_CURSORS, PartitionCreateOptions::default())?;
        let published =
            keyspace.open_partition(PARTITION_PUBLISHED, PartitionCreateOptions::default())?;
        let meta = keyspace.open_partition(PARTITION_META, PartitionCreateOptions::default())?;
        #[cfg(not(feature = "labeler"))]
        let repos_part =
            keyspace.open_partition(PARTITION_REPOS, PartitionCreateOptions::default())?;
        Ok(Self {
            message_rx,
            keyspace,
            hosts: HashMap::new(),
            #[cfg(not(feature = "labeler"))]
            repos: HashMap::new(),
            resolver,
            last,
            conn,
            queue,
            firehose,
            host_cursors,
            published,
            meta,
            #[cfg(not(feature = "labeler"))]
            repos_part,
            queued_dids: HashMap::new(),
            admission: 0,
            lenient: *LENIENT_VALIDATION,
        })
    }

    #[cfg(all(test, not(feature = "labeler")))]
    pub(crate) const fn set_lenient(&mut self, lenient: bool) {
        self.lenient = lenient;
    }

    fn load_state(&mut self) -> Result<(usize, usize, usize), ManagerError> {
        for res in self.host_cursors.iter() {
            let (host, value) = res?;
            let cursor = HostCursor::decode(&value)?;
            let host = String::from_utf8_lossy(&host).into_owned();
            self.hosts.insert(
                host,
                HostState {
                    generation: cursor.generation,
                    seq: cursor.seq.into(),
                    time: cursor.time,
                },
            );
        }
        #[allow(unused_mut)]
        let mut repos = 0;
        #[cfg(not(feature = "labeler"))]
        {
            self.repos.reserve(self.repos_part.approximate_len());
            for res in self.repos_part.iter() {
                let (did, state) = res?;
                let Ok(did) = String::from_utf8(did.to_vec()) else {
                    tracing::warn!("skipping repo with non-UTF8 key");
                    continue;
                };
                let state = serde_ipld_dagcbor::from_slice(&state)?;
                self.repos.insert(did, state);
                repos += 1;
            }
        }
        // First-queued time is not persisted; after a restart every queued DID
        // gets a fresh expiry window.
        let now = Instant::now();
        for res in self.queue.keys() {
            let key = res?;
            let did = key.as_ref().split(|b| *b == b'>').next().unwrap_or_default();
            self.queued_dids.entry(String::from_utf8_lossy(did).into_owned()).or_insert(now);
        }
        self.admission = meta_u64(&self.meta, META_ADMISSION)?.unwrap_or(0);
        Ok((self.hosts.len(), repos, self.queued_dids.len()))
    }

    pub async fn run(mut self) -> Result<(), ManagerError> {
        let (hosts, repos, queued) = self.load_state()?;
        let mut cursor: Cursor =
            self.firehose.last_key_value()?.map(|(k, _)| k.into()).unwrap_or_default();
        HEAD_SEQ.store(cursor.get(), Ordering::Relaxed);
        HEAD_PROGRESS_AT.store(unix_now(), Ordering::Relaxed);
        let dids: Vec<String> = self.queued_dids.keys().cloned().collect();
        for did in &dids {
            self.scan_did(&mut cursor, did)?;
        }
        tracing::info!(%hosts, %repos, %queued, queue_drained = %(queued - self.queued_dids.len()), %cursor, "loaded state");
        set_health(Health::Live);
        while self.update(&mut cursor).await? {}
        tracing::info!("shutting down validator");
        SHUTDOWN.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Read model for `listHosts`; recovery reads the fjall partition instead.
    fn persist(&mut self) -> Result<(), ManagerError> {
        let tx = self.conn.transaction()?;
        let mut stmt = tx.prepare_cached(
            "
                INSERT INTO hosts (host, cursor, latest)
                VALUES (?1, ?2, ?3)
                ON CONFLICT(host)
                DO UPDATE SET cursor = excluded.cursor, latest = excluded.latest
            ",
        )?;
        for (host, state) in &self.hosts {
            stmt.execute((host, i64::try_from(state.seq.get()).unwrap_or(i64::MAX), state.time))?;
        }
        drop(stmt);
        tx.commit()?;
        Ok(())
    }

    fn next_message(&self, wait: bool) -> Next {
        let msg = if wait {
            match self.message_rx.recv_ref_timeout(RECV_TIMEOUT) {
                Ok(msg) => msg,
                Err(RecvTimeoutError::Closed) => return Next::Closed,
                Err(_) => return Next::Empty,
            }
        } else {
            match self.message_rx.try_recv_ref() {
                Ok(msg) => msg,
                Err(TryRecvError::Closed) => return Next::Closed,
                Err(_) => return Next::Empty,
            }
        };
        let host = msg.hostname.clone();
        let data = msg.data.clone();
        crate::types::intake_bytes_sub(data.len());
        Next::Message(host, data)
    }

    async fn update(&mut self, cursor: &mut Cursor) -> Result<bool, ManagerError> {
        if SHUTDOWN.load(Ordering::Relaxed) {
            return Ok(false);
        }
        VALIDATOR_LOOPS.fetch_add(1, Ordering::Relaxed);
        metrics::record_validator_loop();

        let now = Instant::now();
        if self.last + HOSTS_WRITE_INTERVAL < now {
            self.persist()?;
            self.last = now;
        }

        let mut consumed = 0usize;
        for _ in 0..BATCH_MESSAGES {
            match self.next_message(false) {
                Next::Message(host, data) => {
                    consumed += 1;
                    self.handle_frame(cursor, &host, &data, &Origin::Live)?;
                }
                Next::Empty => break,
                Next::Closed => return Ok(false),
            }
        }
        if consumed == 0 {
            // An empty ring is progress: nothing is waiting on the validator.
            HEAD_PROGRESS_AT.store(unix_now(), Ordering::Relaxed);
            match self.next_message(true) {
                Next::Message(host, data) => {
                    self.handle_frame(cursor, &host, &data, &Origin::Live)?;
                }
                Next::Empty => {}
                Next::Closed => return Ok(false),
            }
        }
        metrics::record_intake(crate::types::intake_bytes(), self.message_rx.len());

        for did in self.resolver.poll().await? {
            self.scan_did(cursor, &did)?;
        }

        Ok(true)
    }

    fn drop_frame(&self, reason: &'static str, origin: &Origin) -> Result<(), ManagerError> {
        metrics::record_validator_dropped(reason);
        if let Origin::Queued(key) = origin {
            self.queue.remove(key.as_slice())?;
        }
        Ok(())
    }

    #[expect(clippy::too_many_lines)]
    #[allow(clippy::cognitive_complexity)]
    fn handle_frame(
        &mut self, cursor: &mut Cursor, host: &str, data: &Bytes, origin: &Origin,
    ) -> Result<(), ManagerError> {
        let span = tracing::info_span!("msg_recv", %host, len = %data.len());
        let _enter = span.enter();
        let event = match SubscribeReposEvent::parse_frame(data) {
            Ok(Frame::Event(event)) => event,
            Ok(Frame::Info { name, message }) => {
                tracing::debug!(%name, %message, "upstream #info");
                metrics::record_upstream_error(host, &name);
                return self.drop_frame("info_frame", origin);
            }
            Ok(Frame::Error { name, message }) => {
                tracing::warn!(%name, %message, "upstream error frame");
                metrics::record_upstream_error(host, &name);
                return self.drop_frame("error_frame", origin);
            }
            Err(err) => {
                tracing::debug!(%err, "parse error");
                return self.drop_frame("parse_error", origin);
            }
        };

        let type_ = event.type_();
        let seq = event.seq();
        let mut time = event.time();
        let did = event.did().to_owned();
        let generation = self.hosts.get(host).map_or(0, |state| state.generation);
        let span = tracing::debug_span!("msg_data", type = %type_, %seq, %time, %did);
        let _enter = span.enter();
        if matches!(origin, Origin::Live) {
            if let Some(state) = self.hosts.get(host) {
                time = time.max(state.time);
                if state.seq.get() >= seq.get() {
                    tracing::trace!(prev = %state.seq, "old seq");
                    return self.drop_frame("old_seq", origin);
                }
            }
        }

        let lenient = self.lenient;

        #[allow(unused_variables)]
        let (commit, head) = match event.commit() {
            Ok(Some((commit, head))) => {
                #[cfg(not(feature = "labeler"))]
                if !event.validate(&commit, &head) {
                    return self.drop_frame("envelope_invalid", origin);
                }
                (commit, head)
            }
            Ok(None) => {
                if let SubscribeReposEvent::Identity(_) = &event {
                    self.resolver.expire(&did, event.time());
                }
                return self.publish(
                    cursor,
                    host,
                    generation,
                    seq,
                    time,
                    event,
                    data.len(),
                    None,
                    None,
                    origin,
                );
            }
            Err(err) => {
                tracing::debug!(%err, "commit decode error");
                return self.drop_frame("commit_decode", origin);
            }
        };

        #[cfg(not(feature = "labeler"))]
        let identity = published_key(&did, &commit.rev.to_string(), &head);
        #[cfg(not(feature = "labeler"))]
        if matches!(origin, Origin::Live) {
            if self.published.contains_key(&identity)? {
                return self.drop_frame("replayed", origin);
            }
            if let SubscribeReposEvent::Commit(_) = &event {
                if let Some(state) = self.repos.get(&did) {
                    if state.provenance == Provenance::Verified
                        && !state.rev.older_than(&commit.rev)
                    {
                        return self.drop_frame("replayed_by_rev", origin);
                    }
                }
            }
        }

        // resolve identity & check pds (trait returns owned values so the resolver isn't borrowed)
        let (pds_owned, key_owned) = if let Some((pds, key)) = self.resolver.resolve_owned(&did)? {
            (pds, Some(key))
        } else {
            if !lenient {
                if matches!(origin, Origin::Live) {
                    self.defer(host, generation, seq, time, &did, data, "resolver_pending")?;
                }
                return Ok(());
            }
            self.resolver.request_direct(&did);
            tracing::debug!("resolver pending; publishing under lenient mode");
            metrics::record_validator_passed_with_warning("resolver_pending");
            (None, None)
        };

        let pds_mismatch = pds_owned.as_deref().is_some_and(|p| host != p);
        if pds_mismatch {
            self.resolver.request_direct(&did);
            if !lenient {
                match origin {
                    Origin::Live => {
                        tracing::debug!(?pds_owned, "hostname pds mismatch, deferring");
                        self.defer(host, generation, seq, time, &did, data, "pds_mismatch")?;
                        return Ok(());
                    }
                    Origin::Queued(_) => return self.drop_frame("pds_mismatch", origin),
                }
            }
            tracing::warn!(?pds_owned, "PDS mismatch; publishing under lenient mode");
            metrics::record_validator_passed_with_warning("pds_mismatch");
        }

        #[allow(unused_variables)]
        let verified = match key_owned
            .as_ref()
            .map(|key| (key, utils::verify_commit_sig(commit_ref(&commit), key)))
        {
            None => false,
            Some((_, Ok(true))) => true,
            Some((key, outcome)) => {
                let reason = match &outcome {
                    Ok(_) => "sig_fail",
                    Err(err) => {
                        tracing::debug!(%err, ?key, "signature check error");
                        "sig_check_error"
                    }
                };
                if !lenient {
                    return self.drop_frame(reason, origin);
                }
                tracing::warn!(?key, %reason, "signature not verified; publishing under lenient mode");
                metrics::record_validator_passed_with_warning(reason);
                false
            }
        };

        #[cfg(not(feature = "labeler"))]
        let repo_update = {
            let (rev, data_cid) = (commit.rev, commit.data);
            // A deferred commit older than verified state is authentic but late:
            // it cannot be inverted against newer state and must not rewind it.
            let late = matches!(origin, Origin::Queued(_))
                && self.repos.get(&did).is_some_and(|state| !state.rev.older_than(&rev));
            if late {
                metrics::record_validator_passed_with_warning("late");
            } else if let SubscribeReposEvent::Commit(commit_event) = &event {
                // TODO: should still validate records existing in blocks, etc
                if let Some(prev) = self.repos.get(&did) {
                    let span = tracing::debug_span!("previous", rev = %prev.rev, data = %prev.data, head = %prev.head);
                    let _enter = span.enter();
                    if !utils::verify_commit_event(commit_event, data_cid, prev) {
                        if !lenient {
                            return self.drop_frame("mst_fail", origin);
                        }
                        tracing::warn!("MST verify failed; publishing under lenient mode");
                        metrics::record_validator_passed_with_warning("mst_fail");
                    }
                }
            }
            if !verified {
                metrics::record_validator_passed_with_warning("unverified_replay_candidate");
            }
            (verified && !late).then(|| {
                (
                    did.clone(),
                    RepoState { rev, data: data_cid, head, provenance: Provenance::Verified },
                )
            })
        };
        #[cfg(feature = "labeler")]
        let repo_update = None;
        #[cfg(feature = "labeler")]
        let identity = Vec::new();

        self.publish(
            cursor,
            host,
            generation,
            seq,
            time,
            event,
            data.len(),
            Some(identity),
            repo_update,
            origin,
        )
    }

    #[expect(clippy::too_many_arguments)]
    fn publish(
        &mut self, cursor: &mut Cursor, host: &str, generation: u64, seq: Cursor,
        time: DateTime<Utc>, event: SubscribeReposEvent, frame_len: usize,
        identity: Option<Vec<u8>>, repo_update: Option<RepoUpdate>, origin: &Origin,
    ) -> Result<(), ManagerError> {
        let event_type = event.type_();
        let event_time = event.time();
        let msg = event.serialize(frame_len, cursor.next())?;
        let mut batch = self.keyspace.batch();
        batch.insert(&self.firehose, *cursor, msg);
        if let Some(identity) = identity.filter(|k| !k.is_empty()) {
            batch.insert(&self.published, identity, cursor.as_ref());
        }
        if let Some(state) = self.advance_host(host, generation, seq, time) {
            batch.insert(&self.host_cursors, host, state.encode()?);
        }
        #[cfg(not(feature = "labeler"))]
        if let Some((did, state)) = repo_update {
            batch.insert(&self.repos_part, did.as_bytes(), serde_ipld_dagcbor::to_vec(&state)?);
            self.repos.insert(did, state);
        }
        #[cfg(feature = "labeler")]
        let _: Option<RepoUpdate> = repo_update;
        if let Origin::Queued(key) = origin {
            batch.remove(&self.queue, key.as_slice());
        }
        batch.commit()?;
        metrics::record_validator_published(event_type);
        metrics::record_firehose_head(cursor.get());
        HEAD_SEQ.store(cursor.get(), Ordering::Relaxed);
        HEAD_TIME.store(event_time.timestamp(), Ordering::Relaxed);
        HEAD_PROGRESS_AT.store(unix_now(), Ordering::Relaxed);
        Ok(())
    }

    /// Host checkpoints only move forward within a generation, so draining a
    /// deferred event can never rewind the crawl position.
    fn advance_host(
        &mut self, host: &str, generation: u64, seq: Cursor, time: DateTime<Utc>,
    ) -> Option<HostCursor> {
        let advance = self.hosts.get(host).is_none_or(|state| {
            generation > state.generation
                || (generation == state.generation && seq.get() > state.seq.get())
        });
        if !advance {
            return None;
        }
        self.hosts.insert(host.to_owned(), HostState { generation, seq, time });
        Some(HostCursor { generation, seq: seq.get(), time })
    }

    #[expect(clippy::too_many_arguments)]
    fn defer(
        &mut self, host: &str, generation: u64, seq: Cursor, time: DateTime<Utc>, did: &str,
        data: &Bytes, reason: &'static str,
    ) -> Result<(), ManagerError> {
        self.admission += 1;
        let entry =
            QueueEntry { host: host.to_owned(), generation, seq: seq.get(), frame: data.to_vec() };
        let mut batch = self.keyspace.batch();
        batch.insert(&self.queue, QueueEntry::key(did, self.admission), entry.encode()?);
        batch.insert(&self.meta, META_ADMISSION, self.admission.to_be_bytes());
        if let Some(state) = self.advance_host(host, generation, seq, time) {
            batch.insert(&self.host_cursors, host, state.encode()?);
        }
        batch.commit()?;
        self.queued_dids.entry(did.to_owned()).or_insert_with(Instant::now);
        metrics::record_validator_deferred(reason);
        Ok(())
    }

    fn scan_did(&mut self, cursor: &mut Cursor, did: &str) -> Result<(), ManagerError> {
        let Some(&since) = self.queued_dids.get(did) else {
            return Ok(());
        };
        let prefix = QueueEntry::prefix(did);
        if self.resolver.resolve_owned(did)?.is_none() {
            if since.elapsed() <= QUEUE_MAX_AGE {
                return Ok(());
            }
            let mut batch = self.keyspace.batch();
            let mut evicted = 0u64;
            for res in self.queue.prefix(&prefix) {
                let (k, _) = res?;
                batch.remove(&self.queue, k);
                evicted += 1;
            }
            batch.commit()?;
            for _ in 0..evicted {
                metrics::record_validator_dropped("queue_expired");
            }
            tracing::warn!(%evicted, %did, "dropping deferred events for a DID unresolved past the queue age");
            self.queued_dids.remove(did);
            return Ok(());
        }
        let entries = self.queue.prefix(&prefix).collect::<Result<Vec<_>, _>>()?;
        for (key, value) in entries {
            let entry = match QueueEntry::decode(&value) {
                Ok(entry) => entry,
                Err(err) => {
                    tracing::warn!(%err, "dropping undecodable queue entry");
                    self.queue.remove(key)?;
                    continue;
                }
            };
            let origin = Origin::Queued(key.to_vec());
            self.handle_frame(cursor, &entry.host, &Bytes::from(entry.frame), &origin)?;
        }
        if self.queue.prefix(&prefix).next().is_none() {
            self.queued_dids.remove(did);
        }
        Ok(())
    }
}

impl<R: IdentityResolver> Drop for Manager<R> {
    fn drop(&mut self) {
        if let Err(err) = self.persist() {
            tracing::warn!(%err, "unable to persist host state");
        }
        if let Err(err) = self.keyspace.persist(PersistMode::SyncAll) {
            tracing::warn!(%err, "unable to flush db");
        }
    }
}

#[cfg(all(test, not(feature = "labeler")))]
mod tests {
    use super::*;

    use crate::types::{MessageRecycle, MessageSender, PARTITION_QUEUE_LEGACY, open_keyspace};
    use crate::validator::resolver::tests::FakeResolver;
    use crate::validator::testutil::{
        CommitSpec, TestKey, account_frame, broken_car_frame, commit_frame, data_cid, error_frame,
        identity_frame, info_frame, sync_frame, test_key,
    };
    use ::metrics::with_local_recorder;
    use metrics_exporter_prometheus::PrometheusBuilder;

    use crate::shutdown_guard;

    const DID: &str = "did:plc:aaaaaaaaaaaaaaaaaaaaaaaa";
    const HOST: &str = "pds.example";

    struct Harness {
        manager: Manager<FakeResolver>,
        tx: MessageSender,
        cursor: Cursor,
        _tmp: tempfile::TempDir,
        keyspace: Keyspace,
        relay_db: std::path::PathBuf,
        key: TestKey,
    }

    fn harness() -> Harness {
        let tmp = tempfile::tempdir().unwrap();
        let keyspace = open_keyspace(&tmp.path().join("db")).unwrap();
        let relay_db = tmp.path().join("relay.db");
        let (tx, rx) = thingbuf::mpsc::blocking::with_recycle(64, MessageRecycle);
        let manager =
            Manager::with_keyspace(rx, FakeResolver::new(), keyspace.clone(), &relay_db).unwrap();
        Harness {
            manager,
            tx,
            cursor: Cursor::default(),
            _tmp: tmp,
            keyspace,
            relay_db,
            key: test_key(1),
        }
    }

    impl Harness {
        fn resolved(&mut self) {
            self.manager
                .resolver
                .script
                .push_back(Ok(Some((Some(HOST.to_owned()), self.key.did_key))));
        }

        fn resolved_as(&mut self, host: &str, key: [u8; 35]) {
            self.manager.resolver.script.push_back(Ok(Some((Some(host.to_owned()), key))));
        }

        fn unresolved(&mut self) {
            self.manager.resolver.script.push_back(Ok(None));
        }

        fn live(&mut self, host: &str, frame: Vec<u8>) {
            let bytes = Bytes::from(frame);
            self.manager.handle_frame(&mut self.cursor, host, &bytes, &Origin::Live).unwrap();
        }

        fn commit(&self, rev: &str, seq: u64, data: u8, prev: Option<u8>) -> Vec<u8> {
            commit_frame(&CommitSpec {
                did: DID,
                rev,
                seq,
                data: data_cid(data),
                prev_data: prev.map(data_cid),
                key: &self.key,
                sig_override: None,
                too_big: false,
            })
            .0
        }

        fn firehose_len(&self) -> usize {
            self.manager.firehose.len().unwrap()
        }

        fn queue_len(&self) -> usize {
            self.manager.queue.len().unwrap()
        }

        fn published_len(&self) -> usize {
            self.manager.published.len().unwrap()
        }

        fn host_seq(&self, host: &str) -> Option<u64> {
            self.manager
                .host_cursors
                .get(host)
                .unwrap()
                .map(|v| HostCursor::decode(&v).unwrap().seq)
        }
    }

    fn rendered<F: FnOnce()>(f: F) -> String {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        with_local_recorder(&recorder, f);
        handle.render()
    }

    #[test]
    fn identity_and_account_publish_and_identity_expires_resolver() {
        let mut h = harness();
        h.live(HOST, identity_frame(DID, 1, Some("h.test")));
        h.live(HOST, account_frame(DID, 2, true));
        assert_eq!(h.firehose_len(), 2);
        assert_eq!(h.published_len(), 0, "identity/account carry no replay identity");
        assert_eq!(h.manager.resolver.expirations.len(), 1);
        assert_eq!(h.host_seq(HOST), Some(2));
        assert_eq!(h.cursor.get(), 2);
        assert_eq!(HEAD_SEQ.load(Ordering::Relaxed), 2);
        // old seq is dropped
        h.live(HOST, account_frame(DID, 2, true));
        assert_eq!(h.firehose_len(), 2);
    }

    #[test]
    fn unresolved_lenient_commit_publishes_once_and_replay_is_dropped() {
        let mut h = harness();
        let frame = h.commit("3mw73ekioet25", 10, 1, None);
        h.unresolved();
        let out = rendered(|| h.live(HOST, frame.clone()));
        assert!(out.contains("resolver_pending"), "{out}");
        assert!(out.contains("unverified_replay_candidate"), "{out}");
        assert_eq!(h.manager.resolver.direct_requests, vec![DID.to_owned()]);
        assert_eq!(h.firehose_len(), 1);
        assert_eq!(h.published_len(), 1);
        assert!(h.manager.repos.get(DID).is_none(), "unverified commits never touch repo state");
        // a replay from a host reset (lower seq allowed via a different host) is dropped by identity
        h.unresolved();
        let out = rendered(|| h.live("other.example", frame.clone()));
        assert!(out.contains("reason=\"replayed\""), "{out}");
        assert_eq!(h.firehose_len(), 1);
    }

    #[test]
    fn verified_commit_updates_state_and_rev_backstop_drops_old_revs() {
        let mut h = harness();
        h.resolved();
        h.live(HOST, h.commit("3mw73ekioet25", 1, 1, None));
        let state = h.manager.repos.get(DID).unwrap().clone();
        assert_eq!(state.provenance, Provenance::Verified);
        assert_eq!(state.data, data_cid(1));
        // an older rev with a new cid (forged or replayed) is dropped even unverified
        let older = h.commit("3mw73ekioet24", 2, 2, None);
        let out = rendered(|| h.live(HOST, older));
        assert!(out.contains("replayed_by_rev"), "{out}");
        assert_eq!(h.firehose_len(), 1);
        // legacy state does not back the check
        h.manager.repos.get_mut(DID).unwrap().provenance = Provenance::Legacy;
        let older = h.commit("3mw73ekioet23", 3, 3, None);
        h.resolved();
        h.live(HOST, older);
        assert_eq!(h.firehose_len(), 2);
    }

    #[test]
    fn forged_high_rev_cannot_suppress_authentic_commit() {
        let mut h = harness();
        h.resolved();
        h.live(HOST, h.commit("3mw73ekioet25", 1, 1, None));
        // forged: bad signature, higher rev; lenient publishes it with a warning
        let forged = commit_frame(&CommitSpec {
            did: DID,
            rev: "3mw73ekioet99",
            seq: 2,
            data: data_cid(7),
            prev_data: Some(data_cid(1)),
            key: &test_key(9),
            sig_override: None,
            too_big: false,
        })
        .0;
        h.resolved();
        let out = rendered(|| h.live(HOST, forged));
        assert!(out.contains("sig_fail"), "{out}");
        assert_eq!(h.manager.repos.get(DID).unwrap().rev.to_string(), "3mw73ekioet25");
        // the authentic next commit still publishes and advances state
        h.resolved();
        h.live(HOST, h.commit("3mw73ekioet26", 3, 2, Some(1)));
        assert_eq!(h.manager.repos.get(DID).unwrap().rev.to_string(), "3mw73ekioet26");
        assert_eq!(h.firehose_len(), 3);
    }

    #[test]
    fn same_rev_different_cid_is_not_suppressed_by_forged_first() {
        let mut h = harness();
        let forged = commit_frame(&CommitSpec {
            did: DID,
            rev: "3mw73ekioet25",
            seq: 1,
            data: data_cid(5),
            prev_data: None,
            key: &test_key(9),
            sig_override: None,
            too_big: false,
        })
        .0;
        h.resolved();
        h.live(HOST, forged);
        h.resolved();
        h.live(HOST, h.commit("3mw73ekioet25", 2, 1, None));
        assert_eq!(h.firehose_len(), 2);
        assert_eq!(h.published_len(), 2);
    }

    #[test]
    fn strict_mode_defers_unresolved_and_drains_once_resolved() {
        let mut h = harness();
        h.manager.set_lenient(false);
        let frame = h.commit("3mw73ekioet25", 5, 1, None);
        h.unresolved();
        let out = rendered(|| h.live(HOST, frame.clone()));
        assert!(out.contains("deferred"), "{out}");
        assert_eq!(h.firehose_len(), 0);
        assert_eq!(h.queue_len(), 1);
        assert_eq!(h.host_seq(HOST), Some(5), "admission checkpoints the host cursor");
        assert!(h.manager.queued_dids.contains_key(DID));
        assert_eq!(meta_u64(&h.manager.meta, META_ADMISSION).unwrap(), Some(1));
        // resolver still pending: scan is a no-op that keeps the entry
        h.unresolved();
        h.manager.scan_did(&mut h.cursor, DID).unwrap();
        assert_eq!(h.queue_len(), 1);
        // resolved: scan_did (resolve) + handle_frame (resolve) publish exactly once
        h.resolved();
        h.resolved();
        h.manager.scan_did(&mut h.cursor, DID).unwrap();
        assert_eq!(h.queue_len(), 0);
        assert_eq!(h.firehose_len(), 1);
        assert!(!h.manager.queued_dids.contains_key(DID));
        assert_eq!(h.manager.repos.get(DID).unwrap().provenance, Provenance::Verified);
        // unknown DID scan is a no-op
        h.manager.scan_did(&mut h.cursor, "did:plc:nobody").unwrap();
    }

    #[test]
    fn strict_mode_expires_deferred_events_after_queue_max_age() {
        let mut h = harness();
        h.manager.set_lenient(false);
        h.unresolved();
        h.live(HOST, h.commit("3mw73ekioet25", 5, 1, None));
        h.unresolved();
        h.live(HOST, h.commit("3mw73ekioet26", 6, 2, Some(1)));
        assert_eq!(h.queue_len(), 2);
        *h.manager.queued_dids.get_mut(DID).unwrap() =
            Instant::now().checked_sub(QUEUE_MAX_AGE + Duration::from_secs(1)).unwrap();
        h.unresolved();
        let out = rendered(|| h.manager.scan_did(&mut h.cursor, DID).unwrap());
        assert!(out.contains("queue_expired"), "{out}");
        assert_eq!(h.queue_len(), 0);
        assert!(!h.manager.queued_dids.contains_key(DID));
    }

    #[test]
    fn deferred_events_drain_in_admission_order_across_hosts() {
        let mut h = harness();
        h.manager.set_lenient(false);
        h.unresolved();
        h.live("a.example", h.commit("3mw73ekioet25", 1, 1, None));
        h.unresolved();
        h.live("zzz.example", h.commit("3mw73ekioet26", 1, 2, Some(1)));
        // resolve as the second host so the first entry is a pds mismatch and is dropped
        for _ in 0..3 {
            h.resolved_as("zzz.example", h.key.did_key);
        }
        let out = rendered(|| h.manager.scan_did(&mut h.cursor, DID).unwrap());
        assert!(out.contains("pds_mismatch"), "{out}");
        assert_eq!(h.queue_len(), 0);
        assert_eq!(h.firehose_len(), 1);
        assert_eq!(h.manager.repos.get(DID).unwrap().rev.to_string(), "3mw73ekioet26");
    }

    #[test]
    fn late_deferred_commit_publishes_without_rewinding_state() {
        let mut h = harness();
        h.manager.set_lenient(false);
        h.unresolved();
        h.live(HOST, h.commit("3mw73ekioet25", 1, 1, None));
        h.manager.set_lenient(true);
        h.resolved();
        h.live(HOST, h.commit("3mw73ekioet27", 2, 3, None));
        assert_eq!(h.manager.repos.get(DID).unwrap().rev.to_string(), "3mw73ekioet27");
        h.resolved();
        h.resolved();
        let out = rendered(|| h.manager.scan_did(&mut h.cursor, DID).unwrap());
        assert!(out.contains("reason=\"late\""), "{out}");
        assert_eq!(h.firehose_len(), 2);
        assert_eq!(h.manager.repos.get(DID).unwrap().rev.to_string(), "3mw73ekioet27");
        assert_eq!(h.queue_len(), 0);
    }

    #[test]
    fn pds_mismatch_paths() {
        let mut h = harness();
        // lenient: published with warning, direct fetch requested
        h.resolved_as("elsewhere.example", h.key.did_key);
        let out = rendered(|| h.live(HOST, h.commit("3mw73ekioet25", 1, 1, None)));
        assert!(out.contains("pds_mismatch"), "{out}");
        assert_eq!(h.firehose_len(), 1);
        assert_eq!(h.manager.resolver.direct_requests, vec![DID.to_owned()]);
        // strict live: deferred
        h.manager.set_lenient(false);
        h.resolved_as("elsewhere.example", h.key.did_key);
        h.live(HOST, h.commit("3mw73ekioet26", 2, 2, Some(1)));
        assert_eq!(h.queue_len(), 1);
    }

    #[test]
    fn signature_failure_paths() {
        let mut h = harness();
        let bad_sig = commit_frame(&CommitSpec {
            did: DID,
            rev: "3mw73ekioet25",
            seq: 1,
            data: data_cid(1),
            prev_data: None,
            key: &h.key,
            sig_override: Some(vec![1, 2, 3]),
            too_big: false,
        })
        .0;
        h.resolved();
        let out = rendered(|| h.live(HOST, bad_sig.clone()));
        assert!(out.contains("sig_check_error"), "{out}");
        assert_eq!(h.firehose_len(), 1);
        h.manager.set_lenient(false);
        let wrong_key = commit_frame(&CommitSpec {
            did: DID,
            rev: "3mw73ekioet26",
            seq: 2,
            data: data_cid(1),
            prev_data: None,
            key: &test_key(3),
            sig_override: None,
            too_big: false,
        })
        .0;
        h.resolved();
        let out = rendered(|| h.live(HOST, wrong_key));
        assert!(out.contains("reason=\"sig_fail\""), "{out}");
        let bad_sig2 = commit_frame(&CommitSpec {
            did: DID,
            rev: "3mw73ekioet27",
            seq: 3,
            data: data_cid(1),
            prev_data: None,
            key: &h.key,
            sig_override: Some(vec![9]),
            too_big: false,
        })
        .0;
        h.resolved();
        let out = rendered(|| h.live(HOST, bad_sig2));
        assert!(out.contains("reason=\"sig_check_error\""), "{out}");
        assert_eq!(h.firehose_len(), 1);
    }

    #[test]
    fn mst_failure_paths() {
        let mut h = harness();
        h.resolved();
        h.live(HOST, h.commit("3mw73ekioet25", 1, 1, None));
        // missing prevData against known state fails MST verification
        h.resolved();
        let out = rendered(|| h.live(HOST, h.commit("3mw73ekioet26", 2, 2, None)));
        assert!(out.contains("mst_fail"), "{out}");
        assert_eq!(h.firehose_len(), 2);
        h.manager.set_lenient(false);
        h.resolved();
        let out = rendered(|| h.live(HOST, h.commit("3mw73ekioet27", 3, 3, None)));
        assert!(out.contains("reason=\"mst_fail\""), "{out}");
        assert_eq!(h.firehose_len(), 2);
    }

    #[test]
    fn malformed_and_signal_frames_are_counted_not_published() {
        let mut h = harness();
        let out = rendered(|| {
            h.live(HOST, vec![0xff, 0x00]);
            h.live(HOST, error_frame("FutureCursor"));
            h.live(HOST, info_frame("OutdatedCursor"));
            h.live(HOST, broken_car_frame(DID, 1));
            let (too_big, _) = commit_frame(&CommitSpec {
                did: DID,
                rev: "3mw73ekioet25",
                seq: 2,
                data: data_cid(1),
                prev_data: None,
                key: &h.key,
                sig_override: None,
                too_big: true,
            });
            h.live(HOST, too_big);
        });
        for reason in
            ["parse_error", "error_frame", "info_frame", "commit_decode", "envelope_invalid"]
        {
            assert!(out.contains(&format!("reason=\"{reason}\"")), "{reason}: {out}");
        }
        assert!(out.contains("name=\"FutureCursor\""), "{out}");
        assert_eq!(h.firehose_len(), 0);
    }

    #[test]
    fn sync_events_publish_with_identity_and_verified_state() {
        let mut h = harness();
        let (frame, head) = sync_frame(DID, "3mw73ekioet25", 1, &h.key);
        h.resolved();
        let out = rendered(|| h.live(HOST, frame.clone()));
        assert!(!out.contains("passed_with_warning"), "sync must verify cleanly: {out}");
        assert_eq!(h.published_len(), 1);
        let key = published_key(DID, "3mw73ekioet25", &head);
        assert!(h.manager.published.contains_key(&key).unwrap());
        assert_eq!(h.manager.repos.get(DID).unwrap().head, head);
        h.resolved();
        h.live("other.example", frame);
        assert_eq!(h.firehose_len(), 1, "replayed sync is dropped by identity");
    }

    #[tokio::test(flavor = "current_thread")]
    #[expect(clippy::await_holding_lock)]
    async fn update_drains_ring_waits_when_empty_and_stops_when_closed() {
        let _g = shutdown_guard();
        SHUTDOWN.store(false, Ordering::Relaxed);
        let mut h = harness();
        {
            let mut slot = h.tx.send_ref().unwrap();
            slot.hostname = HOST.to_owned();
            slot.data = Bytes::from(identity_frame(DID, 1, None));
        }
        crate::types::intake_bytes_add(10);
        let mut cursor = Cursor::default();
        assert!(h.manager.update(&mut cursor).await.unwrap());
        assert_eq!(h.manager.firehose.len().unwrap(), 1);
        // empty ring: waits RECV_TIMEOUT and reports progress
        HEAD_PROGRESS_AT.store(0, Ordering::Relaxed);
        assert!(h.manager.update(&mut cursor).await.unwrap());
        assert!(HEAD_PROGRESS_AT.load(Ordering::Relaxed) > 0);
        // a message arriving during the wait is handled
        let tx = h.tx.clone();
        let sender = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_micros(200));
            let mut slot = tx.send_ref().unwrap();
            slot.hostname = HOST.to_owned();
            slot.data = Bytes::from(account_frame(DID, 2, true));
        });
        for _ in 0..50 {
            h.manager.update(&mut cursor).await.unwrap();
            if h.manager.firehose.len().unwrap() == 2 {
                break;
            }
        }
        sender.join().unwrap();
        assert_eq!(h.manager.firehose.len().unwrap(), 2);
        // resolver results drive deferred-queue scans
        h.manager.set_lenient(false);
        h.unresolved();
        h.live(HOST, h.commit("3mw73ekioet25", 9, 1, None));
        assert_eq!(h.manager.queue.len().unwrap(), 1);
        h.manager.resolver.polls.push_back(Ok(vec![DID.to_owned()]));
        h.resolved();
        h.resolved();
        assert!(h.manager.update(&mut cursor).await.unwrap());
        assert_eq!(h.manager.queue.len().unwrap(), 0);
        // closed channel ends the loop
        drop(h.tx);
        assert!(!h.manager.update(&mut cursor).await.unwrap());
        SHUTDOWN.store(true, Ordering::Relaxed);
        assert!(!h.manager.update(&mut cursor).await.unwrap());
        SHUTDOWN.store(false, Ordering::Relaxed);
    }

    #[tokio::test(flavor = "current_thread")]
    #[expect(clippy::await_holding_lock)]
    async fn run_loads_state_drains_queue_and_persists_read_model() {
        let _g = shutdown_guard();
        SHUTDOWN.store(false, Ordering::Relaxed);
        let mut h = harness();
        h.manager.set_lenient(false);
        h.unresolved();
        h.live(HOST, h.commit("3mw73ekioet25", 7, 1, None));
        h.resolved();
        h.live("b.example", identity_frame("did:plc:other", 3, None));
        h.manager.persist().unwrap();
        // fresh manager over the same keyspace: hosts, admission and queued DIDs reload
        let (tx, rx) = thingbuf::mpsc::blocking::with_recycle(8, MessageRecycle);
        let mut fresh =
            Manager::with_keyspace(rx, FakeResolver::new(), h.keyspace.clone(), &h.relay_db)
                .unwrap();
        fresh.set_lenient(false);
        // a verified repo state and a junk-keyed one: the first reloads, the second is skipped
        h.manager
            .repos_part
            .insert(
                b"did:plc:reloaded",
                serde_ipld_dagcbor::to_vec(&RepoState {
                    rev: rsky_common::tid::TID::new("3mw73ekioet25".to_owned()).unwrap(),
                    data: data_cid(1),
                    head: data_cid(2),
                    provenance: Provenance::Verified,
                })
                .unwrap(),
            )
            .unwrap();
        h.manager.repos_part.insert(&[0xffu8, 0xfe][..], b"x".as_slice()).unwrap();
        let (hosts, repos, queued) = fresh.load_state().unwrap();
        assert_eq!((hosts, repos, queued), (2, 1, 1));
        assert_eq!(fresh.repos.get("did:plc:reloaded").unwrap().provenance, Provenance::Verified);
        assert_eq!(fresh.admission, 1);
        // run() drains the queue once resolvable, then exits on close
        fresh.resolver.script.push_back(Ok(Some((Some(HOST.to_owned()), h.key.did_key))));
        fresh.resolver.script.push_back(Ok(Some((Some(HOST.to_owned()), h.key.did_key))));
        drop(tx);
        fresh.run().await.unwrap();
        assert_eq!(crate::health(), Health::Live);
        SHUTDOWN.store(false, Ordering::Relaxed);
        let conn = Connection::open(&h.relay_db).unwrap();
        let rows: i64 = conn.query_row("SELECT COUNT(*) FROM hosts", [], |r| r.get(0)).unwrap();
        assert_eq!(rows, 2);
        let queue =
            h.keyspace.open_partition(PARTITION_QUEUE, PartitionCreateOptions::default()).unwrap();
        assert!(queue.is_empty().unwrap());
        let firehose = h
            .keyspace
            .open_partition(crate::types::PARTITION_FIREHOSE, PartitionCreateOptions::default())
            .unwrap();
        assert_eq!(firehose.len().unwrap(), 2);
        let _ = PARTITION_QUEUE_LEGACY;
    }

    #[test]
    fn undecodable_queue_entries_are_dropped_during_scan() {
        let mut h = harness();
        h.manager.queue.insert(QueueEntry::key(DID, 1), b"junk".as_slice()).unwrap();
        h.manager.queued_dids.insert(DID.to_owned(), Instant::now());
        h.resolved();
        h.manager.scan_did(&mut h.cursor, DID).unwrap();
        assert_eq!(h.queue_len(), 0);
        assert!(!h.manager.queued_dids.contains_key(DID));
    }

    #[test]
    fn host_checkpoint_never_rewinds_within_a_generation() {
        let mut h = harness();
        assert!(h.manager.advance_host(HOST, 0, Cursor::from(10), Utc::now()).is_some());
        assert!(h.manager.advance_host(HOST, 0, Cursor::from(9), Utc::now()).is_none());
        assert!(h.manager.advance_host(HOST, 0, Cursor::from(10), Utc::now()).is_none());
        assert!(h.manager.advance_host(HOST, 1, Cursor::from(1), Utc::now()).is_some());
        assert!(h.manager.advance_host(HOST, 0, Cursor::from(99), Utc::now()).is_none());
    }

    #[test]
    fn drop_persists_read_model() {
        let _g = shutdown_guard();
        let h = harness();
        let relay_db = h.relay_db.clone();
        let mut h = h;
        h.live(HOST, identity_frame(DID, 1, None));
        drop(h.manager);
        assert!(!SHUTDOWN.load(Ordering::Relaxed), "dropping a manager must not stop the relay");
        let conn = Connection::open(&relay_db).unwrap();
        let rows: i64 = conn.query_row("SELECT COUNT(*) FROM hosts", [], |r| r.get(0)).unwrap();
        assert_eq!(rows, 1);
    }
}
