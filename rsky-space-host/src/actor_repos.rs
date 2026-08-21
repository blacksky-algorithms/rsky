//! [`RepoStore`] over a directory of per-account actor stores.
//!
//! One `store.sqlite` per author DID, laid out as the PDS lays out its actor
//! files, carrying the `space_*` tables of [`crate::actor_schema`]. Revisions
//! are minted here rather than supplied by the caller, so a repo's `rev` is
//! always a TID monotonic in that repo's own history.

use async_trait::async_trait;
use rsky_common::tid::TID;
use rsky_space::lthash::{element, LtHash};
use rsky_space::record::dag_cbor_cid;
use rsky_space::space_id::SpaceId;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::error::{HostError, Result};
use crate::repo::{
    page_cursor, parse_cursor, record_path, Applied, OpPage, RepoHead, RepoStore, RepoWrite,
    StoredOp, StoredRecord, WriteOutcome, STATE_BYTES,
};

/// Oplog rows retained per repo before the oldest revisions are dropped.
pub const DEFAULT_OPLOG_WINDOW: usize = 10_000;

pub struct ActorStoreRepos {
    directory: PathBuf,
    oplog_window: usize,
    connections: Mutex<HashMap<String, Connection>>,
}

impl ActorStoreRepos {
    pub fn open(directory: impl Into<PathBuf>) -> Result<Self> {
        Self::with_oplog_window(directory, DEFAULT_OPLOG_WINDOW)
    }

    pub fn with_oplog_window(directory: impl Into<PathBuf>, oplog_window: usize) -> Result<Self> {
        Ok(Self {
            directory: directory.into(),
            oplog_window: oplog_window.max(1),
            connections: Mutex::new(HashMap::new()),
        })
    }

    /// `{root}/{sha256(did)[..2]}/{did}/store.sqlite`, the PDS actor layout.
    pub fn store_path(&self, did: &str) -> Result<PathBuf> {
        if did.is_empty()
            || !did.starts_with("did:")
            || did.contains('/')
            || did.contains('\\')
            || did.contains("..")
        {
            return Err(HostError::InvalidRequest(format!("unusable did: {did}")));
        }
        let digest = hex::encode(Sha256::digest(did.as_bytes()));
        Ok(self
            .directory
            .join(&digest[..2])
            .join(did)
            .join("store.sqlite"))
    }

    fn with_store<T>(
        &self,
        did: &str,
        act: impl FnOnce(&mut Connection) -> Result<T>,
    ) -> Result<T> {
        let mut connections = self.connections.lock().unwrap();
        if !connections.contains_key(did) {
            let path = self.store_path(did)?;
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|error| store(error.to_string()))?;
            }
            connections.insert(did.to_string(), crate::actor_schema::get_migrated_db(path)?);
        }
        act(connections.get_mut(did).expect("store just opened"))
    }
}

#[async_trait]
impl RepoStore for ActorStoreRepos {
    /// `rev` is ignored: the store mints the revision every write in the batch
    /// shares, and returns it on [`Applied`].
    async fn apply_writes(
        &self,
        space_uri: &str,
        did: &str,
        _rev: &str,
        writes: &[RepoWrite],
    ) -> Result<Applied> {
        let space = SpaceId::parse(space_uri)?;
        let window = self.oplog_window;
        self.with_store(did, move |conn| {
            let tx = conn.transaction().map_err(sql)?;
            let applied = apply_writes_tx(&tx, &space, writes, window)?;
            tx.commit().map_err(sql)?;
            Ok(applied)
        })
    }

    async fn head(&self, space_uri: &str, did: &str) -> Result<Option<RepoHead>> {
        self.with_store(did, |conn| {
            let Some((rev, state)) = live_repo(conn, space_uri)? else {
                return Ok(None);
            };
            Ok(Some(RepoHead {
                rev,
                state: state_bytes(state)?,
            }))
        })
    }

    async fn get_record(
        &self,
        space_uri: &str,
        did: &str,
        collection: &str,
        rkey: &str,
    ) -> Result<Option<StoredRecord>> {
        self.with_store(did, |conn| {
            conn.query_row(
                "SELECT collection, rkey, cid, value FROM space_record \
                 WHERE space_uri = ?1 AND collection = ?2 AND rkey = ?3",
                params![space_uri, collection, rkey],
                row_to_record,
            )
            .optional()
            .map_err(sql)
        })
    }

    async fn list_records(
        &self,
        space_uri: &str,
        did: &str,
        collection: Option<&str>,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<(Vec<StoredRecord>, Option<String>)> {
        self.with_store(did, |conn| {
            if live_repo(conn, space_uri)?.is_none() {
                return Err(HostError::RepoNotFound);
            }
            let cursor = cursor.and_then(|c| c.split_once('/'));
            let mut query = String::from(
                "SELECT collection, rkey, cid, value FROM space_record WHERE space_uri = ?1",
            );
            let mut args: Vec<&dyn rusqlite::ToSql> = vec![&space_uri];
            if let Some(ref collection) = collection {
                query.push_str(&format!(" AND collection = ?{}", args.len() + 1));
                args.push(collection);
            }
            if let Some((ref c, ref r)) = cursor {
                let base = args.len();
                query.push_str(&format!(
                    " AND (collection > ?{0} OR (collection = ?{0} AND rkey > ?{1}))",
                    base + 1,
                    base + 2
                ));
                args.push(c);
                args.push(r);
            }
            query.push_str(&format!(" ORDER BY collection, rkey LIMIT {limit}"));
            let mut statement = conn.prepare(&query).map_err(sql)?;
            let page = statement
                .query_map(rusqlite::params_from_iter(args), row_to_record)
                .map_err(sql)?
                .collect::<rusqlite::Result<Vec<StoredRecord>>>()
                .map_err(sql)?;
            let cursor = page_cursor(&page, limit, |r| r.path());
            Ok((page, cursor))
        })
    }

    async fn list_ops(
        &self,
        space_uri: &str,
        did: &str,
        since: Option<&str>,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<OpPage> {
        let after = parse_cursor(cursor)?;
        self.with_store(did, |conn| {
            let floor: Option<String> = conn
                .query_row(
                    "SELECT oplog_floor_rev FROM space_repo WHERE space_uri = ?1 AND deleted = 0",
                    [space_uri],
                    |row| row.get(0),
                )
                .optional()
                .map_err(sql)?
                .ok_or(HostError::RepoNotFound)?;
            if let Some(floor) = floor {
                match since {
                    Some(since) if since >= floor.as_str() => {}
                    _ => return Err(HostError::HistoryUnavailable),
                }
            }
            let mut statement = conn
                .prepare(
                    "SELECT id, rev, collection, rkey, cid, prev FROM space_oplog \
                     WHERE space_uri = ?1 AND (?2 IS NULL OR rev > ?2) \
                       AND (?3 IS NULL OR id > ?3) \
                     ORDER BY id LIMIT ?4",
                )
                .map_err(sql)?;
            let mut ops = statement
                .query_map(params![space_uri, since, after, limit as i64 + 1], |row| {
                    Ok(StoredOp {
                        seq: row.get(0)?,
                        rev: row.get(1)?,
                        collection: row.get(2)?,
                        rkey: row.get(3)?,
                        cid: row.get(4)?,
                        prev: row.get(5)?,
                    })
                })
                .map_err(sql)?
                .collect::<rusqlite::Result<Vec<StoredOp>>>()
                .map_err(sql)?;
            let complete = ops.len() <= limit as usize;
            ops.truncate(limit as usize);
            Ok(OpPage {
                cursor: if complete {
                    None
                } else {
                    page_cursor(&ops, limit, |o| o.seq.to_string())
                },
                complete,
                ops,
            })
        })
    }

    async fn delete_repo(&self, space_uri: &str, did: &str) -> Result<()> {
        self.with_store(did, |conn| {
            conn.execute(
                "UPDATE space_repo SET deleted = 1 WHERE space_uri = ?1",
                [space_uri],
            )
            .map_err(sql)?;
            Ok(())
        })
    }
}

/// Apply one batch: evolve the LtHash state, upsert/delete record rows, append
/// oplog rows sharing one fresh revision, then compact the oplog.
fn apply_writes_tx(
    tx: &Transaction,
    space: &SpaceId,
    writes: &[RepoWrite],
    window: usize,
) -> Result<Applied> {
    let space_uri = space.uri();
    let existing: Option<(String, Vec<u8>, i64)> = tx
        .query_row(
            "SELECT rev, lthash_state, deleted FROM space_repo WHERE space_uri = ?1",
            [&space_uri],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(sql)?;
    let (prev_rev, mut lthash) = match existing {
        Some((_, _, deleted)) if deleted != 0 => return Err(HostError::RepoNotFound),
        Some((rev, state, _)) => (Some(rev), LtHash::from_state_bytes(&state_bytes(state)?)),
        None => (None, LtHash::new()),
    };
    let rev = TID::next_str(prev_rev.clone()).map_err(|error| store(error.to_string()))?;
    let mut outcomes = Vec::with_capacity(writes.len());

    for write in writes {
        let (collection, rkey) = (write.collection(), write.rkey());
        let current: Option<String> = tx
            .query_row(
                "SELECT cid FROM space_record WHERE space_uri = ?1 AND collection = ?2 AND rkey = ?3",
                params![&space_uri, collection, rkey],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql)?;
        let swap = match write {
            RepoWrite::Create { .. } => None,
            RepoWrite::Update { swap_record, .. } | RepoWrite::Delete { swap_record, .. } => {
                Some(swap_record)
            }
        };
        if let Some(swap) = swap {
            if swap.is_some() && swap.as_deref() != current.as_deref() {
                return Err(HostError::InvalidSwap);
            }
        }
        let path = record_path(collection, rkey);
        match write {
            RepoWrite::Create { .. } if current.is_some() => {
                return Err(HostError::RecordExists(path))
            }
            RepoWrite::Update { .. } | RepoWrite::Delete { .. } if current.is_none() => {
                return Err(HostError::RecordNotFound(path))
            }
            _ => {}
        }
        if let Some(ref old_cid) = current {
            lthash.remove(&element(collection, rkey, old_cid));
            tx.execute(
                "DELETE FROM space_blob_ref WHERE space_uri = ?1 AND collection = ?2 AND rkey = ?3",
                params![&space_uri, collection, rkey],
            )
            .map_err(sql)?;
        }
        let new = match write {
            RepoWrite::Create { value, .. } | RepoWrite::Update { value, .. } => {
                Some((dag_cbor_cid(value).to_string(), value))
            }
            RepoWrite::Delete { .. } => None,
        };
        match new {
            Some((ref cid, value)) => {
                lthash.add(&element(collection, rkey, cid));
                tx.execute(
                    "INSERT INTO space_record (space_uri, collection, rkey, cid, rev, value) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
                     ON CONFLICT (space_uri, collection, rkey) \
                     DO UPDATE SET cid = excluded.cid, rev = excluded.rev, value = excluded.value",
                    params![&space_uri, collection, rkey, cid, rev, value],
                )
                .map_err(sql)?;
            }
            None => {
                tx.execute(
                    "DELETE FROM space_record WHERE space_uri = ?1 AND collection = ?2 AND rkey = ?3",
                    params![&space_uri, collection, rkey],
                )
                .map_err(sql)?;
            }
        }
        let cid = new.map(|(cid, _)| cid);
        tx.execute(
            "INSERT INTO space_oplog (space_uri, rev, collection, rkey, cid, prev) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![&space_uri, rev, collection, rkey, cid, current],
        )
        .map_err(sql)?;
        outcomes.push(match (cid, current) {
            (Some(cid), None) => WriteOutcome::Created { cid },
            (Some(cid), Some(_)) => WriteOutcome::Updated { cid },
            (None, _) => WriteOutcome::Deleted,
        });
    }

    let state = lthash.state_bytes().to_vec();
    if prev_rev.is_some() {
        tx.execute(
            "UPDATE space_repo SET rev = ?2, lthash_state = ?3 WHERE space_uri = ?1",
            params![&space_uri, rev, state],
        )
        .map_err(sql)?;
    } else {
        tx.execute(
            "INSERT INTO space_repo \
             (space_uri, authority, space_type, skey, rev, lthash_state, oplog_floor_rev, deleted, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, 0, ?7)",
            params![
                &space_uri,
                space.authority,
                space.space_type,
                space.skey,
                rev,
                state,
                rsky_common::now()
            ],
        )
        .map_err(sql)?;
    }
    compact_oplog(tx, &space_uri, window)?;
    Ok(Applied {
        rev,
        hash: lthash.hash(),
        outcomes,
    })
}

/// Keep at most `window` oplog rows per repo, dropping whole revisions from the
/// oldest end and advancing `oplog_floor_rev` to the newest dropped revision.
fn compact_oplog(tx: &Transaction, space_uri: &str, window: usize) -> Result<()> {
    let cutoff_rev: Option<String> = tx
        .query_row(
            "SELECT rev FROM space_oplog WHERE space_uri = ?1 ORDER BY id DESC LIMIT 1 OFFSET ?2",
            params![space_uri, (window - 1) as i64],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql)?;
    let Some(cutoff_rev) = cutoff_rev else {
        return Ok(());
    };
    let floor: Option<String> = tx
        .query_row(
            "SELECT MAX(rev) FROM space_oplog WHERE space_uri = ?1 AND rev < ?2",
            params![space_uri, cutoff_rev],
            |row| row.get(0),
        )
        .map_err(sql)?;
    let Some(floor) = floor else {
        return Ok(());
    };
    tx.execute(
        "DELETE FROM space_oplog WHERE space_uri = ?1 AND rev < ?2",
        params![space_uri, cutoff_rev],
    )
    .map_err(sql)?;
    tx.execute(
        "UPDATE space_repo SET oplog_floor_rev = ?2 WHERE space_uri = ?1",
        params![space_uri, floor],
    )
    .map_err(sql)?;
    Ok(())
}

fn live_repo(conn: &Connection, space_uri: &str) -> Result<Option<(String, Vec<u8>)>> {
    conn.query_row(
        "SELECT rev, lthash_state FROM space_repo WHERE space_uri = ?1 AND deleted = 0",
        [space_uri],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
    .map_err(sql)
}

fn row_to_record(row: &rusqlite::Row) -> rusqlite::Result<StoredRecord> {
    Ok(StoredRecord {
        collection: row.get(0)?,
        rkey: row.get(1)?,
        cid: row.get(2)?,
        value: row.get(3)?,
    })
}

fn state_bytes(state: Vec<u8>) -> Result<[u8; STATE_BYTES]> {
    state
        .try_into()
        .map_err(|_| store("lthash state is not 2048 bytes".to_string()))
}

fn store(message: String) -> HostError {
    HostError::Store(message)
}

fn sql(error: rusqlite::Error) -> HostError {
    HostError::Store(error.to_string())
}
