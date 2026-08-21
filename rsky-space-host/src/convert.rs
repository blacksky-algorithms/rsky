//! One-off conversion of a deployed multi-tenant store into per-account
//! actor stores.
//!
//! The source keeps every repo in one file with `did` as a column; the
//! destination is one `store.sqlite` per author DID. Two things are carried
//! across verbatim rather than recomputed: the LtHash state, because it is the
//! commit digest readers have already seen, and the operation-log row ids,
//! because they are the cursors syncers hold — a syncer must resume across the
//! swap without replaying or skipping.
//!
//! The source has no per-record revision. A record's revision is taken from
//! the newest operation that left it at its current CID, falling back to the
//! repo's revision when the log no longer reaches back that far.

use rsky_space::space_id::SpaceId;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use std::collections::HashMap;
use std::path::Path;

use crate::error::{HostError, Result};

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Converted {
    pub accounts: usize,
    pub repos: usize,
    pub records: usize,
    pub ops: usize,
}

pub fn convert(legacy: &Path, directory: &Path) -> Result<Converted> {
    let source =
        Connection::open_with_flags(legacy, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(sql)?;
    let mut totals = Converted::default();

    for did in accounts(&source)? {
        let path = crate::actor_repos::store_path(directory, &did)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| HostError::Store(error.to_string()))?;
        }
        let mut target = crate::actor_schema::get_migrated_db(&path)?;
        let tx = target.transaction().map_err(sql)?;
        for (space_uri, rev, state) in repos(&source, &did)? {
            let space = SpaceId::parse(&space_uri)?;
            if state.len() != 2048 {
                return Err(HostError::Store(format!(
                    "lthash state for {space_uri}/{did} is not 2048 bytes"
                )));
            }
            tx.execute(
                "INSERT INTO space_repo \
                 (space_uri, authority, space_type, skey, rev, lthash_state, oplog_floor_rev, deleted, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, 0, ?7)",
                params![
                    space_uri,
                    space.authority,
                    space.space_type,
                    space.skey,
                    rev,
                    state,
                    rsky_common::now()
                ],
            )
            .map_err(sql)?;

            let mut revisions = HashMap::new();
            for (seq, op_rev, collection, rkey, cid, prev) in ops(&source, &space_uri, &did)? {
                if let Some(ref cid) = cid {
                    revisions.insert(
                        (collection.clone(), rkey.clone(), cid.clone()),
                        op_rev.clone(),
                    );
                }
                tx.execute(
                    "INSERT INTO space_oplog (id, space_uri, rev, collection, rkey, cid, prev) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![seq, space_uri, op_rev, collection, rkey, cid, prev],
                )
                .map_err(sql)?;
                totals.ops += 1;
            }

            for (collection, rkey, cid, value) in records(&source, &space_uri, &did)? {
                let record_rev = revisions
                    .get(&(collection.clone(), rkey.clone(), cid.clone()))
                    .unwrap_or(&rev);
                tx.execute(
                    "INSERT INTO space_record (space_uri, collection, rkey, cid, rev, value) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![space_uri, collection, rkey, cid, record_rev, value],
                )
                .map_err(sql)?;
                totals.records += 1;
            }
            totals.repos += 1;
        }
        tx.commit().map_err(sql)?;
        totals.accounts += 1;
    }
    Ok(totals)
}

/// The oplog id a syncer holding `cursor` before the conversion resumes from
/// after it. Ids are preserved, so the cursor is unchanged — the lookup exists
/// to prove that, and fails loudly if a converted store ever renumbers.
pub fn resumes_at(store: &Path, space_uri: &str, cursor: i64) -> Result<Option<i64>> {
    let conn = Connection::open_with_flags(store, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(sql)?;
    conn.query_row(
        "SELECT MIN(id) FROM space_oplog WHERE space_uri = ?1 AND id > ?2",
        params![space_uri, cursor],
        |row| row.get(0),
    )
    .optional()
    .map(Option::flatten)
    .map_err(sql)
}

fn accounts(source: &Connection) -> Result<Vec<String>> {
    let mut statement = source
        .prepare("SELECT DISTINCT did FROM repo ORDER BY did")
        .map_err(sql)?;
    let rows = statement
        .query_map([], |row| row.get(0))
        .map_err(sql)?
        .collect::<rusqlite::Result<Vec<String>>>()
        .map_err(sql)?;
    Ok(rows)
}

type LegacyRepo = (String, String, Vec<u8>);

fn repos(source: &Connection, did: &str) -> Result<Vec<LegacyRepo>> {
    let mut statement = source
        .prepare("SELECT space_uri, rev, state FROM repo WHERE did = ?1 ORDER BY space_uri")
        .map_err(sql)?;
    let rows = statement
        .query_map([did], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .map_err(sql)?
        .collect::<rusqlite::Result<Vec<LegacyRepo>>>()
        .map_err(sql)?;
    Ok(rows)
}

type LegacyRecord = (String, String, String, Vec<u8>);

fn records(source: &Connection, space_uri: &str, did: &str) -> Result<Vec<LegacyRecord>> {
    let mut statement = source
        .prepare(
            "SELECT collection, rkey, cid, value FROM record \
             WHERE space_uri = ?1 AND did = ?2 ORDER BY collection, rkey",
        )
        .map_err(sql)?;
    let rows = statement
        .query_map(params![space_uri, did], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .map_err(sql)?
        .collect::<rusqlite::Result<Vec<LegacyRecord>>>()
        .map_err(sql)?;
    Ok(rows)
}

type LegacyOp = (i64, String, String, String, Option<String>, Option<String>);

fn ops(source: &Connection, space_uri: &str, did: &str) -> Result<Vec<LegacyOp>> {
    let mut statement = source
        .prepare(
            "SELECT seq, rev, collection, rkey, cid, prev FROM repo_op \
             WHERE space_uri = ?1 AND did = ?2 ORDER BY seq",
        )
        .map_err(sql)?;
    let rows = statement
        .query_map(params![space_uri, did], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })
        .map_err(sql)?
        .collect::<rusqlite::Result<Vec<LegacyOp>>>()
        .map_err(sql)?;
    Ok(rows)
}

fn sql(error: rusqlite::Error) -> HostError {
    HostError::Store(error.to_string())
}
