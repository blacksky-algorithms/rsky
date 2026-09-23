use std::path::Path;

use chrono::{DateTime, Utc};
use fjall::{Batch, Keyspace, PartitionCreateOptions, PersistMode};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use thiserror::Error;

#[cfg(not(feature = "labeler"))]
use crate::types::published_key;
use crate::types::{
    HostCursor, META_ADMISSION, META_DOWNGRADE_GENERATION_PREFIX, META_DOWNGRADED_AT,
    META_MIGRATED_V1, META_PUBLISHED_INDEXED_UPTO, PARTITION_FIREHOSE, PARTITION_HOST_CURSORS,
    PARTITION_LEGACY_IMPORTED, PARTITION_META, PARTITION_QUEUE, PARTITION_QUEUE_LEGACY, QueueEntry,
    legacy_queue_options, meta_u64,
};
use crate::validator::event::ParseError;
#[cfg(not(feature = "labeler"))]
use crate::validator::event::SubscribeReposEvent;

const PUBLISHED_BATCH: usize = 1000;

#[derive(Debug, Error)]
pub enum MigrateError {
    #[error("fjall error: {0}")]
    Fjall(#[from] fjall::Error),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("parse error: {0}")]
    Parse(#[from] ParseError),
    #[error("encode error: {0}")]
    Encode(#[from] serde_ipld_dagcbor::EncodeError<std::collections::TryReserveError>),
    #[error("decode error: {0}")]
    Decode(#[from] serde_ipld_dagcbor::DecodeError<std::convert::Infallible>),
    #[error(
        "legacy state present but no --legacy-source given; copy db/ while stopped and pass it"
    )]
    SourceRequired,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReconcileStats {
    pub queue_imported: u64,
    pub queue_skipped: u64,
    pub published_indexed: u64,
    pub hosts_seeded: u64,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DowngradeStats {
    pub queue_restored: u64,
    pub hosts_written: u64,
}

fn meta(db: &Keyspace) -> fjall::Result<fjall::PartitionHandle> {
    db.open_partition(PARTITION_META, PartitionCreateOptions::default())
}

/// True on the first start of a >= 0.3 binary and after every `--migrate-down`.
pub fn needs_reconcile(db: &Keyspace) -> fjall::Result<bool> {
    let meta = meta(db)?;
    Ok(meta.get(META_MIGRATED_V1)?.is_none() || meta.get(META_DOWNGRADED_AT)?.is_some())
}

/// A relay that has run 0.2.x has a legacy queue partition or `SQLite` host cursors.
pub fn has_legacy_state(db: &Keyspace, relay_db: &Path) -> Result<bool, MigrateError> {
    if db.partition_exists(PARTITION_QUEUE_LEGACY) {
        let queue = db.open_partition(PARTITION_QUEUE_LEGACY, legacy_queue_options())?;
        if !queue.is_empty()? {
            return Ok(true);
        }
    }
    if !relay_db.exists() {
        return Ok(false);
    }
    let conn = Connection::open_with_flags(
        relay_db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let has_table: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = 'hosts'",
        [],
        |row| row.get(0),
    )?;
    if !has_table {
        return Ok(false);
    }
    let rows: i64 = conn.query_row("SELECT COUNT(*) FROM hosts", [], |row| row.get(0))?;
    Ok(rows > 0)
}

/// Fresh install: nothing to import, record the layout version.
pub fn mark_migrated(db: &Keyspace) -> fjall::Result<()> {
    let meta = meta(db)?;
    let mut batch = db.batch();
    batch.insert(&meta, META_MIGRATED_V1, 1u64.to_be_bytes());
    batch.remove(&meta, META_DOWNGRADED_AT);
    batch.commit()?;
    db.persist(PersistMode::SyncAll)
}

fn parse_legacy_key(key: &[u8]) -> Option<(String, String, u64)> {
    let key = std::str::from_utf8(key).ok()?;
    let mut parts = key.splitn(3, '>');
    let did = parts.next()?;
    let host = parts.next()?;
    let seq = parts.next()?.parse().ok()?;
    Some((did.to_owned(), host.to_owned(), seq))
}

fn import_identity(key: &[u8], frame: &[u8]) -> Vec<u8> {
    let digest = Sha256::digest(frame);
    let mut identity = Vec::with_capacity(key.len() + 1 + 64);
    identity.extend_from_slice(key);
    identity.push(b'>');
    identity.extend_from_slice(hex::encode(digest).as_bytes());
    identity
}

/// Imports the 0.2.x layout from a stopped, copied `db/` opened without
/// compaction workers, so nothing is evicted while it is read. Every step is
/// idempotent and checkpointed in `meta`.
pub fn reconcile(
    live: &Keyspace, source: &Keyspace, relay_db: &Path,
) -> Result<ReconcileStats, MigrateError> {
    let mut stats = ReconcileStats::default();
    let meta = meta(live)?;
    stats = import_legacy_queue(live, source, &meta, stats)?;
    stats = index_published(live, source, &meta, stats)?;
    stats = seed_host_cursors(live, &meta, relay_db, stats)?;
    let mut batch = live.batch();
    batch.insert(&meta, META_MIGRATED_V1, 1u64.to_be_bytes());
    batch.remove(&meta, META_DOWNGRADED_AT);
    batch.commit()?;
    live.persist(PersistMode::SyncAll)?;
    tracing::info!(?stats, "legacy reconcile complete");
    Ok(stats)
}

fn import_legacy_queue(
    live: &Keyspace, source: &Keyspace, meta: &fjall::PartitionHandle, mut stats: ReconcileStats,
) -> Result<ReconcileStats, MigrateError> {
    if !source.partition_exists(PARTITION_QUEUE_LEGACY) {
        return Ok(stats);
    }
    let legacy = source.open_partition(PARTITION_QUEUE_LEGACY, legacy_queue_options())?;
    let queue = live.open_partition(PARTITION_QUEUE, PartitionCreateOptions::default())?;
    let imported =
        live.open_partition(PARTITION_LEGACY_IMPORTED, PartitionCreateOptions::default())?;
    let mut admission = meta_u64(meta, META_ADMISSION)?.unwrap_or(0);
    for res in legacy.iter() {
        let (key, frame) = res?;
        let identity = import_identity(&key, &frame);
        if imported.contains_key(&identity)? {
            stats.queue_skipped += 1;
            continue;
        }
        let Some((did, host, seq)) = parse_legacy_key(&key) else {
            tracing::warn!("skipping legacy queue entry with malformed key");
            stats.queue_skipped += 1;
            continue;
        };
        admission += 1;
        let entry = QueueEntry { host, generation: 0, seq, frame: frame.to_vec() };
        let mut batch = live.batch();
        batch.insert(&queue, QueueEntry::key(&did, admission), entry.encode()?);
        batch.insert(&imported, identity, admission.to_be_bytes());
        batch.insert(meta, META_ADMISSION, admission.to_be_bytes());
        batch.commit()?;
        stats.queue_imported += 1;
    }
    Ok(stats)
}

/// The `published` key for a frame: `#commit` carries its head CID in the
/// envelope; `#sync` needs the CAR root. Other frames have no identity.
#[cfg(feature = "labeler")]
pub const fn frame_identity(_frame: &[u8]) -> Result<Option<Vec<u8>>, ParseError> {
    Ok(None)
}

#[cfg(not(feature = "labeler"))]
pub fn frame_identity(frame: &[u8]) -> Result<Option<Vec<u8>>, ParseError> {
    let Some(event) = SubscribeReposEvent::parse(frame)? else {
        return Ok(None);
    };
    match &event {
        SubscribeReposEvent::Commit(commit) => {
            Ok(Some(published_key(&commit.did, &commit.rev.to_string(), &commit.commit)))
        }
        SubscribeReposEvent::Sync(sync) => {
            let Some((_, head)) = event.commit()? else {
                return Ok(None);
            };
            Ok(Some(published_key(&sync.did, &sync.rev.to_string(), &head)))
        }
        _ => Ok(None),
    }
}

fn index_published(
    live: &Keyspace, source: &Keyspace, meta: &fjall::PartitionHandle, mut stats: ReconcileStats,
) -> Result<ReconcileStats, MigrateError> {
    if !source.partition_exists(PARTITION_FIREHOSE) {
        return Ok(stats);
    }
    let firehose = source.open_partition(PARTITION_FIREHOSE, PartitionCreateOptions::default())?;
    let published =
        live.open_partition(crate::types::PARTITION_PUBLISHED, PartitionCreateOptions::default())?;
    let start = meta_u64(meta, META_PUBLISHED_INDEXED_UPTO)?.map_or(0, |seq| seq + 1);
    let mut batch: Option<Batch> = None;
    let mut pending = 0usize;
    let mut last_seq = None;
    for res in firehose.range(start.to_be_bytes()..) {
        let (key, frame) = res?;
        let seq = u64::from_be_bytes(key.as_ref().try_into().unwrap_or_default());
        match frame_identity(&frame) {
            Ok(Some(identity)) => {
                batch.get_or_insert_with(|| live.batch()).insert(
                    &published,
                    identity,
                    key.as_ref(),
                );
                pending += 1;
                stats.published_indexed += 1;
            }
            Ok(None) => {}
            Err(err) => tracing::debug!(%err, %seq, "unindexable legacy frame"),
        }
        last_seq = Some(seq);
        if pending >= PUBLISHED_BATCH {
            let mut b = batch.take().unwrap_or_else(|| live.batch());
            b.insert(meta, META_PUBLISHED_INDEXED_UPTO, seq.to_be_bytes());
            b.commit()?;
            pending = 0;
        }
    }
    if let Some(seq) = last_seq {
        let mut b = batch.take().unwrap_or_else(|| live.batch());
        b.insert(meta, META_PUBLISHED_INDEXED_UPTO, seq.to_be_bytes());
        b.commit()?;
    }
    Ok(stats)
}

fn parse_sqlite_time(raw: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .or_else(|| DateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%.f%:z").ok())
        .map_or_else(Utc::now, |t| t.with_timezone(&Utc))
}

fn seed_host_cursors(
    live: &Keyspace, meta: &fjall::PartitionHandle, relay_db: &Path, mut stats: ReconcileStats,
) -> Result<ReconcileStats, MigrateError> {
    if !relay_db.exists() {
        return Ok(stats);
    }
    let conn = Connection::open_with_flags(
        relay_db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let has_table: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = 'hosts'",
        [],
        |row| row.get(0),
    )?;
    if !has_table {
        return Ok(stats);
    }
    let host_cursors =
        live.open_partition(PARTITION_HOST_CURSORS, PartitionCreateOptions::default())?;
    let mut stmt = conn.prepare("SELECT host, cursor, latest FROM hosts")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?))
    })?;
    for row in rows {
        let (host, cursor, latest) = row?;
        let seq = u64::try_from(cursor).unwrap_or_default();
        let mut gen_key = META_DOWNGRADE_GENERATION_PREFIX.to_vec();
        gen_key.extend_from_slice(host.as_bytes());
        let generation = meta_u64(meta, &gen_key)?.unwrap_or(0);
        let existing = host_cursors.get(&host)?.map(|v| HostCursor::decode(&v)).transpose()?;
        let seeded = match existing {
            Some(current) if current.generation > generation => continue,
            Some(current) if current.generation == generation && current.seq >= seq => continue,
            _ => HostCursor { generation, seq, time: parse_sqlite_time(&latest) },
        };
        host_cursors.insert(&host, seeded.encode()?)?;
        stats.hosts_seeded += 1;
    }
    Ok(stats)
}

/// Rebuilds the 0.2.x layout so `.prev` can run.
///
/// The legacy queue becomes an exact snapshot of outstanding `queue_v2` work and
/// `SQLite` gets the durable cursors. The v1 partitions stay for the next upgrade.
pub fn migrate_down(live: &Keyspace, relay_db: &Path) -> Result<DowngradeStats, MigrateError> {
    let mut stats = DowngradeStats::default();
    let meta = meta(live)?;
    if live.partition_exists(PARTITION_QUEUE_LEGACY) {
        let legacy = live.open_partition(PARTITION_QUEUE_LEGACY, legacy_queue_options())?;
        live.delete_partition(legacy)?;
    }
    let legacy = live.open_partition(PARTITION_QUEUE_LEGACY, legacy_queue_options())?;
    let queue = live.open_partition(PARTITION_QUEUE, PartitionCreateOptions::default())?;
    for res in queue.iter() {
        let (key, value) = res?;
        let entry = QueueEntry::decode(&value)?;
        let did = key
            .as_ref()
            .split(|b| *b == b'>')
            .next()
            .map(|d| String::from_utf8_lossy(d).into_owned())
            .unwrap_or_default();
        legacy.insert(entry.legacy_key(&did), entry.frame.as_slice())?;
        stats.queue_restored += 1;
    }
    let host_cursors =
        live.open_partition(PARTITION_HOST_CURSORS, PartitionCreateOptions::default())?;
    let mut conn = Connection::open(relay_db)?;
    conn.execute_batch("PRAGMA journal_mode = WAL")?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS hosts (host TEXT PRIMARY KEY, cursor INTEGER NOT NULL, latest TEXT NOT NULL)",
        (),
    )?;
    let tx = conn.transaction()?;
    let mut batch = live.batch();
    for res in host_cursors.iter() {
        let (host, value) = res?;
        let cursor = HostCursor::decode(&value)?;
        let host = String::from_utf8_lossy(&host).into_owned();
        tx.execute(
            "INSERT INTO hosts (host, cursor, latest) VALUES (?1, ?2, ?3)
             ON CONFLICT(host) DO UPDATE SET cursor = excluded.cursor, latest = excluded.latest",
            (&host, i64::try_from(cursor.seq).unwrap_or(i64::MAX), cursor.time),
        )?;
        let mut gen_key = META_DOWNGRADE_GENERATION_PREFIX.to_vec();
        gen_key.extend_from_slice(host.as_bytes());
        batch.insert(&meta, gen_key, cursor.generation.to_be_bytes());
        stats.hosts_written += 1;
    }
    tx.commit()?;
    batch.insert(&meta, META_DOWNGRADED_AT, Utc::now().to_rfc3339().into_bytes());
    batch.commit()?;
    live.persist(PersistMode::SyncAll)?;
    tracing::info!(?stats, "downgrade to the 0.2.x layout complete");
    Ok(stats)
}

#[cfg(all(test, not(feature = "labeler")))]
mod tests {
    use super::*;
    use crate::types::{PARTITION_PUBLISHED, open_keyspace, open_source_keyspace};
    use crate::validator::testutil::{
        CommitSpec, commit_frame, data_cid, identity_frame, sync_frame, test_key,
    };

    const DID: &str = "did:plc:migrationtestdid";

    struct Legacy {
        dir: tempfile::TempDir,
    }

    impl Legacy {
        fn db(&self) -> std::path::PathBuf {
            self.dir.path().join("db")
        }
        fn relay_db(&self) -> std::path::PathBuf {
            self.dir.path().join("relay.db")
        }
        fn source(&self) -> std::path::PathBuf {
            self.dir.path().join("db.legacy")
        }
    }

    fn commit(rev: &str, seq: u64) -> Vec<u8> {
        commit_frame(&CommitSpec {
            did: DID,
            rev,
            seq,
            data: data_cid(1),
            prev_data: None,
            key: &test_key(1),
            sig_override: None,
            too_big: false,
        })
        .0
    }

    /// Builds a 0.2.x-shaped keyspace + relay.db, then copies it to the source path
    /// the way deploy-relay.sh does while the relay is stopped.
    fn legacy_fixture() -> Legacy {
        let dir = tempfile::tempdir().unwrap();
        let legacy = Legacy { dir };
        {
            let ks = fjall::Config::new(legacy.db()).open().unwrap();
            let queue = ks.open_partition(PARTITION_QUEUE_LEGACY, legacy_queue_options()).unwrap();
            queue
                .insert(format!("{DID}>a.example>5").into_bytes(), commit("3mw73ekioet25", 5))
                .unwrap();
            queue
                .insert(format!("{DID}>b.example>9").into_bytes(), commit("3mw73ekioet26", 9))
                .unwrap();
            queue.insert(b"malformed-key".as_slice(), b"x".as_slice()).unwrap();
            let firehose =
                ks.open_partition(PARTITION_FIREHOSE, PartitionCreateOptions::default()).unwrap();
            firehose.insert(1u64.to_be_bytes(), commit("3mw73ekioet20", 1)).unwrap();
            firehose.insert(2u64.to_be_bytes(), identity_frame(DID, 2, None)).unwrap();
            firehose
                .insert(3u64.to_be_bytes(), sync_frame(DID, "3mw73ekioet21", 3, &test_key(1)).0)
                .unwrap();
            firehose.insert(4u64.to_be_bytes(), b"garbage".as_slice()).unwrap();
            ks.persist(PersistMode::SyncAll).unwrap();
            let conn = Connection::open(legacy.relay_db()).unwrap();
            conn.execute_batch(
                "CREATE TABLE hosts (host TEXT PRIMARY KEY, cursor INTEGER NOT NULL, latest TEXT NOT NULL);
                 INSERT INTO hosts VALUES ('a.example', 5, '2026-09-22 11:20:53.452+00:00');
                 INSERT INTO hosts VALUES ('b.example', 9, '2026-09-22T11:20:53.452Z');
                 INSERT INTO hosts VALUES ('c.example', 2, 'not a time');",
            )
            .unwrap();
        }
        copy_dir(&legacy.db(), &legacy.source());
        legacy
    }

    fn copy_dir(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_dir(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), target).unwrap();
            }
        }
    }

    #[test]
    fn fresh_install_marks_migrated_without_source() {
        let dir = tempfile::tempdir().unwrap();
        let ks = open_keyspace(&dir.path().join("db")).unwrap();
        assert!(needs_reconcile(&ks).unwrap());
        assert!(!has_legacy_state(&ks, &dir.path().join("relay.db")).unwrap());
        // an empty legacy queue partition or an empty hosts table is still "no state"
        ks.open_partition(PARTITION_QUEUE_LEGACY, legacy_queue_options()).unwrap();
        let relay_db = dir.path().join("relay.db");
        Connection::open(&relay_db)
            .unwrap()
            .execute_batch("CREATE TABLE hosts (host TEXT PRIMARY KEY, cursor INTEGER NOT NULL, latest TEXT NOT NULL)")
            .unwrap();
        assert!(!has_legacy_state(&ks, &relay_db).unwrap());
        let empty_db = dir.path().join("empty.db");
        Connection::open(&empty_db).unwrap().execute_batch("CREATE TABLE other (x)").unwrap();
        assert!(!has_legacy_state(&ks, &empty_db).unwrap());
        mark_migrated(&ks).unwrap();
        assert!(!needs_reconcile(&ks).unwrap());
    }

    #[test]
    fn reconcile_imports_queue_indexes_published_and_seeds_cursors_idempotently() {
        let legacy = legacy_fixture();
        let live = open_keyspace(&legacy.db()).unwrap();
        assert!(needs_reconcile(&live).unwrap());
        assert!(has_legacy_state(&live, &legacy.relay_db()).unwrap());
        let source = open_source_keyspace(&legacy.source()).unwrap();
        let stats = reconcile(&live, &source, &legacy.relay_db()).unwrap();
        assert_eq!(stats.queue_imported, 2);
        assert_eq!(stats.queue_skipped, 1, "malformed legacy key is skipped");
        assert_eq!(
            stats.published_indexed, 2,
            "commit and sync get identities; identity and garbage do not"
        );
        assert_eq!(stats.hosts_seeded, 3);
        assert!(!needs_reconcile(&live).unwrap());

        let queue =
            live.open_partition(PARTITION_QUEUE, PartitionCreateOptions::default()).unwrap();
        let entries: Vec<_> = queue.iter().map(|r| r.unwrap()).collect();
        assert_eq!(entries.len(), 2);
        let first = QueueEntry::decode(&entries[0].1).unwrap();
        assert_eq!((first.host.as_str(), first.seq, first.generation), ("a.example", 5, 0));
        assert_eq!(first.frame, commit("3mw73ekioet25", 5));
        assert_eq!(entries[0].0.as_ref(), QueueEntry::key(DID, 1).as_slice());
        let meta = live.open_partition(PARTITION_META, PartitionCreateOptions::default()).unwrap();
        assert_eq!(meta_u64(&meta, META_ADMISSION).unwrap(), Some(2));
        assert_eq!(meta_u64(&meta, META_PUBLISHED_INDEXED_UPTO).unwrap(), Some(4));

        let published =
            live.open_partition(PARTITION_PUBLISHED, PartitionCreateOptions::default()).unwrap();
        assert_eq!(published.len().unwrap(), 2);
        let host_cursors =
            live.open_partition(PARTITION_HOST_CURSORS, PartitionCreateOptions::default()).unwrap();
        let a = HostCursor::decode(&host_cursors.get("a.example").unwrap().unwrap()).unwrap();
        assert_eq!((a.generation, a.seq), (0, 5));
        assert_eq!(a.time.to_rfc3339(), "2026-09-22T11:20:53.452+00:00");
        let b = HostCursor::decode(&host_cursors.get("b.example").unwrap().unwrap()).unwrap();
        assert_eq!(b.time.to_rfc3339(), "2026-09-22T11:20:53.452+00:00");
        let c = HostCursor::decode(&host_cursors.get("c.example").unwrap().unwrap()).unwrap();
        assert!(c.time > b.time, "unparseable latest falls back to now");

        // a host whose durable generation moved past the downgrade record keeps its cursor
        host_cursors
            .insert(
                "b.example",
                HostCursor { generation: 3, seq: 1, time: Utc::now() }.encode().unwrap(),
            )
            .unwrap();
        // second pass: nothing new
        let again = reconcile(&live, &source, &legacy.relay_db()).unwrap();
        let b = HostCursor::decode(&host_cursors.get("b.example").unwrap().unwrap()).unwrap();
        assert_eq!((b.generation, b.seq), (3, 1));
        assert_eq!(again.queue_imported, 0);
        assert_eq!(again.queue_skipped, 3);
        assert_eq!(again.published_indexed, 0);
        assert_eq!(again.hosts_seeded, 0, "fjall cursors already at or past SQLite");
        assert_eq!(queue.len().unwrap(), 2);
    }

    #[test]
    fn downgrade_then_reupgrade_round_trips_new_work_and_generations() {
        let legacy = legacy_fixture();
        let live = open_keyspace(&legacy.db()).unwrap();
        let source = open_source_keyspace(&legacy.source()).unwrap();
        reconcile(&live, &source, &legacy.relay_db()).unwrap();
        drop(source);
        // the new binary drained one entry and moved a.example to generation 1 seq 10
        let queue =
            live.open_partition(PARTITION_QUEUE, PartitionCreateOptions::default()).unwrap();
        queue.remove(QueueEntry::key(DID, 1)).unwrap();
        let host_cursors =
            live.open_partition(PARTITION_HOST_CURSORS, PartitionCreateOptions::default()).unwrap();
        host_cursors
            .insert(
                "a.example",
                HostCursor { generation: 1, seq: 10, time: Utc::now() }.encode().unwrap(),
            )
            .unwrap();

        let stats = migrate_down(&live, &legacy.relay_db()).unwrap();
        assert_eq!(stats.queue_restored, 1, "only outstanding work is restored");
        assert_eq!(stats.hosts_written, 3);
        assert!(needs_reconcile(&live).unwrap());
        let legacy_queue =
            live.open_partition(PARTITION_QUEUE_LEGACY, legacy_queue_options()).unwrap();
        let keys: Vec<_> =
            legacy_queue.keys().map(|k| String::from_utf8(k.unwrap().to_vec()).unwrap()).collect();
        assert_eq!(keys, vec![format!("{DID}>b.example>9")]);
        let conn = Connection::open(legacy.relay_db()).unwrap();
        let a: i64 = conn
            .query_row("SELECT cursor FROM hosts WHERE host = 'a.example'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(a, 10);

        // old binary runs: queues a new commit under a reused legacy key with different content,
        // discovers a new host, and advances a.example within its (unknown) generation
        legacy_queue
            .insert(format!("{DID}>a.example>5").into_bytes(), commit("3mw73ekioet30", 5))
            .unwrap();
        conn.execute_batch(
            "UPDATE hosts SET cursor = 12 WHERE host = 'a.example';
             INSERT INTO hosts VALUES ('new.example', 3, '2026-09-23T00:00:00Z');",
        )
        .unwrap();
        live.persist(PersistMode::SyncAll).unwrap();
        drop(legacy_queue);
        drop(queue);
        drop(host_cursors);
        drop(live);
        std::fs::remove_dir_all(legacy.source()).unwrap();
        copy_dir(&legacy.db(), &legacy.source());

        let live = open_keyspace(&legacy.db()).unwrap();
        let source = open_source_keyspace(&legacy.source()).unwrap();
        let stats = reconcile(&live, &source, &legacy.relay_db()).unwrap();
        assert_eq!(stats.queue_imported, 1, "same key, new digest is new work");
        assert_eq!(
            stats.hosts_seeded, 2,
            "a.example advanced within generation 1; new.example seeded"
        );
        let host_cursors =
            live.open_partition(PARTITION_HOST_CURSORS, PartitionCreateOptions::default()).unwrap();
        let a = HostCursor::decode(&host_cursors.get("a.example").unwrap().unwrap()).unwrap();
        assert_eq!((a.generation, a.seq), (1, 12));
        let n = HostCursor::decode(&host_cursors.get("new.example").unwrap().unwrap()).unwrap();
        assert_eq!((n.generation, n.seq), (0, 3));
        let queue =
            live.open_partition(PARTITION_QUEUE, PartitionCreateOptions::default()).unwrap();
        assert_eq!(queue.len().unwrap(), 2, "b.example entry kept, new a.example entry imported");
        assert!(!needs_reconcile(&live).unwrap());
    }

    #[test]
    fn reconcile_without_legacy_partitions_is_a_noop() {
        let dir = tempfile::tempdir().unwrap();
        let live = open_keyspace(&dir.path().join("db")).unwrap();
        let source = open_source_keyspace(&dir.path().join("src")).unwrap();
        let stats = reconcile(&live, &source, &dir.path().join("missing.db")).unwrap();
        assert_eq!(stats, ReconcileStats::default());
        let stats = migrate_down(&live, &dir.path().join("relay.db")).unwrap();
        assert_eq!(stats, DowngradeStats::default());
    }

    #[test]
    fn helpers_cover_edge_cases() {
        assert_eq!(parse_legacy_key(b"did>host>7"), Some(("did".to_owned(), "host".to_owned(), 7)));
        assert_eq!(parse_legacy_key(b"did>host>x"), None);
        assert_eq!(parse_legacy_key(b"did"), None);
        assert_eq!(parse_legacy_key(&[0xff]), None);
        assert!(frame_identity(b"\xff").is_err());
        assert_eq!(frame_identity(&identity_frame(DID, 1, None)).unwrap(), None);
        assert_eq!(frame_identity(&crate::validator::testutil::info_frame("x")).unwrap(), None);
        assert_eq!(
            parse_sqlite_time("2026-09-22T11:20:53.452Z").to_rfc3339(),
            "2026-09-22T11:20:53.452+00:00"
        );
        let err = MigrateError::SourceRequired;
        assert!(err.to_string().contains("legacy-source"));
    }

    #[test]
    fn published_index_flushes_in_batches() {
        let dir = tempfile::tempdir().unwrap();
        let source_path = dir.path().join("src");
        {
            let ks = fjall::Config::new(&source_path).open().unwrap();
            let firehose =
                ks.open_partition(PARTITION_FIREHOSE, PartitionCreateOptions::default()).unwrap();
            for seq in 1..=(PUBLISHED_BATCH as u64 + 5) {
                firehose
                    .insert(seq.to_be_bytes(), commit(&format!("3mw73ekio{seq:04}"), seq))
                    .unwrap();
            }
            ks.persist(PersistMode::SyncAll).unwrap();
        }
        let live = open_keyspace(&dir.path().join("db")).unwrap();
        let source = open_source_keyspace(&source_path).unwrap();
        let stats = reconcile(&live, &source, &dir.path().join("relay.db")).unwrap();
        assert_eq!(stats.published_indexed, u64::try_from(PUBLISHED_BATCH + 5).unwrap());
        let published =
            live.open_partition(PARTITION_PUBLISHED, PartitionCreateOptions::default()).unwrap();
        assert_eq!(published.len().unwrap(), PUBLISHED_BATCH + 5);
    }
}
