//! Permissioned repo storage (spec §Permissioned repos, §Incremental sync).
//!
//! One repo is one account's records within one space. A repo carries three
//! pieces of state, all advanced atomically by [`RepoStore::apply_writes`]:
//!
//! - the records themselves, keyed `(collection, rkey)`;
//! - the LtHash state, from which the commit `hash` is derived;
//! - an operation log, the transport optimization syncers page through.
//!
//! Commits are not stored. `ikm` is fresh per reader, so a commit is minted at
//! serve time from the persisted `(rev, state)` pair.

use async_trait::async_trait;
use rsky_space::lthash::{element, LtHash};
use rsky_space::record::dag_cbor_cid;
use rusqlite::{Connection, OptionalExtension};
use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::error::{HostError, Result};

/// Size cap on a single stored record. Storage is shape-agnostic, so this and
/// DAG-CBOR well-formedness are the only limits the host applies to a value.
pub const MAX_RECORD_BYTES: usize = 64 * 1024;

const STATE_BYTES: usize = 2048;

/// One mutation in an atomic batch. Values are already-encoded DAG-CBOR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoWrite {
    Create {
        collection: String,
        rkey: String,
        value: Vec<u8>,
    },
    Update {
        collection: String,
        rkey: String,
        value: Vec<u8>,
        swap_record: Option<String>,
    },
    Delete {
        collection: String,
        rkey: String,
        swap_record: Option<String>,
    },
}

impl RepoWrite {
    pub fn collection(&self) -> &str {
        match self {
            Self::Create { collection, .. }
            | Self::Update { collection, .. }
            | Self::Delete { collection, .. } => collection,
        }
    }

    pub fn rkey(&self) -> &str {
        match self {
            Self::Create { rkey, .. } | Self::Update { rkey, .. } | Self::Delete { rkey, .. } => {
                rkey
            }
        }
    }
}

/// What a write did. A delete of an absent record is [`WriteOutcome::Noop`]:
/// it produces no oplog entry and leaves the digest untouched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteOutcome {
    Created { cid: String },
    Updated { cid: String },
    Deleted,
    Noop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRecord {
    pub collection: String,
    pub rkey: String,
    pub cid: String,
    pub value: Vec<u8>,
}

impl StoredRecord {
    /// The `{collection}/{rkey}` path used as the list cursor and CAR index key.
    pub fn path(&self) -> String {
        record_path(&self.collection, &self.rkey)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredOp {
    pub seq: i64,
    pub rev: String,
    pub collection: String,
    pub rkey: String,
    pub cid: Option<String>,
    pub prev: Option<String>,
}

/// A repo's current position: the latest revision and the LtHash state behind
/// its commit digest.
#[derive(Clone)]
pub struct RepoHead {
    pub rev: String,
    pub state: [u8; STATE_BYTES],
}

impl RepoHead {
    pub fn hash(&self) -> [u8; 32] {
        LtHash::from_state_bytes(&self.state).hash()
    }
}

/// A page of the operation log. `complete` is true when the page reaches the
/// repo's head, which is when a caller may attach the current commit.
pub struct OpPage {
    pub ops: Vec<StoredOp>,
    pub cursor: Option<String>,
    pub complete: bool,
}

/// The result of an atomic batch: the new head plus one outcome per write.
pub struct Applied {
    pub rev: String,
    pub hash: [u8; 32],
    pub outcomes: Vec<WriteOutcome>,
}

pub fn record_path(collection: &str, rkey: &str) -> String {
    format!("{collection}/{rkey}")
}

#[async_trait]
pub trait RepoStore: Send + Sync {
    /// Apply a batch atomically at revision `rev`. Every write in the batch
    /// shares the revision, which is how syncers see them as one mutation.
    async fn apply_writes(
        &self,
        space_uri: &str,
        did: &str,
        rev: &str,
        writes: &[RepoWrite],
    ) -> Result<Applied>;

    async fn head(&self, space_uri: &str, did: &str) -> Result<Option<RepoHead>>;

    async fn get_record(
        &self,
        space_uri: &str,
        did: &str,
        collection: &str,
        rkey: &str,
    ) -> Result<Option<StoredRecord>>;

    /// Records ordered by `{collection}/{rkey}`; `cursor` is the last path of
    /// the previous page.
    async fn list_records(
        &self,
        space_uri: &str,
        did: &str,
        collection: Option<&str>,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<(Vec<StoredRecord>, Option<String>)>;

    /// Operations after `since` (a revision), ordered by insertion. Returns
    /// [`HostError::HistoryUnavailable`] when `since` predates the retained
    /// window, which is a syncer's signal to fall back to full-state recovery.
    async fn list_ops(
        &self,
        space_uri: &str,
        did: &str,
        since: Option<&str>,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<OpPage>;

    /// Drop a repo entirely (account deletion, space deletion).
    async fn delete_repo(&self, space_uri: &str, did: &str) -> Result<()>;
}

/// Fold one batch into an existing record set + digest. Shared by both
/// backings so their semantics cannot drift.
fn plan_batch(
    existing: &BTreeMap<String, StoredRecord>,
    lt: &mut LtHash,
    writes: &[RepoWrite],
) -> Result<Vec<PlannedWrite>> {
    let mut planned = Vec::with_capacity(writes.len());
    let mut seen: BTreeMap<String, Option<String>> = BTreeMap::new();

    for write in writes {
        let path = record_path(write.collection(), write.rkey());
        let current = match seen.get(&path) {
            Some(cid) => cid.clone(),
            None => existing.get(&path).map(|r| r.cid.clone()),
        };

        let planned_write = match write {
            RepoWrite::Create {
                collection,
                rkey,
                value,
            } => {
                if current.is_some() {
                    return Err(HostError::InvalidRequest(format!(
                        "record already exists: {path}"
                    )));
                }
                let cid = dag_cbor_cid(value).to_string();
                lt.add(&element(collection, rkey, &cid));
                PlannedWrite {
                    collection: collection.clone(),
                    rkey: rkey.clone(),
                    cid: Some(cid.clone()),
                    prev: None,
                    value: Some(value.clone()),
                    outcome: WriteOutcome::Created { cid },
                }
            }
            RepoWrite::Update {
                collection,
                rkey,
                value,
                swap_record,
            } => {
                check_swap(swap_record.as_deref(), current.as_deref())?;
                let cid = dag_cbor_cid(value).to_string();
                if let Some(prev) = &current {
                    lt.remove(&element(collection, rkey, prev));
                }
                lt.add(&element(collection, rkey, &cid));
                PlannedWrite {
                    collection: collection.clone(),
                    rkey: rkey.clone(),
                    cid: Some(cid.clone()),
                    prev: current.clone(),
                    value: Some(value.clone()),
                    outcome: WriteOutcome::Updated { cid },
                }
            }
            RepoWrite::Delete {
                collection,
                rkey,
                swap_record,
            } => {
                check_swap(swap_record.as_deref(), current.as_deref())?;
                let Some(prev) = current.clone() else {
                    planned.push(PlannedWrite {
                        collection: collection.clone(),
                        rkey: rkey.clone(),
                        cid: None,
                        prev: None,
                        value: None,
                        outcome: WriteOutcome::Noop,
                    });
                    continue;
                };
                lt.remove(&element(collection, rkey, &prev));
                PlannedWrite {
                    collection: collection.clone(),
                    rkey: rkey.clone(),
                    cid: None,
                    prev: Some(prev),
                    value: None,
                    outcome: WriteOutcome::Deleted,
                }
            }
        };

        seen.insert(path, planned_write.cid.clone());
        planned.push(planned_write);
    }
    Ok(planned)
}

fn check_swap(swap: Option<&str>, current: Option<&str>) -> Result<()> {
    match swap {
        Some(expected) if current != Some(expected) => Err(HostError::InvalidSwap),
        _ => Ok(()),
    }
}

struct PlannedWrite {
    collection: String,
    rkey: String,
    cid: Option<String>,
    prev: Option<String>,
    value: Option<Vec<u8>>,
    outcome: WriteOutcome,
}

impl PlannedWrite {
    fn is_noop(&self) -> bool {
        self.outcome == WriteOutcome::Noop
    }
}

fn page_cursor<T>(page: &[T], limit: u32, key: impl Fn(&T) -> String) -> Option<String> {
    match page.last() {
        Some(last) if page.len() == limit as usize => Some(key(last)),
        _ => None,
    }
}

// ---------------------------------------------------------------- in memory

struct MemRepo {
    records: BTreeMap<String, StoredRecord>,
    ops: Vec<StoredOp>,
    rev: String,
    state: [u8; STATE_BYTES],
}

impl Default for MemRepo {
    fn default() -> Self {
        Self {
            records: BTreeMap::new(),
            ops: Vec::new(),
            rev: String::new(),
            state: [0u8; STATE_BYTES],
        }
    }
}

#[derive(Default)]
pub struct InMemoryRepos {
    repos: Mutex<BTreeMap<(String, String), MemRepo>>,
    next_seq: Mutex<i64>,
}

#[async_trait]
impl RepoStore for InMemoryRepos {
    async fn apply_writes(
        &self,
        space_uri: &str,
        did: &str,
        rev: &str,
        writes: &[RepoWrite],
    ) -> Result<Applied> {
        let mut repos = self.repos.lock().unwrap();
        let repo = repos
            .entry((space_uri.to_string(), did.to_string()))
            .or_default();
        let mut lt = LtHash::from_state_bytes(&repo.state);
        let planned = plan_batch(&repo.records, &mut lt, writes)?;

        let mut seq = self.next_seq.lock().unwrap();
        for p in &planned {
            if p.is_noop() {
                continue;
            }
            let path = record_path(&p.collection, &p.rkey);
            match &p.cid {
                Some(cid) => {
                    repo.records.insert(
                        path,
                        StoredRecord {
                            collection: p.collection.clone(),
                            rkey: p.rkey.clone(),
                            cid: cid.clone(),
                            value: p.value.clone().unwrap_or_default(),
                        },
                    );
                }
                None => {
                    repo.records.remove(&path);
                }
            }
            *seq += 1;
            repo.ops.push(StoredOp {
                seq: *seq,
                rev: rev.to_string(),
                collection: p.collection.clone(),
                rkey: p.rkey.clone(),
                cid: p.cid.clone(),
                prev: p.prev.clone(),
            });
        }

        repo.state = lt.state_bytes();
        if planned.iter().any(|p| !p.is_noop()) {
            repo.rev = rev.to_string();
        }
        Ok(Applied {
            rev: repo.rev.clone(),
            hash: lt.hash(),
            outcomes: planned.into_iter().map(|p| p.outcome).collect(),
        })
    }

    async fn head(&self, space_uri: &str, did: &str) -> Result<Option<RepoHead>> {
        Ok(self
            .repos
            .lock()
            .unwrap()
            .get(&(space_uri.to_string(), did.to_string()))
            .map(|r| RepoHead {
                rev: r.rev.clone(),
                state: r.state,
            }))
    }

    async fn get_record(
        &self,
        space_uri: &str,
        did: &str,
        collection: &str,
        rkey: &str,
    ) -> Result<Option<StoredRecord>> {
        Ok(self
            .repos
            .lock()
            .unwrap()
            .get(&(space_uri.to_string(), did.to_string()))
            .and_then(|r| r.records.get(&record_path(collection, rkey)).cloned()))
    }

    async fn list_records(
        &self,
        space_uri: &str,
        did: &str,
        collection: Option<&str>,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<(Vec<StoredRecord>, Option<String>)> {
        let repos = self.repos.lock().unwrap();
        let Some(repo) = repos.get(&(space_uri.to_string(), did.to_string())) else {
            return Err(HostError::RepoNotFound);
        };
        let page: Vec<StoredRecord> = repo
            .records
            .iter()
            .filter(|(path, record)| {
                collection.is_none_or(|c| record.collection == c)
                    && cursor.is_none_or(|c| path.as_str() > c)
            })
            .take(limit as usize)
            .map(|(_, record)| record.clone())
            .collect();
        let cursor = page_cursor(&page, limit, |r| r.path());
        Ok((page, cursor))
    }

    async fn list_ops(
        &self,
        space_uri: &str,
        did: &str,
        since: Option<&str>,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<OpPage> {
        let repos = self.repos.lock().unwrap();
        let Some(repo) = repos.get(&(space_uri.to_string(), did.to_string())) else {
            return Err(HostError::RepoNotFound);
        };
        ensure_history(since, repo.ops.first().map(|o| o.rev.as_str()))?;
        let after = parse_cursor(cursor)?;
        let page: Vec<StoredOp> = repo
            .ops
            .iter()
            .filter(|op| {
                since.is_none_or(|s| op.rev.as_str() > s) && after.is_none_or(|c| op.seq > c)
            })
            .take(limit as usize)
            .cloned()
            .collect();
        Ok(finish_op_page(page, limit, repo.ops.last().map(|o| o.seq)))
    }

    async fn delete_repo(&self, space_uri: &str, did: &str) -> Result<()> {
        self.repos
            .lock()
            .unwrap()
            .remove(&(space_uri.to_string(), did.to_string()));
        Ok(())
    }
}

fn ensure_history(since: Option<&str>, earliest_retained: Option<&str>) -> Result<()> {
    match (since, earliest_retained) {
        // Every retained operation is already newer than `since`, so the
        // caller's revision fell out of the window (or never existed here).
        (Some(since), Some(earliest)) if earliest > since => Err(HostError::HistoryUnavailable),
        (Some(_), None) => Err(HostError::HistoryUnavailable),
        _ => Ok(()),
    }
}

fn parse_cursor(cursor: Option<&str>) -> Result<Option<i64>> {
    cursor
        .map(|c| {
            c.parse::<i64>()
                .map_err(|_| HostError::InvalidRequest(format!("invalid cursor: {c}")))
        })
        .transpose()
}

fn finish_op_page(ops: Vec<StoredOp>, limit: u32, last_seq: Option<i64>) -> OpPage {
    let reached_head = ops.last().map(|o| o.seq) == last_seq;
    let cursor = page_cursor(&ops, limit, |o| o.seq.to_string());
    OpPage {
        complete: reached_head,
        cursor: if reached_head { None } else { cursor },
        ops,
    }
}

// ------------------------------------------------------------------- sqlite

/// SQLite-backed repo storage. Volume per host is modest and every batch is a
/// single transaction, so one connection behind a mutex is sufficient.
pub struct SqliteRepos {
    conn: Mutex<Connection>,
}

impl SqliteRepos {
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory().map_err(sql_err)?)
    }

    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self> {
        Self::init(Connection::open(path).map_err(sql_err)?)
    }

    pub fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS repo (
                space_uri TEXT NOT NULL,
                did TEXT NOT NULL,
                rev TEXT NOT NULL DEFAULT '',
                state BLOB NOT NULL,
                PRIMARY KEY (space_uri, did)
            );
            CREATE TABLE IF NOT EXISTS record (
                space_uri TEXT NOT NULL,
                did TEXT NOT NULL,
                path TEXT NOT NULL,
                collection TEXT NOT NULL,
                rkey TEXT NOT NULL,
                cid TEXT NOT NULL,
                value BLOB NOT NULL,
                PRIMARY KEY (space_uri, did, path)
            );
            CREATE TABLE IF NOT EXISTS repo_op (
                seq INTEGER PRIMARY KEY AUTOINCREMENT,
                space_uri TEXT NOT NULL,
                did TEXT NOT NULL,
                rev TEXT NOT NULL,
                collection TEXT NOT NULL,
                rkey TEXT NOT NULL,
                cid TEXT,
                prev TEXT
            );
            CREATE INDEX IF NOT EXISTS repo_op_repo_seq ON repo_op (space_uri, did, seq);",
        )
        .map_err(sql_err)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

fn sql_err(e: rusqlite::Error) -> HostError {
    HostError::Store(e.to_string())
}

fn state_from_blob(blob: Vec<u8>) -> Result<[u8; STATE_BYTES]> {
    blob.try_into()
        .map_err(|_| HostError::Store("corrupt lthash state".into()))
}

fn row_to_record(row: &rusqlite::Row) -> rusqlite::Result<StoredRecord> {
    Ok(StoredRecord {
        collection: row.get("collection")?,
        rkey: row.get("rkey")?,
        cid: row.get("cid")?,
        value: row.get("value")?,
    })
}

fn row_to_op(row: &rusqlite::Row) -> rusqlite::Result<StoredOp> {
    Ok(StoredOp {
        seq: row.get("seq")?,
        rev: row.get("rev")?,
        collection: row.get("collection")?,
        rkey: row.get("rkey")?,
        cid: row.get("cid")?,
        prev: row.get("prev")?,
    })
}

#[async_trait]
impl RepoStore for SqliteRepos {
    async fn apply_writes(
        &self,
        space_uri: &str,
        did: &str,
        rev: &str,
        writes: &[RepoWrite],
    ) -> Result<Applied> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction().map_err(sql_err)?;

        let existing_state: Option<Vec<u8>> = tx
            .query_row(
                "SELECT state FROM repo WHERE space_uri = ?1 AND did = ?2",
                rusqlite::params![space_uri, did],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql_err)?;
        let mut current_rev: String = tx
            .query_row(
                "SELECT rev FROM repo WHERE space_uri = ?1 AND did = ?2",
                rusqlite::params![space_uri, did],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql_err)?
            .unwrap_or_default();

        let mut lt = match existing_state {
            Some(blob) => LtHash::from_state_bytes(&state_from_blob(blob)?),
            None => LtHash::new(),
        };

        // Only the paths this batch touches are needed to plan it.
        let mut existing = BTreeMap::new();
        for write in writes {
            let path = record_path(write.collection(), write.rkey());
            if let Some(record) = tx
                .query_row(
                    "SELECT collection, rkey, cid, value FROM record
                     WHERE space_uri = ?1 AND did = ?2 AND path = ?3",
                    rusqlite::params![space_uri, did, path],
                    row_to_record,
                )
                .optional()
                .map_err(sql_err)?
            {
                existing.insert(path, record);
            }
        }

        let planned = plan_batch(&existing, &mut lt, writes)?;
        for p in &planned {
            if p.is_noop() {
                continue;
            }
            let path = record_path(&p.collection, &p.rkey);
            match (&p.cid, &p.value) {
                (Some(cid), Some(value)) => {
                    tx.execute(
                        "INSERT INTO record (space_uri, did, path, collection, rkey, cid, value)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                         ON CONFLICT (space_uri, did, path)
                         DO UPDATE SET cid = ?6, value = ?7",
                        rusqlite::params![space_uri, did, path, p.collection, p.rkey, cid, value],
                    )
                    .map_err(sql_err)?;
                }
                _ => {
                    tx.execute(
                        "DELETE FROM record WHERE space_uri = ?1 AND did = ?2 AND path = ?3",
                        rusqlite::params![space_uri, did, path],
                    )
                    .map_err(sql_err)?;
                }
            }
            tx.execute(
                "INSERT INTO repo_op (space_uri, did, rev, collection, rkey, cid, prev)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![space_uri, did, rev, p.collection, p.rkey, p.cid, p.prev],
            )
            .map_err(sql_err)?;
            current_rev = rev.to_string();
        }

        tx.execute(
            "INSERT INTO repo (space_uri, did, rev, state) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (space_uri, did) DO UPDATE SET rev = ?3, state = ?4",
            rusqlite::params![space_uri, did, current_rev, lt.state_bytes().to_vec()],
        )
        .map_err(sql_err)?;
        tx.commit().map_err(sql_err)?;

        Ok(Applied {
            rev: current_rev,
            hash: lt.hash(),
            outcomes: planned.into_iter().map(|p| p.outcome).collect(),
        })
    }

    async fn head(&self, space_uri: &str, did: &str) -> Result<Option<RepoHead>> {
        let conn = self.conn.lock().unwrap();
        let row: Option<(String, Vec<u8>)> = conn
            .query_row(
                "SELECT rev, state FROM repo WHERE space_uri = ?1 AND did = ?2",
                rusqlite::params![space_uri, did],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(sql_err)?;
        row.map(|(rev, state)| {
            Ok(RepoHead {
                rev,
                state: state_from_blob(state)?,
            })
        })
        .transpose()
    }

    async fn get_record(
        &self,
        space_uri: &str,
        did: &str,
        collection: &str,
        rkey: &str,
    ) -> Result<Option<StoredRecord>> {
        self.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT collection, rkey, cid, value FROM record
                 WHERE space_uri = ?1 AND did = ?2 AND path = ?3",
                rusqlite::params![space_uri, did, record_path(collection, rkey)],
                row_to_record,
            )
            .optional()
            .map_err(sql_err)
    }

    async fn list_records(
        &self,
        space_uri: &str,
        did: &str,
        collection: Option<&str>,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<(Vec<StoredRecord>, Option<String>)> {
        let conn = self.conn.lock().unwrap();
        require_repo(&conn, space_uri, did)?;
        let mut stmt = conn
            .prepare(
                "SELECT collection, rkey, cid, value FROM record
                 WHERE space_uri = ?1 AND did = ?2 AND path > ?3
                   AND (?4 IS NULL OR collection = ?4)
                 ORDER BY path ASC LIMIT ?5",
            )
            .map_err(sql_err)?;
        let page = stmt
            .query_map(
                rusqlite::params![space_uri, did, cursor.unwrap_or(""), collection, limit],
                row_to_record,
            )
            .map_err(sql_err)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(sql_err)?;
        let cursor = page_cursor(&page, limit, |r| r.path());
        Ok((page, cursor))
    }

    async fn list_ops(
        &self,
        space_uri: &str,
        did: &str,
        since: Option<&str>,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<OpPage> {
        let conn = self.conn.lock().unwrap();
        require_repo(&conn, space_uri, did)?;
        let bounds: (Option<String>, Option<i64>) = conn
            .query_row(
                "SELECT (SELECT rev FROM repo_op WHERE space_uri = ?1 AND did = ?2
                          ORDER BY seq ASC LIMIT 1),
                        (SELECT seq FROM repo_op WHERE space_uri = ?1 AND did = ?2
                          ORDER BY seq DESC LIMIT 1)",
                rusqlite::params![space_uri, did],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(sql_err)?;
        ensure_history(since, bounds.0.as_deref())?;
        let after = parse_cursor(cursor)?;

        let mut stmt = conn
            .prepare(
                "SELECT seq, rev, collection, rkey, cid, prev FROM repo_op
                 WHERE space_uri = ?1 AND did = ?2
                   AND (?3 IS NULL OR rev > ?3)
                   AND (?4 IS NULL OR seq > ?4)
                 ORDER BY seq ASC LIMIT ?5",
            )
            .map_err(sql_err)?;
        let ops = stmt
            .query_map(
                rusqlite::params![space_uri, did, since, after, limit],
                row_to_op,
            )
            .map_err(sql_err)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(sql_err)?;
        Ok(finish_op_page(ops, limit, bounds.1))
    }

    async fn delete_repo(&self, space_uri: &str, did: &str) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction().map_err(sql_err)?;
        for table in ["record", "repo_op", "repo"] {
            tx.execute(
                &format!("DELETE FROM {table} WHERE space_uri = ?1 AND did = ?2"),
                rusqlite::params![space_uri, did],
            )
            .map_err(sql_err)?;
        }
        tx.commit().map_err(sql_err)
    }
}

fn require_repo(conn: &Connection, space_uri: &str, did: &str) -> Result<()> {
    let exists: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM repo WHERE space_uri = ?1 AND did = ?2",
            rusqlite::params![space_uri, did],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql_err)?;
    exists.map(|_| ()).ok_or(HostError::RepoNotFound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SPACE: &str = "at://did:plc:auth/space/community.blacksky.feed/main";
    const DID: &str = "did:plc:member";
    const POST: &str = "app.bsky.feed.post";
    const LIKE: &str = "app.bsky.feed.like";

    fn value(text: &str) -> Vec<u8> {
        rsky_space::record::encode_record(&json!({"text": text}), MAX_RECORD_BYTES).unwrap()
    }

    fn create(rkey: &str, text: &str) -> RepoWrite {
        RepoWrite::Create {
            collection: POST.to_string(),
            rkey: rkey.to_string(),
            value: value(text),
        }
    }

    fn delete(rkey: &str) -> RepoWrite {
        RepoWrite::Delete {
            collection: POST.to_string(),
            rkey: rkey.to_string(),
            swap_record: None,
        }
    }

    async fn exercise_write_read_cycle(store: &dyn RepoStore) {
        let applied = store
            .apply_writes(SPACE, DID, "3rev1", &[create("a", "one")])
            .await
            .unwrap();
        assert_eq!(applied.rev, "3rev1");
        assert!(matches!(applied.outcomes[0], WriteOutcome::Created { .. }));

        let got = store
            .get_record(SPACE, DID, POST, "a")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            rsky_space::record::decode_record(&got.value).unwrap(),
            json!({"text": "one"})
        );
        assert_eq!(got.cid, dag_cbor_cid(&value("one")).to_string());

        // The digest matches an independent fold over the record set.
        let head = store.head(SPACE, DID).await.unwrap().unwrap();
        let mut expected = LtHash::new();
        expected.add(&element(POST, "a", &got.cid));
        assert_eq!(head.hash(), expected.hash());

        // Update swaps the element, delete removes it, digest returns to empty.
        store
            .apply_writes(
                SPACE,
                DID,
                "3rev2",
                &[RepoWrite::Update {
                    collection: POST.to_string(),
                    rkey: "a".to_string(),
                    value: value("two"),
                    swap_record: Some(got.cid.clone()),
                }],
            )
            .await
            .unwrap();
        let updated = store
            .get_record(SPACE, DID, POST, "a")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.cid, dag_cbor_cid(&value("two")).to_string());

        store
            .apply_writes(SPACE, DID, "3rev3", &[delete("a")])
            .await
            .unwrap();
        assert!(store
            .get_record(SPACE, DID, POST, "a")
            .await
            .unwrap()
            .is_none());
        assert_eq!(
            store.head(SPACE, DID).await.unwrap().unwrap().hash(),
            LtHash::new().hash()
        );
    }

    async fn exercise_swap_and_conflict(store: &dyn RepoStore) {
        store
            .apply_writes(SPACE, DID, "3rev1", &[create("a", "one")])
            .await
            .unwrap();

        assert!(matches!(
            store
                .apply_writes(SPACE, DID, "3rev2", &[create("a", "again")])
                .await,
            Err(HostError::InvalidRequest(_))
        ));
        assert!(matches!(
            store
                .apply_writes(
                    SPACE,
                    DID,
                    "3rev2",
                    &[RepoWrite::Delete {
                        collection: POST.to_string(),
                        rkey: "a".to_string(),
                        swap_record: Some("bafyreiwrong".to_string()),
                    }]
                )
                .await,
            Err(HostError::InvalidSwap)
        ));
        // A swap against an absent record is also a swap failure.
        assert!(matches!(
            store
                .apply_writes(
                    SPACE,
                    DID,
                    "3rev2",
                    &[RepoWrite::Update {
                        collection: POST.to_string(),
                        rkey: "missing".to_string(),
                        value: value("x"),
                        swap_record: Some("bafyreiwrong".to_string()),
                    }]
                )
                .await,
            Err(HostError::InvalidSwap)
        ));

        // A rejected batch leaves nothing behind.
        let head = store.head(SPACE, DID).await.unwrap().unwrap();
        assert_eq!(head.rev, "3rev1");
        let (records, _) = store
            .list_records(SPACE, DID, None, None, 10)
            .await
            .unwrap();
        assert_eq!(records.len(), 1);
    }

    async fn exercise_noop_delete(store: &dyn RepoStore) {
        store
            .apply_writes(SPACE, DID, "3rev1", &[create("a", "one")])
            .await
            .unwrap();
        let applied = store
            .apply_writes(SPACE, DID, "3rev2", &[delete("ghost")])
            .await
            .unwrap();
        assert_eq!(applied.outcomes, vec![WriteOutcome::Noop]);
        // A no-op neither advances the revision nor writes an oplog entry.
        assert_eq!(applied.rev, "3rev1");
        let page = store.list_ops(SPACE, DID, None, None, 10).await.unwrap();
        assert_eq!(page.ops.len(), 1);
    }

    async fn exercise_atomic_batch(store: &dyn RepoStore) {
        store
            .apply_writes(
                SPACE,
                DID,
                "3rev1",
                &[
                    create("a", "one"),
                    create("b", "two"),
                    RepoWrite::Create {
                        collection: LIKE.to_string(),
                        rkey: "l1".to_string(),
                        value: value("like"),
                    },
                ],
            )
            .await
            .unwrap();
        let page = store.list_ops(SPACE, DID, None, None, 10).await.unwrap();
        assert_eq!(page.ops.len(), 3);
        // Operations mutated atomically share a revision.
        assert!(page.ops.iter().all(|o| o.rev == "3rev1"));
        assert!(page.complete);

        // A create-then-delete of the same path within one batch nets to nothing.
        store
            .apply_writes(SPACE, DID, "3rev2", &[create("c", "three"), delete("c")])
            .await
            .unwrap();
        assert!(store
            .get_record(SPACE, DID, POST, "c")
            .await
            .unwrap()
            .is_none());
    }

    async fn exercise_listing(store: &dyn RepoStore) {
        for (i, rkey) in ["a", "b", "c"].iter().enumerate() {
            store
                .apply_writes(SPACE, DID, &format!("3rev{i}"), &[create(rkey, rkey)])
                .await
                .unwrap();
        }
        store
            .apply_writes(
                SPACE,
                DID,
                "3rev9",
                &[RepoWrite::Create {
                    collection: LIKE.to_string(),
                    rkey: "l1".to_string(),
                    value: value("like"),
                }],
            )
            .await
            .unwrap();

        let (page, cursor) = store.list_records(SPACE, DID, None, None, 2).await.unwrap();
        assert_eq!(page.len(), 2);
        assert_eq!(page[0].collection, LIKE);
        let (page, cursor2) = store
            .list_records(SPACE, DID, None, cursor.as_deref(), 2)
            .await
            .unwrap();
        assert_eq!(page.len(), 2);
        // A full final page still yields a cursor; the empty page ends paging.
        let (page, cursor3) = store
            .list_records(SPACE, DID, None, cursor2.as_deref(), 2)
            .await
            .unwrap();
        assert!(page.is_empty());
        assert!(cursor3.is_none());

        let (posts, _) = store
            .list_records(SPACE, DID, Some(POST), None, 10)
            .await
            .unwrap();
        assert_eq!(posts.len(), 3);
        assert!(posts.iter().all(|r| r.collection == POST));

        assert!(matches!(
            store
                .list_records(SPACE, "did:plc:nobody", None, None, 10)
                .await,
            Err(HostError::RepoNotFound)
        ));
    }

    async fn exercise_oplog_paging(store: &dyn RepoStore) {
        for i in 0..5 {
            store
                .apply_writes(
                    SPACE,
                    DID,
                    &format!("3rev{i}"),
                    &[create(&format!("r{i}"), "x")],
                )
                .await
                .unwrap();
        }

        let page = store.list_ops(SPACE, DID, None, None, 2).await.unwrap();
        assert_eq!(page.ops.len(), 2);
        assert!(!page.complete);
        let page2 = store
            .list_ops(SPACE, DID, None, page.cursor.as_deref(), 10)
            .await
            .unwrap();
        assert_eq!(page2.ops.len(), 3);
        assert!(page2.complete);
        assert!(page2.cursor.is_none());

        // `since` yields strictly later revisions.
        let page = store
            .list_ops(SPACE, DID, Some("3rev2"), None, 10)
            .await
            .unwrap();
        assert_eq!(
            page.ops.iter().map(|o| o.rev.as_str()).collect::<Vec<_>>(),
            vec!["3rev3", "3rev4"]
        );

        // A revision at or past the head yields an empty, complete page.
        let page = store
            .list_ops(SPACE, DID, Some("3rev4"), None, 10)
            .await
            .unwrap();
        assert!(page.ops.is_empty());

        assert!(matches!(
            store.list_ops(SPACE, DID, None, Some("abc"), 10).await,
            Err(HostError::InvalidRequest(_))
        ));
        assert!(matches!(
            store
                .list_ops(SPACE, "did:plc:nobody", None, None, 10)
                .await,
            Err(HostError::RepoNotFound)
        ));
    }

    async fn exercise_history_unavailable(store: &dyn RepoStore) {
        store
            .apply_writes(SPACE, DID, "3rev5", &[create("a", "one")])
            .await
            .unwrap();
        // `since` predates every retained operation.
        assert!(matches!(
            store.list_ops(SPACE, DID, Some("3rev1"), None, 10).await,
            Err(HostError::HistoryUnavailable)
        ));
    }

    async fn exercise_isolation_and_deletion(store: &dyn RepoStore) {
        let other_space = "at://did:plc:auth/space/community.blacksky.feed/other";
        store
            .apply_writes(SPACE, DID, "3rev1", &[create("a", "one")])
            .await
            .unwrap();
        store
            .apply_writes(other_space, DID, "3rev1", &[create("a", "other")])
            .await
            .unwrap();
        store
            .apply_writes(SPACE, "did:plc:other", "3rev1", &[create("a", "theirs")])
            .await
            .unwrap();

        store.delete_repo(SPACE, DID).await.unwrap();
        assert!(store.head(SPACE, DID).await.unwrap().is_none());
        assert!(store.head(other_space, DID).await.unwrap().is_some());
        assert!(store.head(SPACE, "did:plc:other").await.unwrap().is_some());
        // Deleting an absent repo is not an error.
        store.delete_repo(SPACE, DID).await.unwrap();
    }

    macro_rules! both_backings {
        ($name:ident, $exercise:ident) => {
            #[tokio::test]
            async fn $name() {
                $exercise(&InMemoryRepos::default()).await;
                $exercise(&SqliteRepos::open_in_memory().unwrap()).await;
            }
        };
    }

    both_backings!(write_read_cycle, exercise_write_read_cycle);
    both_backings!(swap_and_conflict, exercise_swap_and_conflict);
    both_backings!(noop_delete, exercise_noop_delete);
    both_backings!(atomic_batch, exercise_atomic_batch);
    both_backings!(listing, exercise_listing);
    both_backings!(oplog_paging, exercise_oplog_paging);
    both_backings!(history_unavailable, exercise_history_unavailable);
    both_backings!(isolation_and_deletion, exercise_isolation_and_deletion);

    #[tokio::test]
    async fn digest_is_order_independent_across_repos() {
        let a = InMemoryRepos::default();
        a.apply_writes(SPACE, DID, "3rev1", &[create("x", "one")])
            .await
            .unwrap();
        a.apply_writes(SPACE, DID, "3rev2", &[create("y", "two")])
            .await
            .unwrap();

        let b = SqliteRepos::open_in_memory().unwrap();
        b.apply_writes(SPACE, DID, "3rev1", &[create("y", "two")])
            .await
            .unwrap();
        b.apply_writes(SPACE, DID, "3rev2", &[create("x", "one")])
            .await
            .unwrap();

        assert_eq!(
            a.head(SPACE, DID).await.unwrap().unwrap().hash(),
            b.head(SPACE, DID).await.unwrap().unwrap().hash()
        );
    }

    #[tokio::test]
    async fn sqlite_persists_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("repos.db");
        let cid = {
            let store = SqliteRepos::open(&path).unwrap();
            let applied = store
                .apply_writes(SPACE, DID, "3rev1", &[create("a", "one")])
                .await
                .unwrap();
            match &applied.outcomes[0] {
                WriteOutcome::Created { cid } => cid.clone(),
                other => panic!("unexpected outcome {other:?}"),
            }
        };
        let store = SqliteRepos::open(&path).unwrap();
        let got = store
            .get_record(SPACE, DID, POST, "a")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got.cid, cid);
        assert_eq!(store.head(SPACE, DID).await.unwrap().unwrap().rev, "3rev1");

        assert!(matches!(
            SqliteRepos::open(dir.path().join("missing/nested.db")),
            Err(HostError::Store(_))
        ));
    }

    #[test]
    fn history_window_and_cursor_edges() {
        assert!(ensure_history(None, None).is_ok());
        assert!(ensure_history(Some("3rev1"), Some("3rev1")).is_ok());
        assert!(matches!(
            ensure_history(Some("3rev1"), None),
            Err(HostError::HistoryUnavailable)
        ));
        assert_eq!(parse_cursor(None).unwrap(), None);
        assert_eq!(parse_cursor(Some("7")).unwrap(), Some(7));
    }
}
