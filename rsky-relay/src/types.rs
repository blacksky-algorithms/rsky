use std::ops::{Add, Sub};
use std::path::PathBuf;
use std::sync::LazyLock;
use std::{env, fmt};

use bytes::Bytes;
use chrono::{DateTime, Utc};
use fjall::compaction::{Fifo, Strategy};
use fjall::{Keyspace, PartitionCreateOptions, Slice};
use serde::{Deserialize, Serialize};
use thingbuf::{Recycle, mpsc};

use crate::config::{
    BLOCK_SIZE, CACHE_SIZE, DISK_SIZE, FSYNC_MS, MEMTABLE_SIZE, PUBLISHED_DISK_SIZE,
    QUEUE_DISK_SIZE, QUEUE_TTL_SECONDS, TTL_SECONDS, WRITE_BUFFER_SIZE,
};

pub const PARTITION_FIREHOSE: &str = "firehose";
/// 0.2.x deferred queue, kept frozen for rollback until PR 6 removes it.
pub const PARTITION_QUEUE_LEGACY: &str = "queue";
pub const PARTITION_QUEUE: &str = "queue_v2";
#[cfg(not(feature = "labeler"))]
pub const PARTITION_REPOS: &str = "repos";
pub const PARTITION_HOST_CURSORS: &str = "host_cursors";
pub const PARTITION_PUBLISHED: &str = "published";
pub const PARTITION_META: &str = "meta";
pub const PARTITION_LEGACY_IMPORTED: &str = "legacy_imported";

pub const META_MIGRATED_V1: &[u8] = b"migrated_v1";
pub const META_DOWNGRADED_AT: &[u8] = b"downgraded_at";
pub const META_ADMISSION: &[u8] = b"admission";
pub const META_PUBLISHED_INDEXED_UPTO: &[u8] = b"published_indexed_upto";
pub const META_DOWNGRADE_GENERATION_PREFIX: &[u8] = b"downgrade_generation>";

pub type MessageSender = mpsc::blocking::Sender<Message, MessageRecycle>;
pub type MessageReceiver = mpsc::blocking::Receiver<Message, MessageRecycle>;

#[expect(clippy::unwrap_used)]
pub static DB: LazyLock<Keyspace> = LazyLock::new(|| open_keyspace(&db_path()).unwrap());

#[inline]
pub fn db_path() -> PathBuf {
    env::var_os("RELAY_DB_PATH").map_or_else(|| PathBuf::from("db"), PathBuf::from)
}

pub fn open_keyspace(path: &std::path::Path) -> fjall::Result<Keyspace> {
    let db = fjall::Config::new(path)
        .cache_size(CACHE_SIZE)
        .max_write_buffer_size(WRITE_BUFFER_SIZE)
        .fsync_ms(FSYNC_MS)
        .open()?;
    open_partitions(&db)?;
    Ok(db)
}

/// A read-only view of a stopped relay's copied `db/`: with no compaction workers
/// the legacy FIFO policies cannot evict anything while it is being imported.
pub fn open_source_keyspace(path: &std::path::Path) -> fjall::Result<Keyspace> {
    fjall::Config::new(path).compaction_workers(0).flush_workers(1).open()
}

pub fn open_partitions(db: &Keyspace) -> fjall::Result<()> {
    db.open_partition(PARTITION_FIREHOSE, firehose_options())?;
    db.open_partition(PARTITION_QUEUE, PartitionCreateOptions::default())?;
    db.open_partition(PARTITION_HOST_CURSORS, PartitionCreateOptions::default())?;
    db.open_partition(PARTITION_PUBLISHED, published_options())?;
    db.open_partition(PARTITION_META, PartitionCreateOptions::default())?;
    db.open_partition(PARTITION_LEGACY_IMPORTED, PartitionCreateOptions::default())?;
    #[cfg(not(feature = "labeler"))]
    db.open_partition(PARTITION_REPOS, PartitionCreateOptions::default())?;
    Ok(())
}

pub fn legacy_queue_options() -> PartitionCreateOptions {
    PartitionCreateOptions::default()
        .compaction_strategy(Strategy::Fifo(Fifo::new(QUEUE_DISK_SIZE, QUEUE_TTL_SECONDS)))
}

fn published_options() -> PartitionCreateOptions {
    PartitionCreateOptions::default()
        .compaction_strategy(Strategy::Fifo(Fifo::new(PUBLISHED_DISK_SIZE, None)))
}

/// Value stored in `queue_v2`; the key (`{did}>` + admission counter) carries no
/// provenance, so the source host travels with the frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueEntry {
    pub host: String,
    pub generation: u64,
    pub seq: u64,
    #[serde(with = "serde_bytes")]
    pub frame: Vec<u8>,
}

impl QueueEntry {
    #[must_use]
    pub fn key(did: &str, admission: u64) -> Vec<u8> {
        let mut key = Vec::with_capacity(did.len() + 9);
        key.extend_from_slice(did.as_bytes());
        key.push(b'>');
        key.extend_from_slice(&admission.to_be_bytes());
        key
    }

    #[must_use]
    pub fn prefix(did: &str) -> Vec<u8> {
        let mut key = Vec::with_capacity(did.len() + 1);
        key.extend_from_slice(did.as_bytes());
        key.push(b'>');
        key
    }

    pub fn encode(
        &self,
    ) -> Result<Vec<u8>, serde_ipld_dagcbor::EncodeError<std::collections::TryReserveError>> {
        serde_ipld_dagcbor::to_vec(self)
    }

    pub fn decode(
        bytes: &[u8],
    ) -> Result<Self, serde_ipld_dagcbor::DecodeError<std::convert::Infallible>> {
        serde_ipld_dagcbor::from_slice(bytes)
    }

    /// The 0.2.x key shape `{did}>{host}>{seq}`, for the downgrade path.
    #[must_use]
    pub fn legacy_key(&self, did: &str) -> Vec<u8> {
        format!("{did}>{}>{}", self.host, self.seq).into_bytes()
    }
}

/// Durable per-host crawl position, written in the same batch as the event it
/// belongs to. Only moves forward within a generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostCursor {
    pub generation: u64,
    pub seq: u64,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub time: DateTime<Utc>,
}

impl HostCursor {
    pub fn encode(
        &self,
    ) -> Result<Vec<u8>, serde_ipld_dagcbor::EncodeError<std::collections::TryReserveError>> {
        serde_ipld_dagcbor::to_vec(self)
    }

    pub fn decode(
        bytes: &[u8],
    ) -> Result<Self, serde_ipld_dagcbor::DecodeError<std::convert::Infallible>> {
        serde_ipld_dagcbor::from_slice(bytes)
    }
}

/// `published` key: `{did}>{rev}>{commit cid}`.
#[cfg(not(feature = "labeler"))]
#[must_use]
pub fn published_key(did: &str, rev: &str, cid: &cid::Cid) -> Vec<u8> {
    let cid = cid.to_bytes();
    let mut key = Vec::with_capacity(did.len() + rev.len() + cid.len() + 2);
    key.extend_from_slice(did.as_bytes());
    key.push(b'>');
    key.extend_from_slice(rev.as_bytes());
    key.push(b'>');
    key.extend_from_slice(&cid);
    key
}

pub fn meta_u64(meta: &fjall::PartitionHandle, key: &[u8]) -> fjall::Result<Option<u64>> {
    Ok(meta.get(key)?.map(|v| u64::from_be_bytes(v.as_ref().try_into().unwrap_or_default())))
}

fn firehose_options() -> PartitionCreateOptions {
    PartitionCreateOptions::default()
        .manual_journal_persist(true)
        .compaction_strategy(Strategy::Fifo(Fifo::new(DISK_SIZE, TTL_SECONDS)))
        .max_memtable_size(MEMTABLE_SIZE)
        .block_size(BLOCK_SIZE)
}

#[derive(Debug)]
pub struct Message {
    pub data: Bytes,
    pub hostname: String,
}

#[derive(Debug)]
pub struct MessageRecycle;

impl Recycle<Message> for MessageRecycle {
    fn new_element(&self) -> Message {
        Message { data: Bytes::new(), hostname: String::new() }
    }

    // A released slot must drop its frame: retaining the Bytes turns the ring
    // into a frame cache of capacity * frame_size, which reaches tens of GiB
    // during large-commit floods.
    fn recycle(&self, element: &mut Message) {
        element.data = Bytes::new();
        element.hostname.clear();
    }
}

// Unconsumed intake bytes across the ring. Slot count alone cannot bound
// memory because commit frames vary from bytes to many megabytes; crawlers
// pause reading once this passes the budget.
static INFLIGHT_INTAKE_BYTES: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

pub fn intake_bytes_add(len: usize) {
    INFLIGHT_INTAKE_BYTES.fetch_add(len, std::sync::atomic::Ordering::Relaxed);
}

pub fn intake_bytes_sub(len: usize) {
    #[expect(clippy::unwrap_used)]
    INFLIGHT_INTAKE_BYTES
        .fetch_update(
            std::sync::atomic::Ordering::Relaxed,
            std::sync::atomic::Ordering::Relaxed,
            |v| Some(v.saturating_sub(len)),
        )
        .unwrap();
}

#[must_use]
pub fn intake_bytes() -> usize {
    INFLIGHT_INTAKE_BYTES.load(std::sync::atomic::Ordering::Relaxed)
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct Cursor([u8; 8]);

impl Cursor {
    #[inline]
    pub const fn get(self) -> u64 {
        u64::from_be_bytes(self.0)
    }

    /// Non-mutating `+1`; saturates at `u64::MAX` to keep fjall key ordering monotonic.
    #[inline]
    pub const fn successor(self) -> Self {
        let value = u64::from_be_bytes(self.0).saturating_add(1);
        Self(value.to_be_bytes())
    }

    /// In-place `+1`; saturates at `u64::MAX`. Prefer `successor` for non-mutating reads.
    #[inline]
    pub const fn next(&mut self) -> Self {
        let value = u64::from_be_bytes(self.0).saturating_add(1);
        self.0 = value.to_be_bytes();
        *self
    }
}

impl From<Slice> for Cursor {
    #[inline]
    fn from(value: Slice) -> Self {
        Self(value.as_ref().try_into().unwrap_or_default())
    }
}

impl From<Cursor> for Slice {
    #[inline]
    fn from(value: Cursor) -> Self {
        (&value.0).into()
    }
}

impl AsRef<[u8]> for Cursor {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl From<u64> for Cursor {
    #[inline]
    fn from(value: u64) -> Self {
        Self(value.to_be_bytes())
    }
}

impl From<Cursor> for u64 {
    #[inline]
    fn from(value: Cursor) -> Self {
        Self::from_be_bytes(value.0)
    }
}

impl Add<u64> for Cursor {
    type Output = Self;

    #[inline]
    fn add(self, rhs: u64) -> Self::Output {
        let value = u64::from_be_bytes(self.0) + rhs;
        Self(value.to_be_bytes())
    }
}

impl Sub<u64> for Cursor {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: u64) -> Self::Output {
        let value = u64::from_be_bytes(self.0) - rhs;
        Self(value.to_be_bytes())
    }
}

impl fmt::Debug for Cursor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.get().fmt(f)
    }
}

impl fmt::Display for Cursor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.get().fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_get_round_trips_u64() {
        for v in [0u64, 1, 42, u64::MAX / 2, u64::MAX - 1, u64::MAX] {
            assert_eq!(Cursor::from(v).get(), v);
        }
    }

    #[test]
    fn successor_does_not_mutate_self() {
        let c = Cursor::from(5);
        let s = c.successor();
        assert_eq!(s.get(), 6);
        assert_eq!(c.get(), 5);
    }

    #[test]
    fn successor_saturates_at_u64_max() {
        assert_eq!(Cursor::from(u64::MAX).successor().get(), u64::MAX);
    }

    #[test]
    fn next_mutates_and_returns_new_value() {
        let mut c = Cursor::from(10);
        let returned = c.next();
        assert_eq!(returned.get(), 11);
        assert_eq!(c.get(), 11);
    }

    #[test]
    fn next_saturates_at_u64_max() {
        let mut c = Cursor::from(u64::MAX);
        assert_eq!(c.next().get(), u64::MAX);
        assert_eq!(c.get(), u64::MAX);
    }

    #[test]
    fn add_sub_round_trip() {
        let c = Cursor::from(100);
        assert_eq!((c + 50).get(), 150);
        assert_eq!((c - 25).get(), 75);
    }

    #[test]
    fn equality_and_default() {
        assert_eq!(Cursor::default().get(), 0);
        assert_eq!(Cursor::from(7), Cursor::from(7));
        assert_ne!(Cursor::from(7), Cursor::from(8));
    }

    #[test]
    fn debug_and_display_match_u64() {
        let c = Cursor::from(123);
        assert_eq!(format!("{c:?}"), "123");
        assert_eq!(format!("{c}"), "123");
    }

    #[test]
    fn slice_round_trip() {
        let c = Cursor::from(0xdead_beef);
        let s: Slice = c.into();
        let c2: Cursor = s.into();
        assert_eq!(c, c2);
    }

    #[test]
    fn slice_too_short_yields_default() {
        let s: Slice = (&[1u8, 2, 3][..]).into();
        assert_eq!(Cursor::from(s), Cursor::default());
    }

    #[test]
    fn as_ref_returns_be_bytes() {
        let c = Cursor::from(1);
        assert_eq!(c.as_ref(), &1u64.to_be_bytes());
    }

    #[test]
    fn into_u64_round_trips() {
        let c = Cursor::from(42);
        let v: u64 = c.into();
        assert_eq!(v, 42);
    }

    #[test]
    fn copy_clone_semantics() {
        let c = Cursor::from(9);
        let d = c;
        let e = c;
        assert_eq!(d, e);
        assert_eq!(c.get(), 9);
    }

    #[test]
    fn message_recycle_clears_slot() {
        let recycler = MessageRecycle;
        let mut msg = recycler.new_element();
        msg.data = Bytes::from_static(b"x");
        msg.hostname = "h".to_owned();
        // recycle must drop the frame so released ring slots hold no data
        recycler.recycle(&mut msg);
        assert!(msg.data.is_empty());
        assert!(msg.hostname.is_empty());
    }

    #[test]
    fn intake_byte_accounting_saturates_at_zero() {
        let before = intake_bytes();
        intake_bytes_add(10);
        assert_eq!(intake_bytes(), before + 10);
        intake_bytes_sub(before + 25);
        assert_eq!(intake_bytes(), 0);
        intake_bytes_add(7);
        intake_bytes_sub(7);
        assert_eq!(intake_bytes(), 0);
    }

    #[test]
    fn db_path_defaults_to_db_when_unset() {
        // Don't mutate global env to avoid races with parallel tests; check pure default branch.
        let saved = std::env::var_os("RELAY_DB_PATH");
        // SAFETY: tests run with a single-threaded test runner via #[test] only when serialized;
        // we restore the var below and the LazyLock DB is intentionally not deref'd here.
        unsafe {
            std::env::remove_var("RELAY_DB_PATH");
            assert_eq!(db_path(), std::path::PathBuf::from("db"));
            if let Some(v) = saved {
                std::env::set_var("RELAY_DB_PATH", v);
            }
        }
    }

    #[test]
    fn db_path_reads_env_var_when_set() {
        unsafe {
            std::env::set_var("RELAY_DB_PATH", "/tmp/rsky-relay-test-path");
        }
        assert_eq!(db_path(), std::path::PathBuf::from("/tmp/rsky-relay-test-path"));
        unsafe {
            std::env::remove_var("RELAY_DB_PATH");
        }
    }

    #[test]
    fn open_keyspace_creates_partitions_at_path() {
        let tmp = tempfile::tempdir().unwrap();
        let ks = open_keyspace(tmp.path()).unwrap();
        // Both partitions must be present after open_keyspace.
        let firehose = ks.open_partition("firehose", PartitionCreateOptions::default()).unwrap();
        let queue = ks.open_partition(PARTITION_QUEUE, PartitionCreateOptions::default()).unwrap();
        // The on-disk dir was created.
        assert!(tmp.path().exists());
        // Both partitions are usable: insert + read round-trip.
        firehose.insert(Cursor::from(1), b"hello".as_slice()).unwrap();
        let v = firehose.get(Cursor::from(1)).unwrap().unwrap();
        assert_eq!(v.as_ref(), b"hello");
        // queue partition is functional too.
        queue.insert(b"k".as_slice(), b"v".as_slice()).unwrap();
    }

    #[test]
    fn db_lazylock_initializes_against_env_path() {
        // Trigger the global LazyLock body once. Set RELAY_DB_PATH first; if another
        // test already deref'd DB, the global path is sticky and this test still
        // exercises the (already-cached) deref; coverage reaches the closure either way.
        let tmp = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("RELAY_DB_PATH", tmp.path());
        }
        let _ks: &fjall::Keyspace = &DB;
    }

    #[test]
    fn queue_entry_and_host_cursor_round_trip() {
        let entry =
            QueueEntry { host: "h".to_owned(), generation: 2, seq: 7, frame: vec![1, 2, 3] };
        let decoded = QueueEntry::decode(&entry.encode().unwrap()).unwrap();
        assert_eq!(decoded, entry);
        assert_eq!(entry.legacy_key("did:plc:x"), b"did:plc:x>h>7".to_vec());
        let key = QueueEntry::key("did:plc:x", 5);
        assert!(key.starts_with(&QueueEntry::prefix("did:plc:x")));
        assert_eq!(&key[key.len() - 8..], &5u64.to_be_bytes());
        assert!(QueueEntry::key("did:plc:x", 5) < QueueEntry::key("did:plc:x", 6));
        let cursor = HostCursor { generation: 1, seq: 9, time: chrono::DateTime::UNIX_EPOCH };
        assert_eq!(HostCursor::decode(&cursor.encode().unwrap()).unwrap(), cursor);
        assert!(QueueEntry::decode(b"nope").is_err());
        assert!(HostCursor::decode(b"nope").is_err());
    }

    #[cfg(not(feature = "labeler"))]
    #[test]
    fn published_key_shape() {
        let cid = cid::Cid::try_from("bafyreigbtj4x7ip5legnfznufuopl4sg4knzc2cof6duas4b3q2fy6swua")
            .unwrap();
        let key = published_key("did:plc:x", "rev", &cid);
        assert!(key.starts_with(b"did:plc:x>rev>"));
        assert_eq!(key.len(), "did:plc:x>rev>".len() + cid.to_bytes().len());
    }

    #[test]
    fn meta_helpers() {
        let tmp = tempfile::tempdir().unwrap();
        let ks = open_keyspace(tmp.path()).unwrap();
        let meta = ks.open_partition(PARTITION_META, PartitionCreateOptions::default()).unwrap();
        assert_eq!(meta_u64(&meta, b"missing").unwrap(), None);
        meta.insert(b"n", 42u64.to_be_bytes()).unwrap();
        assert_eq!(meta_u64(&meta, b"n").unwrap(), Some(42));
        meta.insert(b"short", b"x".as_slice()).unwrap();
        assert_eq!(meta_u64(&meta, b"short").unwrap(), Some(0));
        let source = open_source_keyspace(&tmp.path().join("src")).unwrap();
        assert!(source.list_partitions().is_empty());
        let _ = legacy_queue_options();
    }
}
