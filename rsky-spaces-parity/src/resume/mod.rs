//! Support code for the resume-across-swap gate: the projection sink the
//! daemon is pointed at, and readers for the three durable artefacts the gate
//! reasons about — the daemon's index, the legacy multi-tenant store, and a
//! converted per-account store.

pub mod sink;

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OpenFlags};
use std::collections::BTreeMap;
use std::path::Path;

/// One oplog row, in the form both storage eras can be read into.
pub type OpRow = (i64, String, String, String, Option<String>);

/// What the daemon durably knows about one repo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexSnapshot {
    /// The syncer's cursor: `since` on the next `listRepoOps`.
    pub head_rev: Option<String>,
    pub lthash_state: Option<Vec<u8>>,
    /// `collection/rkey -> (cid, rev, value)`.
    pub records: BTreeMap<String, (String, String, Option<Vec<u8>>)>,
    /// Journalled batches, by revision, with the mutation count each carried.
    pub journal: BTreeMap<String, usize>,
    /// `projector -> rev`: how far each projection has been confirmed.
    pub projector_cursors: BTreeMap<String, String>,
}

/// Read the daemon's index for one `(space, did)`. The daemon runs SQLite in
/// WAL mode, so this opens read-write to let the reader replay the log of a
/// process that was killed rather than shut down.
pub fn read_index(path: &Path, space: &str, did: &str) -> Result<IndexSnapshot> {
    let conn = Connection::open(path).with_context(|| format!("open {}", path.display()))?;
    let head: Option<(String, Vec<u8>)> = conn
        .query_row(
            "SELECT rev, lthash_state FROM sync_state WHERE space_uri = ?1 AND did = ?2",
            params![space, did],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok();
    let mut records = BTreeMap::new();
    {
        let mut statement = conn.prepare(
            "SELECT collection, rkey, cid, rev, value FROM record \
             WHERE space_uri = ?1 AND did = ?2 ORDER BY collection, rkey",
        )?;
        let mut rows = statement.query(params![space, did])?;
        while let Some(row) = rows.next()? {
            let collection: String = row.get(0)?;
            let rkey: String = row.get(1)?;
            records.insert(
                format!("{collection}/{rkey}"),
                (row.get(2)?, row.get(3)?, row.get(4)?),
            );
        }
    }
    let mut journal = BTreeMap::new();
    {
        let mut statement = conn.prepare(
            "SELECT rev, mutations FROM projection_journal \
             WHERE space_uri = ?1 AND did = ?2 ORDER BY rev",
        )?;
        let mut rows = statement.query(params![space, did])?;
        while let Some(row) = rows.next()? {
            let rev: String = row.get(0)?;
            let mutations: Vec<u8> = row.get(1)?;
            let count = serde_json::from_slice::<serde_json::Value>(&mutations)
                .ok()
                .and_then(|value| value.as_array().map(Vec::len))
                .unwrap_or(0);
            journal.insert(rev, count);
        }
    }
    let mut projector_cursors = BTreeMap::new();
    {
        let mut statement = conn.prepare(
            "SELECT projector, rev FROM projector_cursor \
             WHERE space_uri = ?1 AND did = ?2 ORDER BY projector",
        )?;
        let mut rows = statement.query(params![space, did])?;
        while let Some(row) = rows.next()? {
            projector_cursors.insert(row.get(0)?, row.get(1)?);
        }
    }
    Ok(IndexSnapshot {
        head_rev: head.as_ref().map(|(rev, _)| rev.clone()),
        lthash_state: head.map(|(_, state)| state),
        records,
        journal,
        projector_cursors,
    })
}

/// The legacy multi-tenant oplog for one `(space, did)`, in row order.
pub fn legacy_oplog(db: &Path, space: &str, did: &str) -> Result<Vec<OpRow>> {
    let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("open {}", db.display()))?;
    let mut statement = conn.prepare(
        "SELECT seq, rev, collection, rkey, cid FROM repo_op \
         WHERE space_uri = ?1 AND did = ?2 ORDER BY seq",
    )?;
    let mut rows = statement.query(params![space, did])?;
    let mut ops = Vec::new();
    while let Some(row) = rows.next()? {
        ops.push((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
        ));
    }
    Ok(ops)
}

/// The converged per-account oplog for one space, in row order.
pub fn converged_oplog(store: &Path, space: &str) -> Result<Vec<OpRow>> {
    let conn = Connection::open_with_flags(store, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("open {}", store.display()))?;
    let mut statement = conn.prepare(
        "SELECT id, rev, collection, rkey, cid FROM space_oplog \
         WHERE space_uri = ?1 ORDER BY id",
    )?;
    let mut rows = statement.query(params![space])?;
    let mut ops = Vec::new();
    while let Some(row) = rows.next()? {
        ops.push((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
        ));
    }
    Ok(ops)
}

/// `(rev, oplog_floor_rev)` for a converged repo. A floor of `None` is the
/// state in which no `since` can be refused as outside the oplog window.
pub fn converged_head(store: &Path, space: &str) -> Result<(String, Option<String>)> {
    let conn = Connection::open_with_flags(store, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("open {}", store.display()))?;
    conn.query_row(
        "SELECT rev, oplog_floor_rev FROM space_repo WHERE space_uri = ?1 AND deleted = 0",
        params![space],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .with_context(|| format!("no live space_repo row for {space}"))
}

/// Labels of a projected batch list, in arrival order.
pub fn labels(ops: &[sink::Projected]) -> Vec<String> {
    ops.iter().map(sink::Projected::label).collect()
}

/// Labels that appear more than once across `ops` — a duplicated projection.
pub fn duplicates(ops: &[sink::Projected]) -> Vec<String> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for op in ops {
        *counts.entry(op.label()).or_default() += 1;
    }
    counts
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(label, count)| format!("{label} x{count}"))
        .collect()
}

/// The final projected state per record path, which is what a destination
/// holds once a batch list has been applied in order.
pub fn final_state(ops: &[sink::Projected]) -> BTreeMap<String, String> {
    let mut state = BTreeMap::new();
    for op in ops {
        state.insert(op.path(), op.operation.clone());
    }
    state
}

/// Lines of `log` that name a sync failure, cursor refusal, or full-state
/// recovery. Any of them means the resume was not clean.
pub fn unclean_resume_lines(log: &str) -> Vec<String> {
    const MARKERS: [&str; 5] = [
        "HistoryUnavailable",
        "full-state recovery",
        "diverged",
        "sweep failed",
        "prev does not match",
    ];
    log.lines()
        .filter(|line| {
            let lowered = line.to_lowercase();
            MARKERS
                .iter()
                .any(|marker| lowered.contains(&marker.to_lowercase()))
        })
        .map(str::to_string)
        .collect()
}
