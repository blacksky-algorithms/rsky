//! Live-path sync 1.1 gap detection.
//!
//! Every `#commit` frame from a sync-1.1 host carries `prevData`, the MST root
//! of the commit it follows. Holding the last applied `(rev, data)` per repo
//! therefore lets the live path prove, inductively and without touching the
//! repo, that it has not missed anything: the frame's `prevData` must equal the
//! `data` root we stored. When it does not, or when a `#sync` frame announces a
//! rewritten MST, the repo is handed to the backfill subsystem as a resync.
//!
//! This is the same proof hubble-sync applies (`AccountSyncState::apply`),
//! minus its host-level lax/strict tracking: we see one relay, not the hosts
//! behind it, so laxness is judged per frame.
//!
//! The decision function [`decide`] is pure. Everything stateful lives in
//! [`Tracker`], which runs as a single task fed by the firehose loop through a
//! bounded channel, so per-repo ordering is preserved and the hot loop never
//! waits on Postgres. Store reads happen only on cache miss and are batched per
//! drain; writes are batched into one `unnest` upsert per flush interval.
//!
//! Persistent state is the `repo_sync` table (see
//! `migrations/create_repo_sync.sql`), holding the last *observed* commit per
//! repo. "Observed" rather than "indexed": the record jobs are already durable
//! in the fjall queue by the time a frame is recorded here, which is the same
//! guarantee the firehose cursor gives.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use deadpool_postgres::Pool;
use lexicon_cid::Cid;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::SHUTDOWN;
use crate::backfiller::state::RepoStateStore;
use crate::metrics;
use crate::types::{FirehoseEvent, WintermuteError};

/// Entries per cache generation. Two generations are live at once, so the
/// cache holds at most twice this: roughly 60 MB at 300k entries.
pub const CACHE_GENERATION_ENTRIES: usize = 150_000;
/// How often pending rows are flushed to Postgres.
pub const FLUSH_INTERVAL: Duration = Duration::from_secs(1);
/// Flush early once this many repos have pending rows.
pub const FLUSH_ROWS: usize = 2_000;
/// Pending rows kept through a flush failure before they are discarded. The
/// in-memory cache stays authoritative; only the persisted copy lags.
pub const MAX_PENDING_ROWS: usize = 50_000;
/// Depth of the channel from the firehose loop. Full means the loop waits.
pub const CHANNEL_CAPACITY: usize = 16_384;
/// Messages drained per tracker pass; cache misses in a pass share one read.
pub const DRAIN_BATCH: usize = 512;

// ------------------------------------------------------------------ CAR head

/// The two commit-block fields the inductive proof needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitHead {
    pub rev: String,
    /// The MST root.
    pub data: Cid,
}

#[derive(serde::Deserialize)]
struct CommitBlock {
    rev: String,
    data: Cid,
}

fn read_uvarint(buf: &[u8]) -> Option<(u64, usize)> {
    let mut value = 0u64;
    let mut shift = 0u32;
    for (i, b) in buf.iter().enumerate() {
        if shift > 63 {
            return None;
        }
        value |= u64::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            return Some((value, i + 1));
        }
        shift += 7;
    }
    None
}

/// One length-prefixed CAR section: `(section, rest)`.
fn read_section(buf: &[u8]) -> Option<(&[u8], &[u8])> {
    let (len, n) = read_uvarint(buf)?;
    let len = usize::try_from(len).ok()?;
    let end = n.checked_add(len)?;
    let body = buf.get(n..end)?;
    Some((body, &buf[end..]))
}

/// Decode the commit block (the CAR root) out of a firehose `blocks` slice.
///
/// Synchronous and allocation-light: it walks sections until it meets the
/// root, which atproto hosts emit first, and decodes only that block. `None`
/// when the slice is empty, malformed, or does not contain its root.
#[must_use]
pub fn commit_head_from_car(car: &[u8]) -> Option<CommitHead> {
    let (header, mut rest) = read_section(car)?;
    let header = iroh_car::CarHeader::decode(header).ok()?;
    let root = *header.roots().first()?;
    while !rest.is_empty() {
        let (section, next) = read_section(rest)?;
        rest = next;
        let mut reader = std::io::Cursor::new(section);
        let Ok(cid) = Cid::read_bytes(&mut reader) else {
            continue;
        };
        if cid != root {
            continue;
        }
        let pos = usize::try_from(reader.position()).ok()?;
        let block: CommitBlock = serde_ipld_dagcbor::from_slice(section.get(pos..)?).ok()?;
        return Some(CommitHead {
            rev: block.rev,
            data: block.data,
        });
    }
    None
}

// ------------------------------------------------------------------ decision

/// The last commit we hold for a repo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncState {
    pub rev: String,
    /// MST root CID, as a string.
    pub data: String,
}

/// What a `#commit` frame meant against stored state. The `as_str` values are
/// the `outcome` label of `ingester_sync11_commits_total`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// No stored state: record and move on. History comes from backfill
    /// enumeration, not from asking for a resync of every unknown repo.
    FirstSeen,
    /// `prevData` matched our root: the proof holds.
    Applied,
    /// `rev` not newer than what we hold: a replay. State is not advanced.
    Stale,
    /// No `prevData`: a pre-sync-1.1 host. Recorded, but unproven.
    Lax,
    /// `prevData` did not match our root: we missed something.
    Desync,
    /// The frame carried no commit block, so there is no root to store. Any
    /// stored state is forgotten rather than left to trip the next frame.
    NoData,
}

impl Outcome {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FirstSeen => "first_seen",
            Self::Applied => "applied",
            Self::Stale => "stale",
            Self::Lax => "lax",
            Self::Desync => "desync",
            Self::NoData => "no_data",
        }
    }
}

/// Whether `rev` is strictly newer than `stored`. Revs are TIDs, which sort
/// lexicographically when well-formed (13 chars); anything else is only ever
/// judged equal or not.
#[must_use]
pub fn rev_newer(rev: &str, stored: &str) -> bool {
    if rev.len() == 13 && stored.len() == 13 {
        rev > stored
    } else {
        rev != stored
    }
}

/// The sync 1.1 inductive step, as a pure function of `(stored, frame)`.
///
/// 1. nothing stored: `FirstSeen` (or `NoData` if there is nothing to store);
/// 2. `rev` not newer than stored: `Stale`;
/// 3. `prevData` present and not equal to the stored root: `Desync`;
/// 4. no commit block in the frame: `NoData`;
/// 5. no `prevData`: `Lax`;
/// 6. otherwise `Applied`.
#[must_use]
pub fn decide(
    stored: Option<&SyncState>,
    rev: &str,
    prev_data: Option<&str>,
    data: Option<&str>,
) -> Outcome {
    let Some(stored) = stored else {
        return if data.is_some() {
            Outcome::FirstSeen
        } else {
            Outcome::NoData
        };
    };
    if !rev_newer(rev, &stored.rev) {
        return Outcome::Stale;
    }
    if let Some(prev) = prev_data {
        if prev != stored.data {
            return Outcome::Desync;
        }
    }
    if data.is_none() {
        return Outcome::NoData;
    }
    if prev_data.is_none() {
        return Outcome::Lax;
    }
    Outcome::Applied
}

// ------------------------------------------------------------------ messages

/// What the firehose loop tells the tracker. Only the proof-relevant fields
/// cross the channel; the record blocks stay behind.
#[derive(Debug)]
pub enum Msg {
    Commit {
        did: String,
        rev: String,
        prev_data: Option<String>,
        data: Option<String>,
        host: Arc<str>,
        seq: i64,
    },
    /// A `#sync` frame: the repo's MST was reset upstream.
    Sync {
        did: String,
        rev: String,
        data: Option<String>,
        host: Arc<str>,
        seq: i64,
    },
    /// An `#account` frame took the repo out of service.
    WriteOff { did: String, status: String },
}

impl Msg {
    /// Build the message for a `commit` or `sync` event, if it carries one.
    #[must_use]
    pub fn from_event(event: &FirehoseEvent, host: &Arc<str>) -> Option<Self> {
        let commit = event.commit.as_ref()?;
        match event.kind.as_str() {
            "commit" => Some(Self::Commit {
                did: event.did.clone(),
                rev: commit.rev.clone(),
                prev_data: commit.prev_data.clone(),
                data: commit.data.clone(),
                host: Arc::clone(host),
                seq: event.seq,
            }),
            "sync" => Some(Self::Sync {
                did: event.did.clone(),
                rev: commit.rev.clone(),
                data: commit.data.clone(),
                host: Arc::clone(host),
                seq: event.seq,
            }),
            _ => None,
        }
    }
}

/// What the ingester needs to run the tracker: the shared backfill state
/// store and whether anything will drain resync requests filed into it.
#[derive(Clone)]
pub struct Sync11Config {
    pub state: RepoStateStore,
    pub backfill_enabled: bool,
}

/// Sending side of the tracker channel. Cheap to clone; one per connection.
#[derive(Clone)]
pub struct Sync11Handle {
    tx: mpsc::Sender<Msg>,
}

impl Sync11Handle {
    /// Waits when the channel is full: the tracker is far cheaper than the
    /// CAR parse that precedes it, so this is backpressure, not a stall.
    pub async fn send(&self, msg: Msg) {
        if self.tx.send(msg).await.is_err() {
            metrics::INGESTER_ERRORS_TOTAL
                .with_label_values(&["sync11_closed"])
                .inc();
        }
    }
}

// ------------------------------------------------------------------- storage

type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A row bound for `repo_sync`.
pub type Upsert = (String, SyncState, Arc<str>);

/// Where `repo_sync` rows live. Postgres in the binary; in-memory in tests.
pub trait SyncStore: Send + Sync {
    fn load<'a>(
        &'a self,
        dids: &'a [String],
    ) -> BoxFut<'a, Result<Vec<(String, SyncState)>, WintermuteError>>;

    fn flush<'a>(
        &'a self,
        upserts: &'a [Upsert],
        deletes: &'a [String],
    ) -> BoxFut<'a, Result<(), WintermuteError>>;
}

/// `repo_sync` in Postgres.
pub struct PgSyncStore {
    pool: Pool,
}

impl PgSyncStore {
    #[must_use]
    pub const fn new(pool: Pool) -> Self {
        Self { pool }
    }
}

impl SyncStore for PgSyncStore {
    fn load<'a>(
        &'a self,
        dids: &'a [String],
    ) -> BoxFut<'a, Result<Vec<(String, SyncState)>, WintermuteError>> {
        Box::pin(async move {
            let client = self.pool.get().await?;
            let rows = client
                .query(
                    "SELECT did, rev, data_cid FROM repo_sync WHERE did = ANY($1)",
                    &[&dids],
                )
                .await?;
            Ok(rows
                .into_iter()
                .map(|r| {
                    (
                        r.get::<_, String>(0),
                        SyncState {
                            rev: r.get(1),
                            data: r.get(2),
                        },
                    )
                })
                .collect())
        })
    }

    fn flush<'a>(
        &'a self,
        upserts: &'a [Upsert],
        deletes: &'a [String],
    ) -> BoxFut<'a, Result<(), WintermuteError>> {
        Box::pin(async move {
            let client = self.pool.get().await?;
            if !upserts.is_empty() {
                let dids: Vec<&str> = upserts.iter().map(|(d, _, _)| d.as_str()).collect();
                let revs: Vec<&str> = upserts.iter().map(|(_, s, _)| s.rev.as_str()).collect();
                let roots: Vec<&str> = upserts.iter().map(|(_, s, _)| s.data.as_str()).collect();
                let hosts: Vec<&str> = upserts.iter().map(|(_, _, h)| h.as_ref()).collect();
                client
                    .execute(
                        "INSERT INTO repo_sync (did, rev, data_cid, host)
                         SELECT * FROM unnest($1::text[], $2::text[], $3::text[], $4::text[])
                         ON CONFLICT (did) DO UPDATE SET
                             rev        = EXCLUDED.rev,
                             data_cid   = EXCLUDED.data_cid,
                             host       = EXCLUDED.host,
                             updated_at = now()",
                        &[&dids, &revs, &roots, &hosts],
                    )
                    .await?;
            }
            if !deletes.is_empty() {
                client
                    .execute("DELETE FROM repo_sync WHERE did = ANY($1)", &[&deletes])
                    .await?;
            }
            Ok(())
        })
    }
}

// --------------------------------------------------------------------- cache

/// Two-generation bounded map: an approximate LRU with no per-access
/// bookkeeping. Inserts go to the current generation; when it fills, it
/// becomes the previous one and the one before that is dropped. A hit in the
/// previous generation is promoted.
struct Cache {
    cur: HashMap<String, SyncState>,
    prev: HashMap<String, SyncState>,
    cap: usize,
}

impl Cache {
    fn new(cap: usize) -> Self {
        Self {
            cur: HashMap::new(),
            prev: HashMap::new(),
            cap: cap.max(1),
        }
    }

    fn get(&mut self, did: &str) -> Option<&SyncState> {
        if self.cur.contains_key(did) {
            return self.cur.get(did);
        }
        let (key, state) = self.prev.remove_entry(did)?;
        self.insert(key, state);
        self.cur.get(did)
    }

    fn contains(&self, did: &str) -> bool {
        self.cur.contains_key(did) || self.prev.contains_key(did)
    }

    fn insert(&mut self, did: String, state: SyncState) {
        self.prev.remove(&did);
        self.cur.insert(did, state);
        if self.cur.len() >= self.cap {
            self.prev = std::mem::take(&mut self.cur);
        }
    }

    fn remove(&mut self, did: &str) {
        self.cur.remove(did);
        self.prev.remove(did);
    }

    fn len(&self) -> usize {
        self.cur.len() + self.prev.len()
    }
}

// ------------------------------------------------------------------- tracker

enum Pending {
    Upsert(SyncState, Arc<str>),
    Delete,
}

/// Per-repo live sync state: cache, pending writes, and the resync hand-off.
pub struct Tracker {
    cache: Cache,
    pending: HashMap<String, Pending>,
    state: RepoStateStore,
    store: Arc<dyn SyncStore>,
    backfill_enabled: bool,
    warned_backfill_off: bool,
}

impl Tracker {
    #[must_use]
    pub fn new(store: Arc<dyn SyncStore>, state: RepoStateStore, backfill_enabled: bool) -> Self {
        Self {
            cache: Cache::new(CACHE_GENERATION_ENTRIES),
            pending: HashMap::new(),
            state,
            store,
            backfill_enabled,
            warned_backfill_off: false,
        }
    }

    /// Entries held in memory. For tests and the occasional log line.
    #[must_use]
    pub fn cached(&self) -> usize {
        self.cache.len()
    }

    /// Rows awaiting a flush.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    fn known(&self, did: &str) -> bool {
        self.pending.contains_key(did) || self.cache.contains(did)
    }

    fn lookup(&mut self, did: &str) -> Option<SyncState> {
        match self.pending.get(did) {
            Some(Pending::Upsert(state, _)) => return Some(state.clone()),
            Some(Pending::Delete) => return None,
            None => {}
        }
        self.cache.get(did).cloned()
    }

    fn record(&mut self, did: &str, state: SyncState, host: &Arc<str>) {
        self.cache.insert(did.to_owned(), state.clone());
        self.pending
            .insert(did.to_owned(), Pending::Upsert(state, Arc::clone(host)));
    }

    fn forget(&mut self, did: &str) {
        self.cache.remove(did);
        self.pending.insert(did.to_owned(), Pending::Delete);
    }

    /// Warm the cache for every repo in the batch we know nothing about, in
    /// one read. A failed read is logged and the repos are treated as unseen:
    /// the frame is recorded rather than dropped.
    async fn prefetch(&mut self, msgs: &[Msg]) {
        let mut want: Vec<String> = Vec::new();
        let mut seen: HashSet<&str> = HashSet::new();
        for m in msgs {
            let did = match m {
                Msg::Commit { did, .. } | Msg::Sync { did, .. } => did.as_str(),
                Msg::WriteOff { .. } => continue,
            };
            if !self.known(did) && seen.insert(did) {
                want.push(did.to_owned());
            }
        }
        if want.is_empty() {
            return;
        }
        match self.store.load(&want).await {
            Ok(rows) => {
                for (did, state) in rows {
                    self.cache.insert(did, state);
                }
            }
            Err(e) => {
                tracing::warn!(repos = want.len(), "sync11: repo_sync read failed: {e}");
                metrics::INGESTER_ERRORS_TOTAL
                    .with_label_values(&["sync11_load"])
                    .inc();
            }
        }
    }

    /// Process one drained batch in order, then flush if enough is pending.
    pub async fn handle_batch(&mut self, msgs: Vec<Msg>) {
        self.prefetch(&msgs).await;
        for msg in msgs {
            self.handle(msg).await;
        }
        if self.pending.len() >= FLUSH_ROWS {
            self.flush().await;
        }
    }

    /// Process one message. Public so tests can drive the tracker directly;
    /// the actor uses [`Self::handle_batch`].
    pub async fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Commit {
                did,
                rev,
                prev_data,
                data,
                host,
                seq,
            } => {
                let stored = self.lookup(&did);
                let outcome = decide(stored.as_ref(), &rev, prev_data.as_deref(), data.as_deref());
                metrics::INGESTER_SYNC11_COMMITS_TOTAL
                    .with_label_values(&[outcome.as_str()])
                    .inc();
                match outcome {
                    Outcome::Stale => {
                        tracing::debug!(did, rev, seq, "sync11: stale commit, not advancing");
                        return;
                    }
                    Outcome::Desync => {
                        tracing::warn!(
                            did,
                            rev,
                            seq,
                            stored_rev = stored.as_ref().map(|s| s.rev.as_str()),
                            stored_data = stored.as_ref().map(|s| s.data.as_str()),
                            prev_data,
                            "sync11: prevData does not follow from stored root; requesting resync"
                        );
                        self.request_resync(&did, &rev, "prev_mismatch").await;
                    }
                    Outcome::Lax => {
                        tracing::trace!(did, rev, seq, "sync11: commit without prevData");
                    }
                    Outcome::NoData => {
                        tracing::debug!(did, rev, seq, "sync11: commit without a commit block");
                    }
                    Outcome::FirstSeen | Outcome::Applied => {}
                }
                match data {
                    Some(data) => self.record(&did, SyncState { rev, data }, &host),
                    None => {
                        if stored.is_some() {
                            self.forget(&did);
                        }
                    }
                }
            }
            Msg::Sync {
                did,
                rev,
                data,
                host,
                seq,
            } => {
                let stored = self.lookup(&did);
                let unchanged = matches!(
                    (&stored, &data),
                    (Some(s), Some(d)) if s.rev == rev && &s.data == d
                );
                if unchanged {
                    metrics::INGESTER_SYNC11_SYNC_EVENTS_TOTAL
                        .with_label_values(&["unchanged"])
                        .inc();
                    tracing::debug!(did, rev, seq, "sync11: #sync matches stored state");
                    return;
                }
                metrics::INGESTER_SYNC11_SYNC_EVENTS_TOTAL
                    .with_label_values(&["resync"])
                    .inc();
                tracing::info!(
                    did,
                    rev,
                    seq,
                    stored_rev = stored.as_ref().map(|s| s.rev.as_str()),
                    "sync11: #sync announced a new MST; requesting resync"
                );
                self.request_resync(&did, &rev, "sync_event").await;
                match data {
                    Some(data) => self.record(&did, SyncState { rev, data }, &host),
                    None => self.forget(&did),
                }
            }
            Msg::WriteOff { did, status } => {
                let state = self.state.clone();
                let d = did.clone();
                let s = status.clone();
                let written = tokio::task::spawn_blocking(move || state.write_off(&d, &s)).await;
                match written {
                    Ok(Ok(())) => {
                        tracing::debug!(did, status, "sync11: repo written off");
                    }
                    Ok(Err(e)) => {
                        tracing::error!(did, "sync11: write_off failed: {e}");
                        metrics::INGESTER_ERRORS_TOTAL
                            .with_label_values(&["sync11_state"])
                            .inc();
                    }
                    Err(e) => {
                        tracing::error!(did, "sync11: write_off task failed: {e}");
                        metrics::INGESTER_ERRORS_TOTAL
                            .with_label_values(&["sync11_state"])
                            .inc();
                    }
                }
                self.forget(&did);
            }
        }
    }

    async fn request_resync(&mut self, did: &str, rev: &str, reason: &'static str) {
        metrics::INGESTER_SYNC11_RESYNCS_REQUESTED_TOTAL
            .with_label_values(&[reason])
            .inc();
        if !self.backfill_enabled && !self.warned_backfill_off {
            self.warned_backfill_off = true;
            tracing::info!(
                did,
                reason,
                "sync11: resync requested while BACKFILL_MODE=off; it is recorded in the state \
                 store and will be fetched once backfill is enabled"
            );
        }
        let state = self.state.clone();
        let d = did.to_owned();
        let r = rev.to_owned();
        match tokio::task::spawn_blocking(move || state.request_resync(&d, &r, reason)).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                tracing::error!(did, reason, "sync11: request_resync failed: {e}");
                metrics::INGESTER_ERRORS_TOTAL
                    .with_label_values(&["sync11_state"])
                    .inc();
            }
            Err(e) => {
                tracing::error!(did, reason, "sync11: request_resync task failed: {e}");
                metrics::INGESTER_ERRORS_TOTAL
                    .with_label_values(&["sync11_state"])
                    .inc();
            }
        }
    }

    /// Write pending rows. On failure they are kept for the next attempt,
    /// up to [`MAX_PENDING_ROWS`]; past that the persisted copy is allowed to
    /// lag rather than the process to grow.
    pub async fn flush(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let drained = std::mem::take(&mut self.pending);
        let mut upserts: Vec<Upsert> = Vec::new();
        let mut deletes: Vec<String> = Vec::new();
        // Sorted, so the write order is deterministic whatever the map order.
        let mut entries: Vec<(String, Pending)> = drained.into_iter().collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (did, p) in entries {
            match p {
                Pending::Upsert(state, host) => upserts.push((did, state, host)),
                Pending::Delete => deletes.push(did),
            }
        }
        if let Err(e) = self.store.flush(&upserts, &deletes).await {
            tracing::warn!(
                rows = upserts.len() + deletes.len(),
                "sync11: repo_sync flush failed, retrying next tick: {e}"
            );
            metrics::INGESTER_ERRORS_TOTAL
                .with_label_values(&["sync11_flush"])
                .inc();
            if upserts.len() + deletes.len() > MAX_PENDING_ROWS {
                tracing::error!(
                    rows = upserts.len() + deletes.len(),
                    "sync11: dropping pending repo_sync rows past the retry cap"
                );
                return;
            }
            for (did, state, host) in upserts {
                self.pending
                    .entry(did)
                    .or_insert(Pending::Upsert(state, host));
            }
            for did in deletes {
                self.pending.entry(did).or_insert(Pending::Delete);
            }
        }
    }
}

// --------------------------------------------------------------------- actor

/// Start the tracker task. The returned join handle resolves after the final
/// flush, once every [`Sync11Handle`] has been dropped.
pub fn spawn(
    store: Arc<dyn SyncStore>,
    state: RepoStateStore,
    backfill_enabled: bool,
) -> (Sync11Handle, JoinHandle<()>) {
    let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
    let tracker = Tracker::new(store, state, backfill_enabled);
    let task = tokio::spawn(run(tracker, rx));
    (Sync11Handle { tx }, task)
}

async fn run(mut tracker: Tracker, mut rx: mpsc::Receiver<Msg>) {
    let mut tick = tokio::time::interval(FLUSH_INTERVAL);
    loop {
        tokio::select! {
            msg = rx.recv() => {
                let Some(first) = msg else { break };
                let mut batch = Vec::with_capacity(DRAIN_BATCH);
                batch.push(first);
                while batch.len() < DRAIN_BATCH {
                    match rx.try_recv() {
                        Ok(m) => batch.push(m),
                        Err(_) => break,
                    }
                }
                tracker.handle_batch(batch).await;
            }
            _ = tick.tick() => {
                tracker.flush().await;
                if SHUTDOWN.load(Ordering::Relaxed) && rx.is_empty() && rx.is_closed() {
                    break;
                }
            }
        }
    }
    tracker.flush().await;
    tracing::info!(cached = tracker.cached(), "sync11 tracker stopped");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backfiller::state::HUBBLE_SOURCE;
    use std::sync::Mutex;

    // ------------------------------------------------------------ fixtures

    fn cid(n: u8) -> Cid {
        rsky_common::ipld::cid_for_cbor(&serde_json::json!({ "n": n })).unwrap()
    }

    fn host() -> Arc<str> {
        Arc::from("relay.test")
    }

    fn state(rev: &str, data: &str) -> SyncState {
        SyncState {
            rev: rev.into(),
            data: data.into(),
        }
    }

    /// Minimal `CARv1` encoder: header + sections. Mirrors what the relay ships.
    fn car(root: Option<Cid>, blocks: &[(Cid, Vec<u8>)]) -> Vec<u8> {
        fn varint(mut n: u64, out: &mut Vec<u8>) {
            loop {
                let b = u8::try_from(n & 0x7f).unwrap();
                n >>= 7;
                if n == 0 {
                    out.push(b);
                    return;
                }
                out.push(b | 0x80);
            }
        }
        let header = iroh_car::CarHeader::new_v1(root.into_iter().collect())
            .encode()
            .unwrap();
        let mut out = Vec::new();
        varint(header.len() as u64, &mut out);
        out.extend_from_slice(&header);
        for (c, data) in blocks {
            let cb = c.to_bytes();
            varint((cb.len() + data.len()) as u64, &mut out);
            out.extend_from_slice(&cb);
            out.extend_from_slice(data);
        }
        out
    }

    /// A commit block plus one filler block, commit first, as hosts emit it.
    fn commit_car(rev: &str, data: Cid) -> (Vec<u8>, Cid) {
        let commit = rsky_repo::types::Commit {
            did: "did:plc:test".into(),
            rev: rev.into(),
            data,
            prev: None,
            version: 3,
            sig: vec![1, 2, 3],
        };
        let bytes = serde_ipld_dagcbor::to_vec(&commit).unwrap();
        let root = rsky_common::ipld::cid_for_cbor(&commit).unwrap();
        let filler = serde_ipld_dagcbor::to_vec(&serde_json::json!({"x": 1})).unwrap();
        (car(Some(root), &[(root, bytes), (cid(200), filler)]), root)
    }

    #[derive(Default)]
    struct MemStore {
        rows: Mutex<HashMap<String, (SyncState, String)>>,
        fail: Mutex<bool>,
        loads: Mutex<usize>,
    }

    impl MemStore {
        fn row(&self, did: &str) -> Option<SyncState> {
            self.rows.lock().unwrap().get(did).map(|(s, _)| s.clone())
        }
    }

    impl SyncStore for MemStore {
        fn load<'a>(
            &'a self,
            dids: &'a [String],
        ) -> BoxFut<'a, Result<Vec<(String, SyncState)>, WintermuteError>> {
            Box::pin(async move {
                *self.loads.lock().unwrap() += 1;
                if *self.fail.lock().unwrap() {
                    return Err(WintermuteError::Other("down".into()));
                }
                let rows = self.rows.lock().unwrap();
                Ok(dids
                    .iter()
                    .filter_map(|d| rows.get(d).map(|(s, _)| (d.clone(), s.clone())))
                    .collect())
            })
        }

        fn flush<'a>(
            &'a self,
            upserts: &'a [Upsert],
            deletes: &'a [String],
        ) -> BoxFut<'a, Result<(), WintermuteError>> {
            Box::pin(async move {
                if *self.fail.lock().unwrap() {
                    return Err(WintermuteError::Other("down".into()));
                }
                let mut rows = self.rows.lock().unwrap();
                for (did, s, h) in upserts {
                    rows.insert(did.clone(), (s.clone(), h.to_string()));
                }
                for did in deletes {
                    rows.remove(did);
                }
                drop(rows);
                Ok(())
            })
        }
    }

    fn tracker(store: &Arc<MemStore>, repo_state: &RepoStateStore) -> Tracker {
        let dyn_store: Arc<dyn SyncStore> = store.clone();
        Tracker::new(dyn_store, repo_state.clone(), true)
    }

    fn commit(did: &str, rev: &str, prev: Option<&Cid>, data: Option<&Cid>) -> Msg {
        Msg::Commit {
            did: did.into(),
            rev: rev.into(),
            prev_data: prev.map(ToString::to_string),
            data: data.map(ToString::to_string),
            host: host(),
            seq: 1,
        }
    }

    const R1: &str = "3lz7gd2xq5c2a";
    const R2: &str = "3lz7gd2xq5c2b";
    const R3: &str = "3lz7gd2xq5c2c";

    // ------------------------------------------------------------ decision

    #[test]
    fn rev_ordering_is_lexicographic_for_tids_and_equality_otherwise() {
        assert!(rev_newer(R2, R1));
        assert!(!rev_newer(R1, R2));
        assert!(!rev_newer(R1, R1));
        assert!(rev_newer("odd", R1));
        assert!(!rev_newer("odd", "odd"));
    }

    #[test]
    fn nothing_stored_is_first_seen_when_there_is_a_root_to_keep() {
        let a = cid(1).to_string();
        assert_eq!(decide(None, R1, None, Some(&a)), Outcome::FirstSeen);
        assert_eq!(
            decide(None, R1, Some(&cid(9).to_string()), Some(&a)),
            Outcome::FirstSeen
        );
        assert_eq!(decide(None, R1, None, None), Outcome::NoData);
    }

    #[test]
    fn a_matching_prev_data_is_applied() {
        let a = cid(1).to_string();
        let b = cid(2).to_string();
        let s = state(R1, &a);
        assert_eq!(decide(Some(&s), R2, Some(&a), Some(&b)), Outcome::Applied);
    }

    #[test]
    fn a_mismatching_prev_data_is_a_desync_even_without_a_commit_block() {
        let a = cid(1).to_string();
        let z = cid(9).to_string();
        let s = state(R1, &a);
        assert_eq!(
            decide(Some(&s), R2, Some(&z), Some(&cid(2).to_string())),
            Outcome::Desync
        );
        assert_eq!(decide(Some(&s), R2, Some(&z), None), Outcome::Desync);
    }

    #[test]
    fn a_rev_that_does_not_advance_is_stale_before_anything_else() {
        let a = cid(1).to_string();
        let z = cid(9).to_string();
        let s = state(R2, &a);
        assert_eq!(decide(Some(&s), R2, Some(&a), Some(&a)), Outcome::Stale);
        assert_eq!(decide(Some(&s), R1, Some(&z), None), Outcome::Stale);
    }

    #[test]
    fn a_frame_without_prev_data_is_lax_not_desync() {
        let a = cid(1).to_string();
        let s = state(R1, &a);
        assert_eq!(
            decide(Some(&s), R2, None, Some(&cid(2).to_string())),
            Outcome::Lax
        );
        assert_eq!(decide(Some(&s), R2, None, None), Outcome::NoData);
    }

    // ------------------------------------------------------------ CAR head

    #[test]
    fn commit_head_is_read_from_the_car_root() {
        let data = cid(7);
        let (bytes, _) = commit_car(R1, data);
        let head = commit_head_from_car(&bytes).unwrap();
        assert_eq!(head.rev, R1);
        assert_eq!(head.data, data);
    }

    #[test]
    fn commit_head_is_found_even_when_the_root_is_not_first() {
        let commit = rsky_repo::types::Commit {
            did: "did:plc:test".into(),
            rev: R2.into(),
            data: cid(3),
            prev: Some(cid(4)),
            version: 3,
            sig: vec![],
        };
        let bytes = serde_ipld_dagcbor::to_vec(&commit).unwrap();
        let root = rsky_common::ipld::cid_for_cbor(&commit).unwrap();
        let filler = serde_ipld_dagcbor::to_vec(&serde_json::json!({"x": 1})).unwrap();
        let car = car(Some(root), &[(cid(200), filler), (root, bytes)]);
        assert_eq!(commit_head_from_car(&car).unwrap().data, cid(3));
    }

    #[test]
    fn commit_head_is_none_for_empty_garbage_or_a_missing_root() {
        assert!(commit_head_from_car(&[]).is_none());
        assert!(commit_head_from_car(b"not a car at all").is_none());
        assert!(commit_head_from_car(&[0x80, 0x80, 0x80]).is_none());
        let filler = serde_ipld_dagcbor::to_vec(&serde_json::json!({"x": 1})).unwrap();
        let without_root = car(Some(cid(1)), &[(cid(200), filler.clone())]);
        assert!(commit_head_from_car(&without_root).is_none());
        let no_roots = car(None, &[(cid(200), filler)]);
        assert!(commit_head_from_car(&no_roots).is_none());
        let not_a_commit = car(Some(cid(200)), &[(cid(200), vec![0xa0])]);
        assert!(commit_head_from_car(&not_a_commit).is_none());
    }

    #[test]
    fn uvarint_rejects_overlong_and_truncated_input() {
        assert_eq!(read_uvarint(&[0x05]), Some((5, 1)));
        assert_eq!(read_uvarint(&[0x80, 0x01]), Some((128, 2)));
        assert_eq!(read_uvarint(&[0x80]), None);
        assert_eq!(read_uvarint(&[0x80; 11]), None);
        assert!(read_section(&[0x05, 1, 2]).is_none());
    }

    // ---------------------------------------------------------------- cache

    #[test]
    fn cache_rotates_generations_and_promotes_hits() {
        let mut c = Cache::new(2);
        c.insert("a".into(), state(R1, "x"));
        c.insert("b".into(), state(R1, "y")); // fills cur -> rotates
        assert_eq!(c.cur.len(), 0);
        assert_eq!(c.prev.len(), 2);
        assert!(c.get("a").is_some()); // promoted into cur
        assert_eq!(c.cur.len(), 1);
        c.insert("c".into(), state(R1, "z")); // cur full again -> rotate
        assert!(c.contains("a") && c.contains("c"));
        assert!(!c.contains("b"), "b was in the dropped generation");
        c.remove("a");
        assert!(!c.contains("a"));
        assert_eq!(c.len(), 1);
    }

    // -------------------------------------------------------------- tracker

    #[tokio::test]
    async fn first_seen_records_and_a_following_commit_applies() {
        let store = Arc::new(MemStore::default());
        let rs = RepoStateStore::open_in_memory().unwrap();
        let mut t = tracker(&store, &rs);

        t.handle(commit("did:a", R1, None, Some(&cid(1)))).await;
        t.handle(commit("did:a", R2, Some(&cid(1)), Some(&cid(2))))
            .await;
        assert_eq!(t.pending(), 1, "one row per repo per flush");
        t.flush().await;
        assert_eq!(store.row("did:a"), Some(state(R2, &cid(2).to_string())));
        assert_eq!(t.pending(), 0);
        assert!(rs.claim_for_source(HUBBLE_SOURCE, 10).unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_stale_replay_does_not_move_state() {
        let store = Arc::new(MemStore::default());
        let rs = RepoStateStore::open_in_memory().unwrap();
        let mut t = tracker(&store, &rs);
        t.handle(commit("did:a", R2, None, Some(&cid(2)))).await;
        t.handle(commit("did:a", R1, Some(&cid(9)), Some(&cid(1))))
            .await;
        t.flush().await;
        assert_eq!(store.row("did:a"), Some(state(R2, &cid(2).to_string())));
        assert!(rs.claim_for_source(HUBBLE_SOURCE, 10).unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_prev_mismatch_requests_a_resync_and_resets_state() {
        let store = Arc::new(MemStore::default());
        let rs = RepoStateStore::open_in_memory().unwrap();
        let mut t = tracker(&store, &rs);
        t.handle(commit("did:a", R1, None, Some(&cid(1)))).await;
        t.handle(commit("did:a", R3, Some(&cid(2)), Some(&cid(3))))
            .await;

        let claimed = rs.claim_for_source(HUBBLE_SOURCE, 10).unwrap();
        assert_eq!(claimed, vec![("did:a".to_owned(), R3.to_owned())]);
        t.flush().await;
        assert_eq!(
            store.row("did:a"),
            Some(state(R3, &cid(3).to_string())),
            "state chains on from the desynchronised commit"
        );
        // the next commit follows from the reset state without another resync
        t.handle(commit(
            "did:a",
            "3lz7gd2xq5c2d",
            Some(&cid(3)),
            Some(&cid(4)),
        ))
        .await;
        assert!(rs.claim_for_source(HUBBLE_SOURCE, 10).unwrap().is_empty());
    }

    #[tokio::test]
    async fn stored_state_is_loaded_from_the_store_on_a_cache_miss() {
        let store = Arc::new(MemStore::default());
        store.rows.lock().unwrap().insert(
            "did:a".into(),
            (state(R1, &cid(1).to_string()), "old".into()),
        );
        let rs = RepoStateStore::open_in_memory().unwrap();
        let mut t = tracker(&store, &rs);
        // prev matches the persisted root: applied, no resync
        t.handle_batch(vec![commit("did:a", R2, Some(&cid(1)), Some(&cid(2)))])
            .await;
        assert_eq!(*store.loads.lock().unwrap(), 1);
        assert!(rs.claim_for_source(HUBBLE_SOURCE, 10).unwrap().is_empty());
        // now cached: another batch does not read again
        t.handle_batch(vec![commit("did:a", R3, Some(&cid(2)), Some(&cid(3)))])
            .await;
        assert_eq!(*store.loads.lock().unwrap(), 1);

        // and a persisted root that does not match is a desync
        store.rows.lock().unwrap().insert(
            "did:b".into(),
            (state(R1, &cid(1).to_string()), "old".into()),
        );
        t.handle_batch(vec![commit("did:b", R2, Some(&cid(8)), Some(&cid(2)))])
            .await;
        assert_eq!(
            rs.claim_for_source(HUBBLE_SOURCE, 10).unwrap(),
            vec![("did:b".to_owned(), R2.to_owned())]
        );
    }

    #[tokio::test]
    async fn a_failed_load_treats_the_repo_as_unseen() {
        let store = Arc::new(MemStore::default());
        store.rows.lock().unwrap().insert(
            "did:a".into(),
            (state(R1, &cid(1).to_string()), "old".into()),
        );
        *store.fail.lock().unwrap() = true;
        let rs = RepoStateStore::open_in_memory().unwrap();
        let mut t = tracker(&store, &rs);
        t.handle_batch(vec![commit("did:a", R2, Some(&cid(8)), Some(&cid(2)))])
            .await;
        assert!(
            rs.claim_for_source(HUBBLE_SOURCE, 10).unwrap().is_empty(),
            "no proof possible, so no resync"
        );
        assert_eq!(t.pending(), 1, "recorded as first seen");
    }

    #[tokio::test]
    async fn lax_commits_record_without_a_resync() {
        let store = Arc::new(MemStore::default());
        let rs = RepoStateStore::open_in_memory().unwrap();
        let mut t = tracker(&store, &rs);
        t.handle(commit("did:a", R1, None, Some(&cid(1)))).await;
        t.handle(commit("did:a", R2, None, Some(&cid(5)))).await;
        t.flush().await;
        assert_eq!(store.row("did:a"), Some(state(R2, &cid(5).to_string())));
        assert!(rs.claim_for_source(HUBBLE_SOURCE, 10).unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_commit_without_a_block_forgets_state_and_desyncs_when_it_can() {
        let store = Arc::new(MemStore::default());
        let rs = RepoStateStore::open_in_memory().unwrap();
        let mut t = tracker(&store, &rs);
        t.handle(commit("did:a", R1, None, Some(&cid(1)))).await;
        t.flush().await;
        // newer, matching prev, but no block: forget rather than keep a root
        // the next frame would not follow from
        t.handle(commit("did:a", R2, Some(&cid(1)), None)).await;
        t.flush().await;
        assert_eq!(store.row("did:a"), None);
        assert!(rs.claim_for_source(HUBBLE_SOURCE, 10).unwrap().is_empty());
        // ...and the next commit is simply first seen again
        t.handle(commit("did:a", R3, Some(&cid(2)), Some(&cid(3))))
            .await;
        assert!(rs.claim_for_source(HUBBLE_SOURCE, 10).unwrap().is_empty());

        // a mismatching prev without a block still proves the gap
        t.handle(commit("did:b", R1, None, Some(&cid(1)))).await;
        t.handle(commit("did:b", R2, Some(&cid(9)), None)).await;
        assert_eq!(
            rs.claim_for_source(HUBBLE_SOURCE, 10).unwrap(),
            vec![("did:b".to_owned(), R2.to_owned())]
        );
    }

    #[tokio::test]
    async fn a_sync_frame_requests_a_resync_unless_it_matches_stored_state() {
        let store = Arc::new(MemStore::default());
        let rs = RepoStateStore::open_in_memory().unwrap();
        let mut t = tracker(&store, &rs);
        t.handle(commit("did:a", R1, None, Some(&cid(1)))).await;

        t.handle(Msg::Sync {
            did: "did:a".into(),
            rev: R1.into(),
            data: Some(cid(1).to_string()),
            host: host(),
            seq: 2,
        })
        .await;
        assert!(
            rs.claim_for_source(HUBBLE_SOURCE, 10).unwrap().is_empty(),
            "identical #sync says nothing new"
        );

        t.handle(Msg::Sync {
            did: "did:a".into(),
            rev: R2.into(),
            data: Some(cid(7).to_string()),
            host: host(),
            seq: 3,
        })
        .await;
        assert_eq!(
            rs.claim_for_source(HUBBLE_SOURCE, 10).unwrap(),
            vec![("did:a".to_owned(), R2.to_owned())]
        );
        t.flush().await;
        assert_eq!(store.row("did:a"), Some(state(R2, &cid(7).to_string())));

        // an unknown repo's #sync is a resync too, even without a block
        t.handle(Msg::Sync {
            did: "did:new".into(),
            rev: R1.into(),
            data: None,
            host: host(),
            seq: 4,
        })
        .await;
        assert_eq!(
            rs.claim_for_source(HUBBLE_SOURCE, 10).unwrap(),
            vec![("did:new".to_owned(), R1.to_owned())]
        );
        t.flush().await;
        assert_eq!(store.row("did:new"), None);
    }

    #[tokio::test]
    async fn a_write_off_marks_the_repo_terminal_and_forgets_it() {
        let store = Arc::new(MemStore::default());
        let rs = RepoStateStore::open_in_memory().unwrap();
        let mut t = tracker(&store, &rs);
        t.handle(commit("did:a", R1, None, Some(&cid(1)))).await;
        t.flush().await;
        assert!(store.row("did:a").is_some());

        t.handle(Msg::WriteOff {
            did: "did:a".into(),
            status: "deleted".into(),
        })
        .await;
        t.flush().await;
        assert_eq!(store.row("did:a"), None);
        assert_eq!(t.cached(), 0);
        let st = rs.stats().unwrap();
        assert_eq!((st.terminal, st.pending), (1, 0));
        assert_eq!(
            rs.source_of("did:a").unwrap().as_deref(),
            Some(HUBBLE_SOURCE)
        );
    }

    #[tokio::test]
    async fn a_failed_flush_keeps_rows_for_the_next_attempt() {
        let store = Arc::new(MemStore::default());
        let rs = RepoStateStore::open_in_memory().unwrap();
        let mut t = tracker(&store, &rs);
        t.handle(commit("did:a", R1, None, Some(&cid(1)))).await;
        *store.fail.lock().unwrap() = true;
        t.flush().await;
        assert_eq!(t.pending(), 1);
        assert_eq!(store.row("did:a"), None);
        // a newer state recorded meanwhile wins over the retried one
        t.handle(commit("did:a", R2, Some(&cid(1)), Some(&cid(2))))
            .await;
        *store.fail.lock().unwrap() = false;
        t.flush().await;
        assert_eq!(store.row("did:a"), Some(state(R2, &cid(2).to_string())));
        assert_eq!(t.pending(), 0);
    }

    #[tokio::test]
    async fn the_actor_flushes_on_shutdown_of_its_last_handle() {
        let store = Arc::new(MemStore::default());
        let rs = RepoStateStore::open_in_memory().unwrap();
        let dyn_store: Arc<dyn SyncStore> = store.clone();
        let (handle, task) = spawn(dyn_store, rs.clone(), false);
        handle.send(commit("did:a", R1, None, Some(&cid(1)))).await;
        handle
            .send(commit("did:a", R2, Some(&cid(9)), Some(&cid(2))))
            .await;
        drop(handle);
        task.await.unwrap();
        assert_eq!(store.row("did:a"), Some(state(R2, &cid(2).to_string())));
        // backfill was "off": the resync is still recorded for later
        assert_eq!(
            rs.claim_for_source(HUBBLE_SOURCE, 10).unwrap(),
            vec![("did:a".to_owned(), R2.to_owned())]
        );
    }

    #[test]
    fn messages_are_built_from_commit_and_sync_events_only() {
        use crate::types::CommitData;
        let ev = |kind: &str, commit: Option<CommitData>| FirehoseEvent {
            seq: 5,
            did: "did:a".into(),
            time: "2025-01-01T00:00:00Z".into(),
            kind: kind.into(),
            commit,
            identity: None,
            account: None,
        };
        let cd = CommitData {
            rev: R1.into(),
            ops: vec![],
            blocks: vec![],
            since: None,
            prev_data: Some(cid(1).to_string()),
            data: Some(cid(2).to_string()),
            too_big: false,
        };
        assert!(matches!(
            Msg::from_event(&ev("commit", Some(cd.clone())), &host()),
            Some(Msg::Commit { seq: 5, .. })
        ));
        assert!(matches!(
            Msg::from_event(&ev("sync", Some(cd.clone())), &host()),
            Some(Msg::Sync { .. })
        ));
        assert!(Msg::from_event(&ev("identity", Some(cd)), &host()).is_none());
        assert!(Msg::from_event(&ev("commit", None), &host()).is_none());
    }

    // -------------------------------------------------------- parse_message

    /// A `#commit` frame as the relay ships it, with sync 1.1 fields.
    fn commit_frame(prev_data: Option<Cid>, blocks: Vec<u8>, too_big: bool) -> Vec<u8> {
        use rsky_lexicon::com::atproto::sync::SubscribeReposCommit;
        #[derive(serde::Serialize)]
        struct Header {
            t: String,
            op: u8,
        }
        let body = SubscribeReposCommit {
            seq: 77,
            time: "2025-01-01T00:00:00Z".parse().unwrap(),
            rebase: false,
            too_big,
            repo: "did:plc:test".into(),
            commit: cid(50),
            prev: None,
            rev: R2.into(),
            since: Some(R1.into()),
            blocks,
            ops: vec![],
            blobs: vec![],
            prev_data,
        };
        let mut out = Vec::new();
        ciborium::ser::into_writer(
            &Header {
                t: "#commit".into(),
                op: 1,
            },
            &mut out,
        )
        .unwrap();
        serde_ipld_dagcbor::to_writer(&mut out, &body).unwrap();
        out
    }

    fn sync_frame(blocks: Vec<u8>) -> Vec<u8> {
        use rsky_lexicon::com::atproto::sync::SubscribeReposSync;
        #[derive(serde::Serialize)]
        struct Header {
            t: String,
            op: u8,
        }
        let body = SubscribeReposSync {
            seq: 78,
            did: "did:plc:test".into(),
            blocks,
            rev: R3.into(),
            time: "2025-01-01T00:00:00Z".parse().unwrap(),
        };
        let mut out = Vec::new();
        ciborium::ser::into_writer(
            &Header {
                t: "#sync".into(),
                op: 1,
            },
            &mut out,
        )
        .unwrap();
        serde_ipld_dagcbor::to_writer(&mut out, &body).unwrap();
        out
    }

    fn parsed(frame: &[u8]) -> FirehoseEvent {
        match crate::ingester::IngesterManager::parse_message(frame).unwrap() {
            crate::ingester::ParseResult::Event(e) => e,
            other => panic!("expected an event, got {other:?}"),
        }
    }

    #[test]
    fn parse_message_carries_the_sync11_fields_of_a_commit() {
        let (blocks, _) = commit_car(R2, cid(3));
        let ev = parsed(&commit_frame(Some(cid(1)), blocks, false));
        assert_eq!(ev.kind, "commit");
        let c = ev.commit.unwrap();
        assert_eq!(c.rev, R2);
        assert_eq!(c.since.as_deref(), Some(R1));
        assert_eq!(c.prev_data, Some(cid(1).to_string()));
        assert_eq!(c.data, Some(cid(3).to_string()));
        assert!(!c.too_big);

        let msg = Msg::from_event(
            &FirehoseEvent {
                commit: Some(c),
                ..ev
            },
            &host(),
        )
        .unwrap();
        assert!(matches!(msg, Msg::Commit { seq: 77, .. }));
    }

    #[test]
    fn parse_message_leaves_data_empty_when_the_frame_has_no_commit_block() {
        let ev = parsed(&commit_frame(None, vec![], true));
        let c = ev.commit.unwrap();
        assert_eq!(c.prev_data, None);
        assert_eq!(c.data, None);
        assert!(c.too_big);
    }

    #[test]
    fn parse_message_turns_a_sync_frame_into_an_op_less_commit() {
        let (blocks, _) = commit_car(R3, cid(9));
        let ev = parsed(&sync_frame(blocks));
        assert_eq!(ev.kind, "sync");
        assert_eq!(ev.did, "did:plc:test");
        let c = ev.commit.unwrap();
        assert!(c.ops.is_empty());
        assert_eq!(c.rev, R3);
        assert_eq!(c.data, Some(cid(9).to_string()));
        assert!(matches!(
            Msg::from_event(
                &FirehoseEvent {
                    commit: Some(c),
                    ..ev
                },
                &host()
            ),
            Some(Msg::Sync { seq: 78, .. })
        ));

        let ev = parsed(&sync_frame(vec![]));
        assert_eq!(ev.commit.unwrap().data, None);
    }
}
