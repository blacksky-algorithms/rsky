//! Permissioned-repo storage over the per-actor sqlite store
//! (permissioned-data proposal: one repo per (user, space), summarized by an
//! LtHash set digest, with a compactable operation log).

use crate::actor_store::db::ActorDb;
use anyhow::Result;
use rsky_common::env::env_int;
use rsky_common::ipld::cid_for_cbor;
use rsky_common::tid::TID;
use rsky_space::lthash::{element, LtHash};
use rsky_space::space_id::SpaceId;
use rusqlite::{params, OptionalExtension, Transaction};
use serde_json::Value as JsonValue;
use thiserror::Error;

pub mod commit;

pub const DEFAULT_OPLOG_WINDOW: usize = 10_000;

/// Oplog retention window (rows per space), from `PDS_SPACE_OPLOG_WINDOW`.
pub fn oplog_window() -> usize {
    env_int("PDS_SPACE_OPLOG_WINDOW")
        .unwrap_or(DEFAULT_OPLOG_WINDOW)
        .max(1)
}

#[derive(Error, Debug, PartialEq, Eq)]
pub enum SpaceStoreError {
    #[error("SpaceNotFound: no repo for `{0}`")]
    SpaceNotFound(String),
    #[error("SpaceDeleted: `{0}` belongs to a deleted space")]
    SpaceDeleted(String),
    #[error("RecordExists: `{0}`")]
    RecordExists(String),
    #[error("RecordNotFound: `{0}`")]
    RecordNotFound(String),
    #[error("HistoryUnavailable: oplog compacted past the requested revision")]
    HistoryUnavailable,
    #[error("InvalidSwap: expected `{0:?}`")]
    InvalidSwap(Option<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum SpaceWrite {
    Create {
        collection: String,
        rkey: String,
        value: JsonValue,
    },
    Update {
        collection: String,
        rkey: String,
        value: JsonValue,
        swap_cid: Option<String>,
    },
    Delete {
        collection: String,
        rkey: String,
        swap_cid: Option<String>,
    },
}

impl SpaceWrite {
    fn collection(&self) -> &str {
        match self {
            SpaceWrite::Create { collection, .. }
            | SpaceWrite::Update { collection, .. }
            | SpaceWrite::Delete { collection, .. } => collection,
        }
    }

    fn rkey(&self) -> &str {
        match self {
            SpaceWrite::Create { rkey, .. }
            | SpaceWrite::Update { rkey, .. }
            | SpaceWrite::Delete { rkey, .. } => rkey,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceWriteResult {
    pub collection: String,
    pub rkey: String,
    /// New record cid; `None` for a delete.
    pub cid: Option<String>,
    /// Previous record cid; `None` for a create.
    pub prev: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceCommitResult {
    pub rev: String,
    pub hash: [u8; 32],
    pub results: Vec<SpaceWriteResult>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceRepoState {
    pub space_uri: String,
    pub authority: String,
    pub rev: String,
    pub lthash_state: Vec<u8>,
    pub oplog_floor_rev: Option<String>,
    pub deleted: bool,
}

impl SpaceRepoState {
    pub fn hash(&self) -> [u8; 32] {
        let mut state = [0u8; 2048];
        state.copy_from_slice(&self.lthash_state);
        LtHash::from_state_bytes(&state).hash()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceRecordRow {
    pub collection: String,
    pub rkey: String,
    pub cid: String,
    pub rev: String,
    pub value: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceOplogRow {
    pub id: i64,
    pub rev: String,
    pub collection: String,
    pub rkey: String,
    pub cid: Option<String>,
    pub prev: Option<String>,
    /// Current record bytes, inlined only when this op's cid is still current.
    pub value: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceDefRow {
    pub space_uri: String,
    pub space_type: String,
    pub skey: String,
    pub policy: String,
    pub app_access: String,
    pub allowed_clients: Option<Vec<String>>,
    pub managing_app: Option<String>,
    pub deleted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceWriterRow {
    pub did: String,
    pub rev: String,
    pub hash: Option<String>,
}

/// Encode a record's JSON value to the DAG-CBOR bytes stored and served for it,
/// plus the CID computed over those bytes. Space records are stored exactly as
/// their JSON arrived; the same bytes back the CID, the LtHash element, and the
/// CAR serialization, so every verification path sees one canonical encoding.
pub fn encode_record(value: &JsonValue) -> Result<(String, Vec<u8>)> {
    let bytes = rsky_common::struct_to_cbor(value)?;
    let cid = cid_for_cbor(value)?;
    Ok((cid.to_string(), bytes))
}

pub fn decode_record(bytes: &[u8]) -> Result<JsonValue> {
    Ok(serde_ipld_dagcbor::from_slice(bytes)?)
}

/// Collect the blob CIDs referenced by a record value: typed refs
/// (`{"$type":"blob","ref":{"$link":cid}}`) and legacy untyped refs
/// (`{"cid":...,"mimeType":...}`).
pub fn blob_cids_in_record(value: &JsonValue) -> Vec<String> {
    fn walk(value: &JsonValue, out: &mut Vec<String>) {
        match value {
            JsonValue::Object(map) => {
                let typed = map.get("$type").and_then(JsonValue::as_str) == Some("blob");
                if typed {
                    if let Some(link) = map
                        .get("ref")
                        .and_then(|r| r.get("$link"))
                        .and_then(JsonValue::as_str)
                    {
                        out.push(link.to_string());
                    }
                } else if let (Some(cid), Some(_)) = (
                    map.get("cid").and_then(JsonValue::as_str),
                    map.get("mimeType").and_then(JsonValue::as_str),
                ) {
                    out.push(cid.to_string());
                }
                for child in map.values() {
                    walk(child, out);
                }
            }
            JsonValue::Array(items) => {
                for child in items {
                    walk(child, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(value, &mut out);
    out.sort();
    out.dedup();
    out
}

/// Reader/writer for one actor's permissioned repos and (when the actor is a
/// space authority) its space definitions.
#[derive(Clone)]
pub struct SpaceStore {
    pub did: String,
    db: ActorDb,
}

struct PreparedSpaceWrite {
    collection: String,
    rkey: String,
    /// `(cid, cbor bytes, blob cids)` for create/update, `None` for delete.
    new: Option<(String, Vec<u8>, Vec<String>)>,
    action: PreparedAction,
    swap_cid: Option<Option<String>>,
}

#[derive(Clone, Copy, PartialEq)]
enum PreparedAction {
    Create,
    Update,
    Delete,
}

impl SpaceStore {
    pub fn new(did: String, db: ActorDb) -> Self {
        SpaceStore { did, db }
    }

    /// Apply a batch of writes to one space's repo in a single transaction:
    /// evolve the LtHash state, upsert/delete record rows, append oplog rows
    /// sharing one fresh rev, track blob refs, and compact the oplog beyond
    /// `window` rows.
    pub async fn apply_writes(
        &self,
        space: &SpaceId,
        writes: Vec<SpaceWrite>,
        window: usize,
    ) -> Result<SpaceCommitResult> {
        let prepared = writes
            .into_iter()
            .map(|write| {
                let (action, new, swap_cid) = match &write {
                    SpaceWrite::Create { value, .. } => (PreparedAction::Create, Some(value), None),
                    SpaceWrite::Update {
                        value, swap_cid, ..
                    } => (PreparedAction::Update, Some(value), Some(swap_cid.clone())),
                    SpaceWrite::Delete { swap_cid, .. } => {
                        (PreparedAction::Delete, None, Some(swap_cid.clone()))
                    }
                };
                let new = match new {
                    Some(value) => {
                        let (cid, bytes) = encode_record(value)?;
                        Some((cid, bytes, blob_cids_in_record(value)))
                    }
                    None => None,
                };
                Ok(PreparedSpaceWrite {
                    collection: write.collection().to_string(),
                    rkey: write.rkey().to_string(),
                    new,
                    action,
                    swap_cid,
                })
            })
            .collect::<Result<Vec<PreparedSpaceWrite>>>()?;
        let space_uri = space.uri();
        let authority = space.authority.clone();
        let space_type = space.space_type.clone();
        let skey = space.skey.clone();
        self.db
            .tx(move |tx| {
                apply_writes_tx(
                    tx,
                    &space_uri,
                    &authority,
                    &space_type,
                    &skey,
                    &prepared,
                    window,
                )
            })
            .await
    }

    pub async fn repo_state(&self, space_uri: &str) -> Result<Option<SpaceRepoState>> {
        let space_uri = space_uri.to_string();
        self.db
            .run(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT space_uri, authority, rev, lthash_state, oplog_floor_rev, deleted \
                         FROM space_repo WHERE space_uri = ?1",
                        [&space_uri],
                        |row| {
                            Ok(SpaceRepoState {
                                space_uri: row.get(0)?,
                                authority: row.get(1)?,
                                rev: row.get(2)?,
                                lthash_state: row.get(3)?,
                                oplog_floor_rev: row.get(4)?,
                                deleted: row.get::<_, i64>(5)? != 0,
                            })
                        },
                    )
                    .optional()?)
            })
            .await
    }

    /// Repo state, erroring on missing/deleted repos.
    pub async fn live_repo_state(&self, space_uri: &str) -> Result<SpaceRepoState> {
        let state = self
            .repo_state(space_uri)
            .await?
            .ok_or_else(|| SpaceStoreError::SpaceNotFound(space_uri.to_string()))?;
        if state.deleted {
            return Err(SpaceStoreError::SpaceDeleted(space_uri.to_string()).into());
        }
        Ok(state)
    }

    pub async fn get_record(
        &self,
        space_uri: &str,
        collection: &str,
        rkey: &str,
    ) -> Result<Option<SpaceRecordRow>> {
        let (space_uri, collection, rkey) = (
            space_uri.to_string(),
            collection.to_string(),
            rkey.to_string(),
        );
        self.db
            .run(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT collection, rkey, cid, rev, value FROM space_record \
                         WHERE space_uri = ?1 AND collection = ?2 AND rkey = ?3",
                        params![space_uri, collection, rkey],
                        row_to_record,
                    )
                    .optional()?)
            })
            .await
    }

    /// List records ordered by `(collection, rkey)`. The cursor is the last
    /// returned `collection/rkey` path.
    pub async fn list_records(
        &self,
        space_uri: &str,
        collection: Option<String>,
        limit: usize,
        cursor: Option<String>,
    ) -> Result<Vec<SpaceRecordRow>> {
        let space_uri = space_uri.to_string();
        let cursor = cursor
            .as_deref()
            .and_then(|c| c.split_once('/'))
            .map(|(c, r)| (c.to_string(), r.to_string()));
        self.db
            .run(move |conn| {
                let mut sql = String::from(
                    "SELECT collection, rkey, cid, rev, value FROM space_record WHERE space_uri = ?1",
                );
                let mut args: Vec<Box<dyn rusqlite::ToSql>> =
                    vec![Box::new(space_uri.clone())];
                if let Some(ref collection) = collection {
                    sql.push_str(" AND collection = ?2");
                    args.push(Box::new(collection.clone()));
                }
                if let Some((ref c, ref r)) = cursor {
                    let base = args.len();
                    sql.push_str(&format!(
                        " AND (collection > ?{0} OR (collection = ?{0} AND rkey > ?{1}))",
                        base + 1,
                        base + 2
                    ));
                    args.push(Box::new(c.clone()));
                    args.push(Box::new(r.clone()));
                }
                sql.push_str(&format!(" ORDER BY collection, rkey LIMIT {limit}"));
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt
                    .query_map(
                        rusqlite::params_from_iter(args.iter().map(|a| a.as_ref())),
                        row_to_record,
                    )?
                    .collect::<Result<Vec<SpaceRecordRow>, rusqlite::Error>>()?;
                Ok(rows)
            })
            .await
    }

    /// Every record in a repo, in `(collection, rkey)` order (CAR export).
    pub async fn all_records(&self, space_uri: &str) -> Result<Vec<SpaceRecordRow>> {
        self.list_records(space_uri, None, usize::MAX >> 1, None)
            .await
    }

    /// Oplog entries after `since` (all history when `None`), paginated by row
    /// id. Errors with [`SpaceStoreError::HistoryUnavailable`] when compaction
    /// dropped revisions at or after the requested start.
    pub async fn list_repo_ops(
        &self,
        space_uri: &str,
        since: Option<String>,
        cursor: Option<i64>,
        limit: usize,
    ) -> Result<(Vec<SpaceOplogRow>, bool)> {
        let state = self.live_repo_state(space_uri).await?;
        if let Some(floor) = state.oplog_floor_rev {
            match since {
                Some(ref since) if since.as_str() >= floor.as_str() => {}
                _ => return Err(SpaceStoreError::HistoryUnavailable.into()),
            }
        }
        let space_uri = space_uri.to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT o.id, o.rev, o.collection, o.rkey, o.cid, o.prev, \
                            (SELECT r.value FROM space_record r \
                             WHERE r.space_uri = o.space_uri AND r.collection = o.collection \
                               AND r.rkey = o.rkey AND r.cid = o.cid) \
                     FROM space_oplog o \
                     WHERE o.space_uri = ?1 AND (?2 IS NULL OR o.rev > ?2) \
                       AND (?3 IS NULL OR o.id > ?3) \
                     ORDER BY o.id LIMIT ?4",
                )?;
                let mut rows = stmt
                    .query_map(
                        params![space_uri, since, cursor, (limit + 1) as i64],
                        |row| {
                            Ok(SpaceOplogRow {
                                id: row.get(0)?,
                                rev: row.get(1)?,
                                collection: row.get(2)?,
                                rkey: row.get(3)?,
                                cid: row.get(4)?,
                                prev: row.get(5)?,
                                value: row.get(6)?,
                            })
                        },
                    )?
                    .collect::<Result<Vec<SpaceOplogRow>, rusqlite::Error>>()?;
                let has_more = rows.len() > limit;
                rows.truncate(limit);
                Ok((rows, has_more))
            })
            .await
    }

    /// Space URIs the actor holds a live repo in, ordered for pagination.
    pub async fn list_spaces(&self, limit: usize, cursor: Option<String>) -> Result<Vec<String>> {
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT space_uri FROM space_repo WHERE deleted = 0 \
                     AND (?1 IS NULL OR space_uri > ?1) ORDER BY space_uri LIMIT ?2",
                )?;
                let rows = stmt
                    .query_map(params![cursor, limit as i64], |row| row.get(0))?
                    .collect::<Result<Vec<String>, rusqlite::Error>>()?;
                Ok(rows)
            })
            .await
    }

    /// Whether a blob is referenced by any record in the space.
    pub async fn space_references_blob(&self, space_uri: &str, blob_cid: &str) -> Result<bool> {
        let (space_uri, blob_cid) = (space_uri.to_string(), blob_cid.to_string());
        self.db
            .run(move |conn| {
                let count: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM space_blob_ref WHERE space_uri = ?1 AND blob_cid = ?2",
                    params![space_uri, blob_cid],
                    |row| row.get(0),
                )?;
                Ok(count > 0)
            })
            .await
    }

    /// Flag a repo as belonging to a deleted space. Never erases records: the
    /// data is the user's own (spec §Space deletion).
    pub async fn flag_repo_deleted(&self, space_uri: &str) -> Result<bool> {
        let space_uri = space_uri.to_string();
        self.db
            .run(move |conn| {
                let n = conn.execute(
                    "UPDATE space_repo SET deleted = 1 WHERE space_uri = ?1",
                    [&space_uri],
                )?;
                Ok(n > 0)
            })
            .await
    }

    // ---- repo-level write notifications ----

    pub async fn register_repo_notify(
        &self,
        space_uri: &str,
        endpoint: &str,
        expires_at: &str,
    ) -> Result<()> {
        let (space_uri, endpoint, expires_at) = (
            space_uri.to_string(),
            endpoint.to_string(),
            expires_at.to_string(),
        );
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO space_repo_notify (space_uri, endpoint, expires_at) \
                     VALUES (?1, ?2, ?3) \
                     ON CONFLICT (space_uri, endpoint) DO UPDATE SET expires_at = excluded.expires_at",
                    params![space_uri, endpoint, expires_at],
                )?;
                Ok(())
            })
            .await
    }

    /// Unexpired repo-notify endpoints for a space.
    pub async fn repo_notify_endpoints(&self, space_uri: &str, now: &str) -> Result<Vec<String>> {
        let (space_uri, now) = (space_uri.to_string(), now.to_string());
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT endpoint FROM space_repo_notify \
                     WHERE space_uri = ?1 AND expires_at > ?2 ORDER BY endpoint",
                )?;
                let rows = stmt
                    .query_map(params![space_uri, now], |row| row.get(0))?
                    .collect::<Result<Vec<String>, rusqlite::Error>>()?;
                Ok(rows)
            })
            .await
    }

    // ---- space-host role: definitions, members, writers, syncers, jti ----

    pub async fn create_space_def(&self, def: SpaceDefRow) -> Result<()> {
        let created_at = rsky_common::now();
        self.db
            .run(move |conn| {
                let existing: Option<i64> = conn
                    .query_row(
                        "SELECT deleted FROM space_def WHERE space_uri = ?1",
                        [&def.space_uri],
                        |row| row.get(0),
                    )
                    .optional()?;
                match existing {
                    Some(0) => {
                        return Err(
                            SpaceStoreError::RecordExists(def.space_uri.clone()).into()
                        )
                    }
                    Some(_) => {
                        return Err(SpaceStoreError::SpaceDeleted(def.space_uri.clone()).into())
                    }
                    None => {}
                }
                conn.execute(
                    "INSERT INTO space_def \
                     (space_uri, space_type, skey, policy, app_access, allowed_clients, managing_app, deleted, created_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8)",
                    params![
                        def.space_uri,
                        def.space_type,
                        def.skey,
                        def.policy,
                        def.app_access,
                        def.allowed_clients
                            .as_ref()
                            .map(|list| serde_json::to_string(list).expect("string list")),
                        def.managing_app,
                        created_at
                    ],
                )?;
                Ok(())
            })
            .await
    }

    pub async fn get_space_def(&self, space_uri: &str) -> Result<Option<SpaceDefRow>> {
        let space_uri = space_uri.to_string();
        self.db
            .run(move |conn| {
                let row = conn
                    .query_row(
                        "SELECT space_uri, space_type, skey, policy, app_access, allowed_clients, managing_app, deleted \
                         FROM space_def WHERE space_uri = ?1",
                        [&space_uri],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, String>(2)?,
                                row.get::<_, String>(3)?,
                                row.get::<_, String>(4)?,
                                row.get::<_, Option<String>>(5)?,
                                row.get::<_, Option<String>>(6)?,
                                row.get::<_, i64>(7)?,
                            ))
                        },
                    )
                    .optional()?;
                let Some((space_uri, space_type, skey, policy, app_access, allowed, managing_app, deleted)) =
                    row
                else {
                    return Ok(None);
                };
                let allowed_clients = match allowed {
                    Some(json) => Some(serde_json::from_str(&json)?),
                    None => None,
                };
                Ok(Some(SpaceDefRow {
                    space_uri,
                    space_type,
                    skey,
                    policy,
                    app_access,
                    allowed_clients,
                    managing_app,
                    deleted: deleted != 0,
                }))
            })
            .await
    }

    /// A space definition that exists and is not deleted.
    pub async fn live_space_def(&self, space_uri: &str) -> Result<SpaceDefRow> {
        let def = self
            .get_space_def(space_uri)
            .await?
            .ok_or_else(|| SpaceStoreError::SpaceNotFound(space_uri.to_string()))?;
        if def.deleted {
            return Err(SpaceStoreError::SpaceDeleted(space_uri.to_string()).into());
        }
        Ok(def)
    }

    pub async fn update_space_def(&self, def: SpaceDefRow) -> Result<()> {
        self.db
            .run(move |conn| {
                let n = conn.execute(
                    "UPDATE space_def SET policy = ?2, app_access = ?3, allowed_clients = ?4, managing_app = ?5 \
                     WHERE space_uri = ?1 AND deleted = 0",
                    params![
                        def.space_uri,
                        def.policy,
                        def.app_access,
                        def.allowed_clients
                            .as_ref()
                            .map(|list| serde_json::to_string(list).expect("string list")),
                        def.managing_app
                    ],
                )?;
                if n == 0 {
                    return Err(SpaceStoreError::SpaceNotFound(def.space_uri.clone()).into());
                }
                Ok(())
            })
            .await
    }

    pub async fn flag_space_def_deleted(&self, space_uri: &str) -> Result<bool> {
        let space_uri = space_uri.to_string();
        self.db
            .run(move |conn| {
                let n = conn.execute(
                    "UPDATE space_def SET deleted = 1 WHERE space_uri = ?1 AND deleted = 0",
                    [&space_uri],
                )?;
                Ok(n > 0)
            })
            .await
    }

    pub async fn add_member(&self, space_uri: &str, did: &str) -> Result<()> {
        let (space_uri, did) = (space_uri.to_string(), did.to_string());
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT OR IGNORE INTO space_member (space_uri, did) VALUES (?1, ?2)",
                    params![space_uri, did],
                )?;
                Ok(())
            })
            .await
    }

    pub async fn remove_member(&self, space_uri: &str, did: &str) -> Result<()> {
        let (space_uri, did) = (space_uri.to_string(), did.to_string());
        self.db
            .run(move |conn| {
                conn.execute(
                    "DELETE FROM space_member WHERE space_uri = ?1 AND did = ?2",
                    params![space_uri, did],
                )?;
                Ok(())
            })
            .await
    }

    pub async fn list_members(
        &self,
        space_uri: &str,
        limit: usize,
        cursor: Option<String>,
    ) -> Result<Vec<String>> {
        let space_uri = space_uri.to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT did FROM space_member WHERE space_uri = ?1 \
                     AND (?2 IS NULL OR did > ?2) ORDER BY did LIMIT ?3",
                )?;
                let rows = stmt
                    .query_map(params![space_uri, cursor, limit as i64], |row| row.get(0))?
                    .collect::<Result<Vec<String>, rusqlite::Error>>()?;
                Ok(rows)
            })
            .await
    }

    pub async fn upsert_writer(
        &self,
        space_uri: &str,
        did: &str,
        rev: &str,
        hash: Option<String>,
    ) -> Result<()> {
        let (space_uri, did, rev) = (space_uri.to_string(), did.to_string(), rev.to_string());
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO space_writer (space_uri, did, rev, hash) VALUES (?1, ?2, ?3, ?4) \
                     ON CONFLICT (space_uri, did) DO UPDATE SET rev = excluded.rev, hash = excluded.hash",
                    params![space_uri, did, rev, hash],
                )?;
                Ok(())
            })
            .await
    }

    pub async fn list_writers(
        &self,
        space_uri: &str,
        limit: usize,
        cursor: Option<String>,
    ) -> Result<Vec<SpaceWriterRow>> {
        let space_uri = space_uri.to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT did, rev, hash FROM space_writer WHERE space_uri = ?1 \
                     AND (?2 IS NULL OR did > ?2) ORDER BY did LIMIT ?3",
                )?;
                let rows = stmt
                    .query_map(params![space_uri, cursor, limit as i64], |row| {
                        Ok(SpaceWriterRow {
                            did: row.get(0)?,
                            rev: row.get(1)?,
                            hash: row.get(2)?,
                        })
                    })?
                    .collect::<Result<Vec<SpaceWriterRow>, rusqlite::Error>>()?;
                Ok(rows)
            })
            .await
    }

    pub async fn register_host_notify(
        &self,
        space_uri: &str,
        endpoint: &str,
        expires_at: &str,
    ) -> Result<()> {
        let (space_uri, endpoint, expires_at) = (
            space_uri.to_string(),
            endpoint.to_string(),
            expires_at.to_string(),
        );
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO space_host_reg (space_uri, endpoint, expires_at) \
                     VALUES (?1, ?2, ?3) \
                     ON CONFLICT (space_uri, endpoint) DO UPDATE SET expires_at = excluded.expires_at",
                    params![space_uri, endpoint, expires_at],
                )?;
                Ok(())
            })
            .await
    }

    pub async fn host_notify_endpoints(&self, space_uri: &str, now: &str) -> Result<Vec<String>> {
        let (space_uri, now) = (space_uri.to_string(), now.to_string());
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT endpoint FROM space_host_reg \
                     WHERE space_uri = ?1 AND expires_at > ?2 ORDER BY endpoint",
                )?;
                let rows = stmt
                    .query_map(params![space_uri, now], |row| row.get(0))?
                    .collect::<Result<Vec<String>, rusqlite::Error>>()?;
                Ok(rows)
            })
            .await
    }

    /// Single-use nonce tracking. Returns true when `jti` was unseen; expired
    /// entries are purged opportunistically.
    pub async fn consume_jti(&self, jti: &str, exp: i64, now: i64) -> Result<bool> {
        let jti = jti.to_string();
        self.db
            .run(move |conn| {
                conn.execute("DELETE FROM space_used_jti WHERE exp < ?1", [now])?;
                let inserted = conn.execute(
                    "INSERT OR IGNORE INTO space_used_jti (jti, exp) VALUES (?1, ?2)",
                    params![jti, exp],
                )?;
                Ok(inserted > 0)
            })
            .await
    }
}

fn row_to_record(row: &rusqlite::Row) -> Result<SpaceRecordRow, rusqlite::Error> {
    Ok(SpaceRecordRow {
        collection: row.get(0)?,
        rkey: row.get(1)?,
        cid: row.get(2)?,
        rev: row.get(3)?,
        value: row.get(4)?,
    })
}

fn apply_writes_tx(
    tx: &Transaction,
    space_uri: &str,
    authority: &str,
    space_type: &str,
    skey: &str,
    writes: &[PreparedSpaceWrite],
    window: usize,
) -> Result<SpaceCommitResult> {
    let existing: Option<(String, Vec<u8>, i64)> = tx
        .query_row(
            "SELECT rev, lthash_state, deleted FROM space_repo WHERE space_uri = ?1",
            [space_uri],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let (prev_rev, mut lthash) = match existing {
        Some((_, _, deleted)) if deleted != 0 => {
            return Err(SpaceStoreError::SpaceDeleted(space_uri.to_string()).into())
        }
        Some((rev, state, _)) => {
            let mut buf = [0u8; 2048];
            buf.copy_from_slice(&state);
            (Some(rev), LtHash::from_state_bytes(&buf))
        }
        None => (None, LtHash::new()),
    };
    let rev = TID::next_str(prev_rev.clone())?;
    let mut results = Vec::with_capacity(writes.len());
    for write in writes {
        let path = format!("{}/{}", write.collection, write.rkey);
        let current: Option<String> = tx
            .query_row(
                "SELECT cid FROM space_record WHERE space_uri = ?1 AND collection = ?2 AND rkey = ?3",
                params![space_uri, write.collection, write.rkey],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(ref swap) = write.swap_cid {
            if swap.is_some() && *swap != current {
                return Err(SpaceStoreError::InvalidSwap(current).into());
            }
        }
        match write.action {
            PreparedAction::Create if current.is_some() => {
                return Err(SpaceStoreError::RecordExists(path).into())
            }
            PreparedAction::Update | PreparedAction::Delete if current.is_none() => {
                return Err(SpaceStoreError::RecordNotFound(path).into())
            }
            _ => {}
        }
        if let Some(ref old_cid) = current {
            lthash.remove(&element(&write.collection, &write.rkey, old_cid));
            tx.execute(
                "DELETE FROM space_blob_ref WHERE space_uri = ?1 AND collection = ?2 AND rkey = ?3",
                params![space_uri, write.collection, write.rkey],
            )?;
        }
        match &write.new {
            Some((cid, bytes, blob_cids)) => {
                lthash.add(&element(&write.collection, &write.rkey, cid));
                tx.execute(
                    "INSERT INTO space_record (space_uri, collection, rkey, cid, rev, value) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
                     ON CONFLICT (space_uri, collection, rkey) \
                     DO UPDATE SET cid = excluded.cid, rev = excluded.rev, value = excluded.value",
                    params![space_uri, write.collection, write.rkey, cid, rev, bytes],
                )?;
                for blob_cid in blob_cids {
                    tx.execute(
                        "INSERT OR IGNORE INTO space_blob_ref (space_uri, blob_cid, collection, rkey) \
                         VALUES (?1, ?2, ?3, ?4)",
                        params![space_uri, blob_cid, write.collection, write.rkey],
                    )?;
                }
            }
            None => {
                tx.execute(
                    "DELETE FROM space_record WHERE space_uri = ?1 AND collection = ?2 AND rkey = ?3",
                    params![space_uri, write.collection, write.rkey],
                )?;
            }
        }
        let cid = write.new.as_ref().map(|(cid, _, _)| cid.clone());
        tx.execute(
            "INSERT INTO space_oplog (space_uri, rev, collection, rkey, cid, prev) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![space_uri, rev, write.collection, write.rkey, cid, current],
        )?;
        results.push(SpaceWriteResult {
            collection: write.collection.clone(),
            rkey: write.rkey.clone(),
            cid,
            prev: current,
        });
    }
    let state = lthash.state_bytes().to_vec();
    if prev_rev.is_some() {
        tx.execute(
            "UPDATE space_repo SET rev = ?2, lthash_state = ?3 WHERE space_uri = ?1",
            params![space_uri, rev, state],
        )?;
    } else {
        tx.execute(
            "INSERT INTO space_repo \
             (space_uri, authority, space_type, skey, rev, lthash_state, oplog_floor_rev, deleted, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, 0, ?7)",
            params![
                space_uri,
                authority,
                space_type,
                skey,
                rev,
                state,
                rsky_common::now()
            ],
        )?;
    }
    compact_oplog(tx, space_uri, window)?;
    Ok(SpaceCommitResult {
        rev,
        hash: lthash.hash(),
        results,
    })
}

/// Keep at most `window` oplog rows per space, dropping whole revisions from
/// the oldest end and advancing `oplog_floor_rev` to the newest dropped rev.
fn compact_oplog(tx: &Transaction, space_uri: &str, window: usize) -> Result<()> {
    let cutoff_rev: Option<String> = tx
        .query_row(
            "SELECT rev FROM space_oplog WHERE space_uri = ?1 \
             ORDER BY id DESC LIMIT 1 OFFSET ?2",
            params![space_uri, (window - 1) as i64],
            |row| row.get(0),
        )
        .optional()?;
    let Some(cutoff_rev) = cutoff_rev else {
        return Ok(());
    };
    let floor: Option<String> = tx.query_row(
        "SELECT MAX(rev) FROM space_oplog WHERE space_uri = ?1 AND rev < ?2",
        params![space_uri, cutoff_rev],
        |row| row.get(0),
    )?;
    let Some(floor) = floor else {
        return Ok(());
    };
    tx.execute(
        "DELETE FROM space_oplog WHERE space_uri = ?1 AND rev < ?2",
        params![space_uri, cutoff_rev],
    )?;
    tx.execute(
        "UPDATE space_repo SET oplog_floor_rev = ?2 WHERE space_uri = ?1",
        params![space_uri, floor],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests;
