use crate::actor_store::blobstore::BlobStore;
use crate::actor_store::db::{ActorDb, Blob as BlobRow};
use crate::actor_store::repo::sql_repo::placeholders;
use crate::background::BackgroundQueue;
use crate::image;
use anyhow::{bail, Result};
use aws_sdk_s3::primitives::ByteStream;
use futures::try_join;
use lexicon_cid::Cid;
use rsky_common::ipld::sha256_to_cid;
use rsky_common::now;
use rsky_lexicon::blob_refs::BlobRef;
use rsky_lexicon::com::atproto::admin::StatusAttr;
use rsky_lexicon::com::atproto::repo::ListMissingBlobsRefRecordBlob;
use rsky_repo::types::{BlobConstraint, PreparedBlobRef, PreparedWrite};
use rusqlite::OptionalExtension;
use sha2::{Digest, Sha256};
use std::str::FromStr;
use std::sync::Arc;

pub struct BlobMetadata {
    pub temp_key: String,
    pub size: i64,
    pub cid: Cid,
    pub mime_type: String,
    pub width: Option<i32>,
    pub height: Option<i32>,
}

pub struct BlobReader {
    pub blobstore: Arc<dyn BlobStore>,
    pub db: ActorDb,
    pub background_queue: BackgroundQueue,
    /// Another implementation may still serve this store's objects, so
    /// nothing is deleted from object storage; dereferenced objects are
    /// journaled for a collector that runs once that is no longer true.
    pub coexistence: bool,
}

/// The state of one journaled unit of object-storage work. Every variant
/// is classified as terminal or not, and the drain and convergence
/// predicates use the classification rather than a list, so a variant added
/// later cannot escape them unnoticed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlobWorkState {
    /// The object is dereferenced and will be deleted by the worker.
    DeletePending,
    /// The object is dereferenced but stays until a collector runs after
    /// every other implementation sharing the store is gone.
    GcDeferred,
    /// A taken-down object whose permanent copy is missing is being
    /// restored from quarantine; the row carries the moderation version the
    /// restoration was requested under.
    RestorePending,
    /// A newer moderation decision made the row moot.
    Superseded,
    Done,
}

impl BlobWorkState {
    pub const ALL: [BlobWorkState; 5] = [
        BlobWorkState::DeletePending,
        BlobWorkState::GcDeferred,
        BlobWorkState::RestorePending,
        BlobWorkState::Superseded,
        BlobWorkState::Done,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            BlobWorkState::DeletePending => "delete-pending",
            BlobWorkState::GcDeferred => "gc-deferred",
            BlobWorkState::RestorePending => "restore-pending",
            BlobWorkState::Superseded => "superseded",
            BlobWorkState::Done => "done",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|state| state.as_str() == value)
            .ok_or_else(|| anyhow::anyhow!("unknown blob work state: {value}"))
    }

    /// Whether the row needs no further action from any worker. A drain,
    /// hand-back, or convergence check waits only on non-terminal rows.
    pub fn is_terminal(self) -> bool {
        match self {
            BlobWorkState::DeletePending | BlobWorkState::RestorePending => false,
            BlobWorkState::GcDeferred | BlobWorkState::Superseded | BlobWorkState::Done => true,
        }
    }
}

/// Which object a `blob_work` row names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlobWorkKind {
    Permanent,
    Temp,
    Quarantine,
}

impl BlobWorkKind {
    pub fn as_str(self) -> &'static str {
        match self {
            BlobWorkKind::Permanent => "permanent",
            BlobWorkKind::Temp => "temp",
            BlobWorkKind::Quarantine => "quarantine",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "permanent" => Ok(BlobWorkKind::Permanent),
            "temp" => Ok(BlobWorkKind::Temp),
            "quarantine" => Ok(BlobWorkKind::Quarantine),
            other => bail!("unknown blob work kind: {other}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobWork {
    pub id: i64,
    pub kind: BlobWorkKind,
    /// The CID of a permanent or quarantined object, or the temporary key of
    /// a temp object.
    pub key: String,
    pub cid: Option<String>,
    pub state: BlobWorkState,
    /// For a restoration, the moderation version it was requested under.
    pub version: Option<i64>,
    pub created_at: String,
}

/// Where a restoration stopped; tests stop after a step to stand in for a
/// crash there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreStep {
    Copied,
    Confirmed,
}

/// A blob promoted out of temporary storage before a write's transaction;
/// the transaction records the promotion by clearing its temporary key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromotedBlob {
    pub cid: Cid,
    pub temp_key: String,
}

const SELECT_BLOB_WORK: &str =
    "SELECT id, kind, key, cid, state, version, \"createdAt\" FROM blob_work";

fn blob_work_from_row(row: &rusqlite::Row) -> Result<BlobWork> {
    Ok(BlobWork {
        id: row.get(0)?,
        kind: BlobWorkKind::parse(&row.get::<_, String>(1)?)?,
        key: row.get(2)?,
        cid: row.get(3)?,
        state: BlobWorkState::parse(&row.get::<_, String>(4)?)?,
        version: row.get(5)?,
        created_at: row.get(6)?,
    })
}

fn insert_blob_work_in(
    conn: &rusqlite::Connection,
    kind: BlobWorkKind,
    key: &str,
    cid: Option<&str>,
    state: BlobWorkState,
    version: Option<i64>,
    now: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO blob_work (kind, key, cid, state, version, \"createdAt\", \"updatedAt\") \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        rusqlite::params![kind.as_str(), key, cid, state.as_str(), version, now],
    )?;
    Ok(())
}

fn set_blob_work_state_in(
    conn: &rusqlite::Connection,
    id: i64,
    state: BlobWorkState,
    now: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE blob_work SET state = ?1, \"updatedAt\" = ?2 WHERE id = ?3",
        rusqlite::params![state.as_str(), now, id],
    )?;
    Ok(())
}

/// Advances the blob's moderation version and marks every restoration
/// requested under an earlier version moot; returns the new version.
fn bump_moderation_version_in(conn: &rusqlite::Connection, cid: &str, now: &str) -> Result<i64> {
    conn.execute(
        "INSERT INTO blob_moderation (cid, version) VALUES (?1, 1) \
         ON CONFLICT (cid) DO UPDATE SET version = version + 1",
        [cid],
    )?;
    let version: i64 = conn.query_row(
        "SELECT version FROM blob_moderation WHERE cid = ?1",
        [cid],
        |row| row.get(0),
    )?;
    conn.execute(
        "UPDATE blob_work SET state = ?1, \"updatedAt\" = ?2 WHERE cid = ?3 AND state = ?4",
        rusqlite::params![
            BlobWorkState::Superseded.as_str(),
            now,
            cid,
            BlobWorkState::RestorePending.as_str()
        ],
    )?;
    Ok(version)
}

/// Clears the takedown only if no newer moderation decision was made since
/// `version`; returns whether it did.
fn clear_takedown_at_version_in(
    conn: &rusqlite::Connection,
    cid: &str,
    version: i64,
) -> Result<bool> {
    let changed = conn.execute(
        "UPDATE blob SET \"takedownRef\" = NULL WHERE cid = ?1 \
         AND (SELECT version FROM blob_moderation WHERE cid = ?1) = ?2",
        rusqlite::params![cid, version],
    )?;
    Ok(changed == 1)
}

fn query_strings<P: rusqlite::ToSql>(
    conn: &rusqlite::Connection,
    sql: &str,
    params: &[P],
) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(params.iter()), |row| {
            row.get::<_, String>(0)
        })?
        .collect::<Result<Vec<String>, rusqlite::Error>>()?;
    Ok(rows)
}

fn written_blobs(writes: &[PreparedWrite]) -> Vec<(&PreparedBlobRef, &str)> {
    writes
        .iter()
        .flat_map(|write| match write {
            PreparedWrite::Create(w) | PreparedWrite::Update(w) => {
                w.blobs.iter().map(|blob| (blob, w.uri.as_str())).collect()
            }
            PreparedWrite::Delete(_) => Vec::new(),
        })
        .collect()
}

/// Removes the blob registrations a set of writes dereferences, on a
/// connection inside a transaction, and journals the objects for deletion
/// or deferred collection.
pub fn dereference_blobs_in(
    conn: &rusqlite::Connection,
    writes: &[PreparedWrite],
    coexistence: bool,
    now: &str,
) -> Result<Vec<String>> {
    let uris: Vec<&str> = writes
        .iter()
        .filter_map(|w| match w {
            PreparedWrite::Delete(w) => Some(w.uri.as_str()),
            PreparedWrite::Update(w) => Some(w.uri.as_str()),
            PreparedWrite::Create(_) => None,
        })
        .collect();
    if uris.is_empty() {
        return Ok(vec![]);
    }
    let deleted_repo_blob_cids = query_strings(
        conn,
        &format!(
            "DELETE FROM record_blob WHERE \"recordUri\" IN ({}) RETURNING \"blobCid\"",
            placeholders(uris.len())
        ),
        &uris,
    )?;
    if deleted_repo_blob_cids.is_empty() {
        return Ok(vec![]);
    }
    let still_referenced = query_strings(
        conn,
        &format!(
            "SELECT \"blobCid\" FROM record_blob WHERE \"blobCid\" IN ({})",
            placeholders(deleted_repo_blob_cids.len())
        ),
        &deleted_repo_blob_cids,
    )?;
    let newly_written: Vec<String> = written_blobs(writes)
        .into_iter()
        .map(|(blob, _)| blob.cid.to_string())
        .collect();
    let cids_to_delete: Vec<String> = deleted_repo_blob_cids
        .into_iter()
        .filter(|cid| !still_referenced.contains(cid) && !newly_written.contains(cid))
        .collect();
    if cids_to_delete.is_empty() {
        return Ok(vec![]);
    }
    let sql = format!(
        "DELETE FROM blob WHERE cid IN ({})",
        placeholders(cids_to_delete.len())
    );
    conn.execute(&sql, rusqlite::params_from_iter(cids_to_delete.iter()))?;
    let state = if coexistence {
        BlobWorkState::GcDeferred
    } else {
        BlobWorkState::DeletePending
    };
    for cid in &cids_to_delete {
        insert_blob_work_in(
            conn,
            BlobWorkKind::Permanent,
            cid,
            Some(cid),
            state,
            None,
            now,
        )?;
    }
    Ok(cids_to_delete)
}

/// Records a promotion: the temporary key is cleared, and under coexistence
/// the temporary object, which was copied rather than moved, is left for
/// the collector.
fn record_promotion_in(
    conn: &rusqlite::Connection,
    blob: &PromotedBlob,
    coexistence: bool,
    now: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE blob SET \"tempKey\" = NULL WHERE cid = ?1 AND \"tempKey\" = ?2",
        rusqlite::params![blob.cid.to_string(), blob.temp_key],
    )?;
    if coexistence {
        insert_blob_work_in(
            conn,
            BlobWorkKind::Temp,
            &blob.temp_key,
            Some(&blob.cid.to_string()),
            BlobWorkState::GcDeferred,
            None,
            now,
        )?;
    }
    Ok(())
}

/// Records a write's blob effects on a connection inside a transaction:
/// dereferenced registrations go, promoted blobs lose their temporary key,
/// and the written records are associated with their blobs.
pub fn apply_write_blobs_in(
    conn: &rusqlite::Connection,
    writes: &[PreparedWrite],
    promoted: &[PromotedBlob],
    coexistence: bool,
    now: &str,
) -> Result<()> {
    dereference_blobs_in(conn, writes, coexistence, now)?;
    for blob in promoted {
        record_promotion_in(conn, blob, coexistence, now)?;
    }
    let mut associate = conn.prepare_cached(
        "INSERT INTO record_blob (\"blobCid\", \"recordUri\") VALUES (?1, ?2) \
         ON CONFLICT DO NOTHING",
    )?;
    for (blob, uri) in written_blobs(writes) {
        associate.execute(rusqlite::params![blob.cid.to_string(), uri])?;
    }
    Ok(())
}

pub struct ListMissingBlobsOpts {
    pub cursor: Option<String>,
    pub limit: u16,
}

pub struct ListBlobsOpts {
    pub since: Option<String>,
    pub cursor: Option<String>,
    pub limit: u16,
}

pub struct GetBlobOutput {
    pub size: i64,
    pub mime_type: Option<String>,
    pub stream: ByteStream,
}

pub struct GetBlobMetadataOutput {
    pub size: i64,
    pub mime_type: Option<String>,
}

fn blob_from_row(row: &rusqlite::Row) -> Result<BlobRow, rusqlite::Error> {
    Ok(BlobRow {
        cid: row.get("cid")?,
        mime_type: row.get("mimeType")?,
        size: row.get("size")?,
        temp_key: row.get("tempKey")?,
        width: row.get("width")?,
        height: row.get("height")?,
        created_at: row.get("createdAt")?,
        takedown_ref: row.get("takedownRef")?,
    })
}

// Handles blob metadata rows in the per-actor db plus blobstore lifecycle
impl BlobReader {
    pub fn new(
        blobstore: Arc<dyn BlobStore>,
        db: ActorDb,
        background_queue: BackgroundQueue,
        coexistence: bool,
    ) -> Self {
        BlobReader {
            blobstore,
            db,
            background_queue,
            coexistence,
        }
    }

    pub async fn get_blob_metadata(&self, cid: Cid) -> Result<GetBlobMetadataOutput> {
        let found: Option<BlobRow> = self
            .db
            .run(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT * FROM blob WHERE cid = ?1 AND \"takedownRef\" IS NULL \
                         AND \"tempKey\" IS NULL",
                        [cid.to_string()],
                        blob_from_row,
                    )
                    .optional()?)
            })
            .await?;
        match found {
            None => Err(crate::actor_store::blobstore::BlobNotFoundError.into()),
            Some(found) => Ok(GetBlobMetadataOutput {
                size: found.size,
                mime_type: Some(found.mime_type),
            }),
        }
    }

    pub async fn get_blob(&self, cid: Cid) -> Result<GetBlobOutput> {
        let metadata = self.get_blob_metadata(cid).await?;
        let blob_stream = self.blobstore.get_stream(cid).await?;
        Ok(GetBlobOutput {
            size: metadata.size,
            mime_type: metadata.mime_type,
            stream: blob_stream,
        })
    }

    pub async fn get_records_for_blob(&self, cid: Cid) -> Result<Vec<String>> {
        self.db
            .run(move |conn| {
                let mut stmt =
                    conn.prepare("SELECT \"recordUri\" FROM record_blob WHERE \"blobCid\" = ?1")?;
                let rows = stmt
                    .query_map([cid.to_string()], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<String>, rusqlite::Error>>()?;
                Ok(rows)
            })
            .await
    }

    pub async fn upload_blob_and_get_metadata(
        &self,
        user_suggested_mime: String,
        bytes: Vec<u8>,
    ) -> Result<BlobMetadata> {
        let size = bytes.len() as i64;
        let (temp_key, sha256, img_info, sniffed_mime) = try_join!(
            self.blobstore.put_temp(bytes.clone()),
            sha256_stream(bytes.clone()),
            image::maybe_get_info(bytes.clone()),
            image::mime_type_from_bytes(bytes.clone())
        )?;
        let cid = sha256_to_cid(sha256);
        let mime_type = sniffed_mime.unwrap_or(user_suggested_mime);

        Ok(BlobMetadata {
            temp_key,
            size,
            cid,
            mime_type,
            width: img_info.as_ref().map(|info| info.width as i32),
            height: img_info.map(|info| info.height as i32),
        })
    }

    /// Registers a spooled upload: hashed and sniffed from the file, then
    /// stored without being read into memory where the store allows it.
    pub async fn upload_blob_from_path(
        &self,
        user_suggested_mime: String,
        path: std::path::PathBuf,
    ) -> Result<BlobMetadata> {
        let hashed = tokio::task::spawn_blocking({
            let path = path.clone();
            move || -> Result<(Vec<u8>, i64, Vec<u8>)> {
                use std::io::Read;
                let mut file = std::fs::File::open(&path)?;
                let mut hasher = Sha256::new();
                let mut head = Vec::new();
                let mut size = 0i64;
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    let n = file.read(&mut buf)?;
                    if n == 0 {
                        break;
                    }
                    hasher.update(&buf[..n]);
                    size += n as i64;
                    if head.len() < 8192 {
                        let take = (8192 - head.len()).min(n);
                        head.extend_from_slice(&buf[..take]);
                    }
                }
                Ok((hasher.finalize().to_vec(), size, head))
            }
        })
        .await??;
        let (sha256, size, head) = hashed;
        let (temp_key, img_info, sniffed_mime) = try_join!(
            self.blobstore.put_temp_from_path(path.clone()),
            image::maybe_get_info_from_path(path),
            image::mime_type_from_bytes(head)
        )?;
        Ok(BlobMetadata {
            temp_key,
            size,
            cid: sha256_to_cid(sha256),
            mime_type: sniffed_mime.unwrap_or(user_suggested_mime),
            width: img_info.as_ref().map(|info| info.width as i32),
            height: img_info.map(|info| info.height as i32),
        })
    }

    pub async fn track_untethered_blob(&self, metadata: BlobMetadata) -> Result<BlobRef> {
        let BlobMetadata {
            temp_key,
            size,
            cid,
            mime_type,
            width,
            height,
        } = metadata;
        let mime_type_clone = mime_type.clone();
        let temp_key_for_row = temp_key.clone();
        self.db
            .run(move |conn| {
                let found: Option<BlobRow> = conn
                    .query_row(
                        "SELECT * FROM blob WHERE cid = ?1",
                        [cid.to_string()],
                        blob_from_row,
                    )
                    .optional()?;
                if let Some(found) = found {
                    if found.takedown_ref.is_some() {
                        bail!("Blob has been takendown, cannot re-upload")
                    }
                }
                conn.execute(
                    "INSERT INTO blob (cid, \"mimeType\", size, \"tempKey\", width, height, \"createdAt\") \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
                     ON CONFLICT (cid) DO UPDATE SET \"tempKey\" = excluded.\"tempKey\" \
                     WHERE blob.\"tempKey\" IS NOT NULL",
                    rusqlite::params![
                        cid.to_string(),
                        mime_type_clone,
                        size,
                        temp_key_for_row,
                        width,
                        height,
                        now()
                    ],
                )?;
                Ok(())
            })
            .await?;
        // A re-upload of a blob that is already permanent adopts nothing:
        // the fresh temporary object is never referenced, and under
        // coexistence it is left for the collector rather than deleted.
        if self.coexistence {
            let cid_str = cid.to_string();
            let temp_key = temp_key.clone();
            let stamp = now();
            self.db
                .run(move |conn| {
                    let adopted: bool = conn
                        .query_row(
                            "SELECT COALESCE(\"tempKey\" = ?1, 0) FROM blob WHERE cid = ?2",
                            rusqlite::params![temp_key, cid_str],
                            |row| row.get(0),
                        )
                        .optional()?
                        .unwrap_or(false);
                    if !adopted {
                        insert_blob_work_in(
                            conn,
                            BlobWorkKind::Temp,
                            &temp_key,
                            Some(&cid_str),
                            BlobWorkState::GcDeferred,
                            None,
                            &stamp,
                        )?;
                    }
                    Ok(())
                })
                .await?;
        }
        // A blob uploaded after the record referencing it was imported is
        // already associated; promote it now or it stays temp forever.
        let cid_str = cid.to_string();
        let associated: bool = self
            .db
            .run(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT 1 FROM record_blob WHERE \"blobCid\" = ?1 LIMIT 1",
                        [cid_str.clone()],
                        |_| Ok(()),
                    )
                    .optional()?
                    .is_some())
            })
            .await?;
        if associated {
            self.verify_blob_and_make_permanent(PreparedBlobRef {
                cid,
                mime_type: mime_type.clone(),
                constraints: BlobConstraint {
                    max_size: None,
                    accept: None,
                },
            })
            .await?;
        }
        Ok(BlobRef::new(cid, mime_type, size, None))
    }

    /// Verifies every blob the writes reference and moves it out of
    /// temporary storage, before the write's transaction. A blob a CAR
    /// import references typically arrives by uploadBlob afterwards, so an
    /// import tolerates a missing blob and the upload promotes it later
    /// (see `track_untethered_blob`); an ordinary write does not.
    pub async fn promote_write_blobs(
        &self,
        writes: &[PreparedWrite],
        tolerate_missing: bool,
    ) -> Result<Vec<PromotedBlob>> {
        let mut promoted = Vec::new();
        for (blob, _) in written_blobs(writes) {
            let cid = blob.cid;
            let found: Option<BlobRow> = self
                .db
                .run(move |conn| {
                    Ok(conn
                        .query_row(
                            "SELECT * FROM blob WHERE cid = ?1 AND \"takedownRef\" IS NULL",
                            [cid.to_string()],
                            blob_from_row,
                        )
                        .optional()?)
                })
                .await?;
            let Some(found) = found else {
                if tolerate_missing {
                    tracing::debug!(cid = %cid, "written record references a blob not yet uploaded");
                    continue;
                }
                bail!("Could not find blob: {:?}", cid.to_string())
            };
            verify_blob(blob, &found).await?;
            if let Some(temp_key) = found.temp_key {
                self.promote(&temp_key, cid).await?;
                promoted.push(PromotedBlob { cid, temp_key });
            }
        }
        Ok(promoted)
    }

    /// Moves a temporary object to its permanent key, or copies it when
    /// another implementation may still read the temporary one.
    async fn promote(&self, temp_key: &str, cid: Cid) -> Result<()> {
        match self.coexistence {
            true => {
                self.blobstore
                    .make_permanent_copy_only(temp_key.to_owned(), cid)
                    .await
            }
            false => {
                self.blobstore
                    .make_permanent(temp_key.to_owned(), cid)
                    .await
            }
        }
    }

    /// The write path as one unit, for callers outside a store transaction:
    /// promote, record the effects, then run any deletion the write left.
    pub async fn process_write_blobs(&self, writes: Vec<PreparedWrite>) -> Result<()> {
        let promoted = self.promote_write_blobs(&writes, false).await?;
        let coexistence = self.coexistence;
        let now = now();
        self.db
            .tx(move |tx| apply_write_blobs_in(tx, &writes, &promoted, coexistence, &now))
            .await?;
        self.queue_blob_work();
        Ok(())
    }

    /// Runs the journaled object deletions in the background.
    pub fn queue_blob_work(&self) {
        let worker = BlobReader {
            blobstore: self.blobstore.clone(),
            db: self.db.clone(),
            background_queue: self.background_queue.clone(),
            coexistence: self.coexistence,
        };
        self.background_queue
            .add(async move { worker.run_blob_work().await });
    }

    /// Deletes every object journaled `delete-pending` and marks the row
    /// done; a row whose deletion fails stays pending for the next run.
    pub async fn run_blob_work(&self) -> Result<()> {
        let all = self.blob_work().await?;
        for restore in all
            .iter()
            .filter(|work| work.state == BlobWorkState::RestorePending)
        {
            self.run_restore(restore, None).await?;
        }
        let pending: Vec<BlobWork> = all
            .into_iter()
            .filter(|work| work.state == BlobWorkState::DeletePending)
            .collect();
        if pending.is_empty() {
            return Ok(());
        }
        let cids = pending
            .iter()
            .filter(|work| work.kind == BlobWorkKind::Permanent)
            .map(|work| Cid::from_str(&work.key).map_err(anyhow::Error::new))
            .collect::<Result<Vec<Cid>>>()?;
        if !cids.is_empty() {
            self.blobstore.delete_many(cids).await?;
        }
        let ids: Vec<i64> = pending.iter().map(|work| work.id).collect();
        let done_at = now();
        self.db
            .run(move |conn| {
                let sql = format!(
                    "UPDATE blob_work SET state = ?1, \"updatedAt\" = ?2 WHERE id IN ({})",
                    placeholders(ids.len())
                );
                let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![
                    Box::new(BlobWorkState::Done.as_str()),
                    Box::new(done_at.clone()),
                ];
                params.extend(
                    ids.iter()
                        .map(|id| Box::new(*id) as Box<dyn rusqlite::types::ToSql>),
                );
                conn.execute(
                    &sql,
                    rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
                )?;
                Ok(())
            })
            .await
    }

    pub async fn blob_work(&self) -> Result<Vec<BlobWork>> {
        self.db
            .run(|conn| {
                let mut stmt = conn.prepare(&format!("{SELECT_BLOB_WORK} ORDER BY id"))?;
                let rows = stmt
                    .query_and_then([], blob_work_from_row)?
                    .collect::<Result<Vec<BlobWork>>>()?;
                Ok(rows)
            })
            .await
    }

    /// Rows still owed to a worker; the drain and convergence predicates
    /// wait on this count.
    pub async fn nonterminal_blob_work(&self) -> Result<usize> {
        Ok(self
            .blob_work()
            .await?
            .into_iter()
            .filter(|work| !work.state.is_terminal())
            .count())
    }

    pub async fn verify_blob_and_make_permanent(&self, blob: PreparedBlobRef) -> Result<()> {
        let cid = blob.cid;
        let found: Option<BlobRow> = self
            .db
            .run(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT * FROM blob WHERE cid = ?1 AND \"takedownRef\" IS NULL",
                        [cid.to_string()],
                        blob_from_row,
                    )
                    .optional()?)
            })
            .await?;
        if let Some(found) = found {
            verify_blob(&blob, &found).await?;
            if let Some(temp_key) = found.temp_key {
                self.promote(&temp_key, cid).await?;
                let promoted = PromotedBlob { cid, temp_key };
                let coexistence = self.coexistence;
                let now = now();
                self.db
                    .tx(move |tx| record_promotion_in(tx, &promoted, coexistence, &now))
                    .await?;
            }
            Ok(())
        } else {
            bail!("Could not find blob: {:?}", blob.cid.to_string())
        }
    }

    pub async fn associate_blob(&self, blob: PreparedBlobRef, record_uri: String) -> Result<()> {
        let cid = blob.cid.to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO record_blob (\"blobCid\", \"recordUri\") \
                     VALUES (?1, ?2) ON CONFLICT DO NOTHING",
                    rusqlite::params![cid, record_uri],
                )?;
                Ok(())
            })
            .await
    }

    pub async fn blob_count(&self) -> Result<i64> {
        self.db
            .run(|conn| Ok(conn.query_row("SELECT count(*) FROM blob", [], |row| row.get(0))?))
            .await
    }

    pub async fn record_blob_count(&self) -> Result<i64> {
        self.db
            .run(|conn| {
                Ok(conn.query_row(
                    "SELECT count(DISTINCT \"blobCid\") FROM record_blob",
                    [],
                    |row| row.get(0),
                )?)
            })
            .await
    }

    pub async fn get_blob_cids(&self) -> Result<Vec<Cid>> {
        let rows: Vec<String> = self
            .db
            .run(|conn| {
                let mut stmt = conn.prepare("SELECT cid FROM blob")?;
                let rows = stmt
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<String>, rusqlite::Error>>()?;
                Ok(rows)
            })
            .await?;
        rows.into_iter()
            .map(|cid| Cid::from_str(&cid).map_err(anyhow::Error::new))
            .collect()
    }

    pub async fn list_missing_blobs(
        &self,
        opts: ListMissingBlobsOpts,
    ) -> Result<Vec<ListMissingBlobsRefRecordBlob>> {
        let ListMissingBlobsOpts { cursor, limit } = opts;
        if limit > 1000 {
            bail!("Limit too high. Max: 1000.");
        }
        self.db
            .run(move |conn| {
                let mut sql = String::from(
                    "SELECT \"blobCid\", \"recordUri\" FROM record_blob \
                     WHERE NOT EXISTS (SELECT 1 FROM blob WHERE blob.cid = record_blob.\"blobCid\")",
                );
                let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
                if let Some(cursor) = &cursor {
                    sql.push_str(" AND \"blobCid\" > ?");
                    params.push(Box::new(cursor.clone()));
                }
                sql.push_str(" GROUP BY \"blobCid\" ORDER BY \"blobCid\" ASC LIMIT ?");
                params.push(Box::new(limit as i64));
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt
                    .query_map(
                        rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
                        |row| {
                            Ok(ListMissingBlobsRefRecordBlob {
                                cid: row.get(0)?,
                                record_uri: row.get(1)?,
                            })
                        },
                    )?
                    .collect::<Result<Vec<ListMissingBlobsRefRecordBlob>, rusqlite::Error>>()?;
                Ok(rows)
            })
            .await
    }

    pub async fn list_blobs(&self, opts: ListBlobsOpts) -> Result<Vec<String>> {
        let ListBlobsOpts {
            since,
            cursor,
            limit,
        } = opts;
        self.db
            .run(move |conn| {
                let mut sql = String::from("SELECT DISTINCT \"blobCid\" FROM record_blob");
                let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
                if let Some(since) = &since {
                    sql.push_str(
                        " INNER JOIN record ON record.uri = record_blob.\"recordUri\" \
                         WHERE record.\"repoRev\" > ?",
                    );
                    params.push(Box::new(since.clone()));
                } else {
                    sql.push_str(" WHERE 1 = 1");
                }
                if let Some(cursor) = &cursor {
                    sql.push_str(" AND \"blobCid\" > ?");
                    params.push(Box::new(cursor.clone()));
                }
                sql.push_str(" ORDER BY \"blobCid\" ASC LIMIT ?");
                params.push(Box::new(limit as i64));
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt
                    .query_map(
                        rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
                        |row| row.get::<_, String>(0),
                    )?
                    .collect::<Result<Vec<String>, rusqlite::Error>>()?;
                Ok(rows)
            })
            .await
    }

    pub async fn get_blob_takedown_status(&self, cid: Cid) -> Result<Option<StatusAttr>> {
        let res: Option<Option<String>> = self
            .db
            .run(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT \"takedownRef\" FROM blob WHERE cid = ?1",
                        [cid.to_string()],
                        |row| row.get(0),
                    )
                    .optional()?)
            })
            .await?;
        match res {
            None => Ok(None),
            Some(Some(takedown_ref)) => Ok(Some(StatusAttr {
                applied: true,
                r#ref: Some(takedown_ref),
            })),
            Some(None) => Ok(Some(StatusAttr {
                applied: false,
                r#ref: None,
            })),
        }
    }

    // Transactors
    // -------------------

    /// Applies or reverses a takedown. Every decision advances the blob's
    /// moderation version and supersedes any restoration still pending, so
    /// a restoration that resumes after a newer decision cannot clear it.
    ///
    /// Outside coexistence the object is moved to or from quarantine as the
    /// reference implementation does. Under coexistence nothing is moved: a
    /// takedown is the flag alone, and a reversal clears the flag when the
    /// permanent object exists, or restores it by copy from a quarantine the
    /// reference implementation left and clears the flag once the copy is
    /// confirmed (see `run_restore`).
    pub async fn update_blob_takedown_status(&self, blob: Cid, takedown: StatusAttr) -> Result<()> {
        let takedown_ref: Option<String> = match takedown.applied {
            true => match takedown.r#ref {
                Some(takedown_ref) => Some(takedown_ref),
                None => Some(now()),
            },
            false => None,
        };
        let cid = blob.to_string();
        let coexistence = self.coexistence;
        let restore_needed = coexistence
            && !takedown.applied
            && !self.blobstore.has_stored(blob).await?
            && self.blobstore.has_quarantined(blob).await?;
        let flag_now = takedown.applied || !restore_needed;
        let stamp = now();
        self.db
            .tx(move |tx| {
                let version = bump_moderation_version_in(tx, &cid, &stamp)?;
                if flag_now {
                    tx.execute(
                        "UPDATE blob SET \"takedownRef\" = ?1 WHERE cid = ?2",
                        rusqlite::params![takedown_ref, cid],
                    )?;
                } else {
                    insert_blob_work_in(
                        tx,
                        BlobWorkKind::Permanent,
                        &cid,
                        Some(&cid),
                        BlobWorkState::RestorePending,
                        Some(version),
                        &stamp,
                    )?;
                }
                Ok(())
            })
            .await?;
        if coexistence {
            // a restoration, when one was journaled, runs with the store's
            // other blob work
            return Ok(());
        }
        let res = match takedown.applied {
            true => self.blobstore.quarantine(blob).await,
            false => self.blobstore.unquarantine(blob).await,
        };
        if let Err(err) = res {
            tracing::error!(?err, cid = %blob, "could not update blob takedown status in blobstore");
        }
        Ok(())
    }

    /// The blob's moderation version; zero before any decision.
    pub async fn moderation_version(&self, cid: Cid) -> Result<i64> {
        self.db
            .run(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT version FROM blob_moderation WHERE cid = ?1",
                        [cid.to_string()],
                        |row| row.get(0),
                    )
                    .optional()?
                    .unwrap_or(0))
            })
            .await
    }

    /// Restores one taken-down object from quarantine by copy, confirms the
    /// copy, then clears the takedown only if no newer decision was made
    /// since the restoration was requested; the quarantined source stays
    /// for the collector. Every step is safe to repeat after a crash.
    pub async fn run_restore(
        &self,
        work: &BlobWork,
        stop_after: Option<RestoreStep>,
    ) -> Result<()> {
        let cid = Cid::from_str(&work.key)?;
        let version = work
            .version
            .ok_or_else(|| anyhow::anyhow!("restoration {} carries no version", work.id))?;
        self.blobstore.restore_copy_only(cid).await?;
        if stop_after == Some(RestoreStep::Copied) {
            return Ok(());
        }
        anyhow::ensure!(
            self.blobstore.has_stored(cid).await?,
            "restored object {cid} is not readable yet"
        );
        if stop_after == Some(RestoreStep::Confirmed) {
            return Ok(());
        }
        let id = work.id;
        let key = work.key.clone();
        let stamp = now();
        self.db
            .tx(move |tx| {
                let state = match clear_takedown_at_version_in(tx, &key, version)? {
                    true => BlobWorkState::Done,
                    false => BlobWorkState::Superseded,
                };
                set_blob_work_state_in(tx, id, state, &stamp)?;
                if state == BlobWorkState::Done {
                    insert_blob_work_in(
                        tx,
                        BlobWorkKind::Quarantine,
                        &key,
                        Some(&key),
                        BlobWorkState::GcDeferred,
                        None,
                        &stamp,
                    )?;
                }
                Ok(())
            })
            .await
    }
}

pub async fn accepted_mime(mime: String, accepted: Vec<String>) -> bool {
    if accepted.contains(&"*/*".to_owned()) {
        return true;
    }
    let globs = accepted.iter().filter_map(|a| a.strip_suffix("/*"));
    for glob in globs {
        if mime.starts_with(&format!("{glob}/")) {
            return true;
        }
    }
    accepted.contains(&mime)
}

pub async fn verify_blob(blob: &PreparedBlobRef, found: &BlobRow) -> Result<()> {
    if let Some(max_size) = blob.constraints.max_size {
        if found.size as usize > max_size {
            bail!(
                "BlobTooLarge: This file is too large. It is {:?} but the maximum size is {:?}",
                found.size,
                max_size
            )
        }
    }
    if blob.mime_type != found.mime_type {
        bail!("InvalidMimeType: Referenced MimeType does not match stored blob. Expected: {:?}, Got: {:?}",found.mime_type, blob.mime_type)
    }
    if let Some(ref accept) = blob.constraints.accept {
        if !accepted_mime(blob.mime_type.clone(), accept.clone()).await {
            bail!(
                "Wrong type of file. It is {:?} but it must match {:?}.",
                blob.mime_type,
                accept
            )
        }
    }
    Ok(())
}

pub async fn sha256_stream(to_hash: Vec<u8>) -> Result<Vec<u8>> {
    let digest = Sha256::digest(&*to_hash);
    let hash: &[u8] = digest.as_ref();
    Ok(hash.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor_store::blobstore::MemoryBlobStore;
    use crate::actor_store::db::get_migrated_db;
    use rsky_repo::types::{BlobConstraint, PreparedCreateOrUpdate, PreparedDelete, WriteOpAction};

    struct TestBlobReader {
        reader: BlobReader,
        store: Arc<MemoryBlobStore>,
        _dir: tempfile::TempDir,
    }

    async fn test_reader() -> TestBlobReader {
        let dir = tempfile::tempdir().unwrap();
        let db = get_migrated_db(dir.path().join("store.sqlite"))
            .await
            .unwrap();
        let store = Arc::new(MemoryBlobStore::default());
        let reader = BlobReader::new(store.clone(), db, BackgroundQueue::default(), false);
        TestBlobReader {
            reader,
            store,
            _dir: dir,
        }
    }

    async fn upload(t: &TestBlobReader, bytes: &[u8]) -> BlobRef {
        let metadata = t
            .reader
            .upload_blob_and_get_metadata("text/plain".to_owned(), bytes.to_vec())
            .await
            .unwrap();
        t.reader.track_untethered_blob(metadata).await.unwrap()
    }

    fn prepared_ref(blob: &BlobRef) -> PreparedBlobRef {
        PreparedBlobRef {
            cid: blob.get_cid().unwrap(),
            mime_type: blob.get_mime_type().to_string(),
            constraints: BlobConstraint {
                max_size: None,
                accept: None,
            },
        }
    }

    /// An uploaded blob is not served until a record references it and it
    /// has been promoted out of temporary storage, on both readers of a
    /// shared data directory.
    #[tokio::test]
    async fn unreferenced_uploads_are_not_served() {
        let t = test_reader().await;
        let blob = upload(&t, b"pending blob bytes").await;
        let cid = blob.get_cid().unwrap();
        let err = t.reader.get_blob_metadata(cid).await.map(drop).unwrap_err();
        assert_eq!(err.to_string(), "Blob not found");
        assert!(t.reader.get_blob(cid).await.is_err());
        // uploading the same bytes again changes nothing
        let again = upload(&t, b"pending blob bytes").await;
        assert_eq!(again.get_cid().unwrap(), cid);
        assert!(t.reader.get_blob_metadata(cid).await.is_err());
        t.reader
            .verify_blob_and_make_permanent(prepared_ref(&blob))
            .await
            .unwrap();
        assert_eq!(t.reader.get_blob_metadata(cid).await.unwrap().size, 18);
    }

    #[tokio::test]
    async fn upload_track_and_promote_blob() {
        let t = test_reader().await;
        let blob = upload(&t, b"some blob bytes").await;
        let cid = blob.get_cid().unwrap();
        assert_eq!(blob.get_mime_type(), "text/plain");
        assert_eq!(t.reader.blob_count().await.unwrap(), 1);
        // still in temp storage
        assert!(!t.store.has_stored(cid).await.unwrap());

        // re-tracking the same blob refreshes the temp key rather than failing
        let metadata = t
            .reader
            .upload_blob_and_get_metadata("text/plain".to_owned(), b"some blob bytes".to_vec())
            .await
            .unwrap();
        t.reader.track_untethered_blob(metadata).await.unwrap();

        t.reader
            .verify_blob_and_make_permanent(prepared_ref(&blob))
            .await
            .unwrap();
        assert!(t.store.has_stored(cid).await.unwrap());
        let metadata = t.reader.get_blob_metadata(cid).await.unwrap();
        assert_eq!(metadata.size, 15);
        assert_eq!(metadata.mime_type.as_deref(), Some("text/plain"));
        let output = t.reader.get_blob(cid).await.unwrap();
        assert_eq!(
            output.stream.collect().await.unwrap().to_vec(),
            b"some blob bytes"
        );
        // promoting again is a no-op
        t.reader
            .verify_blob_and_make_permanent(prepared_ref(&blob))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn verify_blob_enforces_constraints() {
        let t = test_reader().await;
        let blob = upload(&t, b"constrained").await;
        let mut wrong_mime = prepared_ref(&blob);
        wrong_mime.mime_type = "image/png".to_owned();
        assert!(t
            .reader
            .verify_blob_and_make_permanent(wrong_mime)
            .await
            .is_err());

        let mut too_large = prepared_ref(&blob);
        too_large.constraints.max_size = Some(1);
        assert!(t
            .reader
            .verify_blob_and_make_permanent(too_large)
            .await
            .is_err());

        let mut wrong_accept = prepared_ref(&blob);
        wrong_accept.constraints.accept = Some(vec!["image/*".to_owned()]);
        assert!(t
            .reader
            .verify_blob_and_make_permanent(wrong_accept)
            .await
            .is_err());

        let mut accept_any = prepared_ref(&blob);
        accept_any.constraints.accept = Some(vec!["*/*".to_owned()]);
        t.reader
            .verify_blob_and_make_permanent(accept_any)
            .await
            .unwrap();

        let missing = PreparedBlobRef {
            cid: sha256_to_cid(Sha256::digest(b"missing").to_vec()),
            mime_type: "text/plain".to_owned(),
            constraints: BlobConstraint {
                max_size: None,
                accept: None,
            },
        };
        assert!(t
            .reader
            .verify_blob_and_make_permanent(missing)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn accepted_mime_globs() {
        assert!(accepted_mime("image/png".to_owned(), vec!["*/*".to_owned()]).await);
        assert!(accepted_mime("image/png".to_owned(), vec!["image/*".to_owned()]).await);
        assert!(!accepted_mime("text/plain".to_owned(), vec!["image/*".to_owned()]).await);
        assert!(accepted_mime("text/plain".to_owned(), vec!["text/plain".to_owned()]).await);
        assert!(!accepted_mime("text/plain".to_owned(), vec!["image/png".to_owned()]).await);
    }

    #[tokio::test]
    async fn associates_blobs_with_records() {
        let t = test_reader().await;
        let blob = upload(&t, b"blob for record").await;
        let cid = blob.get_cid().unwrap();
        let record_uri = "at://did:example:alice/app.bsky.feed.post/3jt5vlkoraa2a";
        t.reader
            .associate_blob(prepared_ref(&blob), record_uri.to_owned())
            .await
            .unwrap();
        // idempotent
        t.reader
            .associate_blob(prepared_ref(&blob), record_uri.to_owned())
            .await
            .unwrap();
        assert_eq!(
            t.reader.get_records_for_blob(cid).await.unwrap(),
            [record_uri]
        );
        assert_eq!(t.reader.record_blob_count().await.unwrap(), 1);
        assert_eq!(t.reader.get_blob_cids().await.unwrap(), [cid]);
    }

    #[tokio::test]
    async fn takedown_lifecycle() {
        let t = test_reader().await;
        let blob = upload(&t, b"takedown me").await;
        let cid = blob.get_cid().unwrap();
        t.reader
            .verify_blob_and_make_permanent(prepared_ref(&blob))
            .await
            .unwrap();

        let status = t
            .reader
            .get_blob_takedown_status(cid)
            .await
            .unwrap()
            .unwrap();
        assert!(!status.applied && status.r#ref.is_none());
        t.reader
            .update_blob_takedown_status(
                cid,
                StatusAttr {
                    applied: true,
                    r#ref: Some("ref-1".to_owned()),
                },
            )
            .await
            .unwrap();
        assert!(t.store.has_quarantined(&cid));
        let status = t
            .reader
            .get_blob_takedown_status(cid)
            .await
            .unwrap()
            .unwrap();
        assert!(status.applied);
        assert_eq!(status.r#ref.as_deref(), Some("ref-1"));
        // metadata is hidden while taken down
        assert!(t.reader.get_blob_metadata(cid).await.is_err());
        assert!(t.reader.get_blob(cid).await.is_err());
        // taken-down blob cannot be re-uploaded
        let metadata = t
            .reader
            .upload_blob_and_get_metadata("text/plain".to_owned(), b"takedown me".to_vec())
            .await
            .unwrap();
        assert!(t.reader.track_untethered_blob(metadata).await.is_err());

        // takedown without a ref defaults to a timestamp
        t.reader
            .update_blob_takedown_status(
                cid,
                StatusAttr {
                    applied: false,
                    r#ref: None,
                },
            )
            .await
            .unwrap();
        assert!(!t.store.has_quarantined(&cid));
        t.reader
            .update_blob_takedown_status(
                cid,
                StatusAttr {
                    applied: true,
                    r#ref: None,
                },
            )
            .await
            .unwrap();
        let status = t
            .reader
            .get_blob_takedown_status(cid)
            .await
            .unwrap()
            .unwrap();
        assert!(status.applied && status.r#ref.is_some());
        // unknown blob has no status
        let unknown = sha256_to_cid(Sha256::digest(b"unknown").to_vec());
        assert!(t
            .reader
            .get_blob_takedown_status(unknown)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn lists_blobs_and_missing_blobs() {
        let t = test_reader().await;
        let blob = upload(&t, b"listed blob").await;
        let cid = blob.get_cid().unwrap();
        let record_uri = "at://did:example:alice/app.bsky.feed.post/3jt5vlkoraa2a";
        t.reader
            .associate_blob(prepared_ref(&blob), record_uri.to_owned())
            .await
            .unwrap();
        // record row for the `since` join
        t.reader
            .db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO record (uri, cid, collection, rkey, \"repoRev\", \"indexedAt\") \
                     VALUES (?1, 'bafyfake', 'app.bsky.feed.post', '3jt5vlkoraa2a', 'rev-2', 'now')",
                    [record_uri],
                )?;
                Ok(())
            })
            .await
            .unwrap();

        let all = t
            .reader
            .list_blobs(ListBlobsOpts {
                since: None,
                cursor: None,
                limit: 10,
            })
            .await
            .unwrap();
        assert_eq!(all, [cid.to_string()]);
        let since = t
            .reader
            .list_blobs(ListBlobsOpts {
                since: Some("rev-1".to_owned()),
                cursor: None,
                limit: 10,
            })
            .await
            .unwrap();
        assert_eq!(since, [cid.to_string()]);
        let since_after = t
            .reader
            .list_blobs(ListBlobsOpts {
                since: Some("rev-2".to_owned()),
                cursor: None,
                limit: 10,
            })
            .await
            .unwrap();
        assert!(since_after.is_empty());
        let cursored = t
            .reader
            .list_blobs(ListBlobsOpts {
                since: None,
                cursor: Some(cid.to_string()),
                limit: 10,
            })
            .await
            .unwrap();
        assert!(cursored.is_empty());

        // a record_blob row without a blob row is a missing blob
        let missing_uri = "at://did:example:alice/app.bsky.feed.post/3jt5vlkorbb2b";
        t.reader
            .db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO record_blob (\"blobCid\", \"recordUri\") VALUES ('bafymissing', ?1)",
                    [missing_uri],
                )?;
                Ok(())
            })
            .await
            .unwrap();
        let missing = t
            .reader
            .list_missing_blobs(ListMissingBlobsOpts {
                cursor: None,
                limit: 10,
            })
            .await
            .unwrap();
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].cid, "bafymissing");
        assert_eq!(missing[0].record_uri, missing_uri);
        let missing_cursored = t
            .reader
            .list_missing_blobs(ListMissingBlobsOpts {
                cursor: Some("bafymissing".to_owned()),
                limit: 10,
            })
            .await
            .unwrap();
        assert!(missing_cursored.is_empty());
        assert!(t
            .reader
            .list_missing_blobs(ListMissingBlobsOpts {
                cursor: None,
                limit: 1001,
            })
            .await
            .is_err());
    }

    #[test]
    fn blob_work_states_are_all_classified_and_round_trip() {
        for state in BlobWorkState::ALL {
            assert_eq!(BlobWorkState::parse(state.as_str()).unwrap(), state);
            // every variant answers the terminal question without panicking
            let _ = state.is_terminal();
        }
        assert!(!BlobWorkState::DeletePending.is_terminal());
        assert!(!BlobWorkState::RestorePending.is_terminal());
        assert!(BlobWorkState::GcDeferred.is_terminal());
        assert!(BlobWorkState::Superseded.is_terminal());
        assert!(BlobWorkState::Done.is_terminal());
        assert!(BlobWorkState::parse("promote-someday").is_err());
        for kind in [
            BlobWorkKind::Permanent,
            BlobWorkKind::Temp,
            BlobWorkKind::Quarantine,
        ] {
            assert_eq!(BlobWorkKind::parse(kind.as_str()).unwrap(), kind);
        }
        assert!(BlobWorkKind::parse("generation").is_err());
    }

    fn post_with(uri: &str, blob: &BlobRef) -> PreparedWrite {
        PreparedWrite::Create(PreparedCreateOrUpdate {
            action: WriteOpAction::Create,
            uri: uri.to_owned(),
            cid: blob.get_cid().unwrap(),
            swap_cid: None,
            record: serde_json::from_value(serde_json::json!({
                "$type": "app.bsky.feed.post",
                "text": "with blob",
                "createdAt": "2023-01-01T00:00:00.000Z",
            }))
            .unwrap(),
            blobs: vec![prepared_ref(blob)],
        })
    }

    fn takedown(applied: bool) -> StatusAttr {
        StatusAttr {
            applied,
            r#ref: Some("mod-ref".to_owned()),
        }
    }

    /// Under coexistence nothing is ever deleted or moved: promotion copies
    /// and journals the temporary object, a takedown is a flag, and a
    /// reversal with the permanent object present clears the flag alone.
    #[tokio::test]
    async fn coexistence_never_deletes_or_moves_an_object() {
        let mut t = test_reader().await;
        t.reader.coexistence = true;
        let blob = upload(&t, b"shared object").await;
        let cid = blob.get_cid().unwrap();
        let uri = "at://did:example:alice/app.bsky.feed.post/3jt5vlkoraa2a";
        t.reader
            .process_write_blobs(vec![post_with(uri, &blob)])
            .await
            .unwrap();
        t.reader.background_queue.process_all().await;
        assert!(t.store.has_stored(cid).await.unwrap());
        let work = t.reader.blob_work().await.unwrap();
        assert_eq!(work.len(), 1);
        assert_eq!(work[0].kind, BlobWorkKind::Temp);
        assert_eq!(work[0].state, BlobWorkState::GcDeferred);
        assert!(t.store.has_temp(&work[0].key), "the temporary object stays");
        assert_eq!(work[0].cid.as_deref(), Some(cid.to_string().as_str()));
        assert!(t.reader.get_blob_metadata(cid).await.is_ok());

        // a re-upload of the same bytes leaves a temporary object nothing
        // will reference; it is journaled for the collector
        let again = upload(&t, b"shared object").await;
        assert_eq!(again.get_cid().unwrap(), cid);
        t.reader.background_queue.process_all().await;
        assert!(t.reader.get_blob_metadata(cid).await.is_ok());
        let work = t.reader.blob_work().await.unwrap();
        assert_eq!(work.len(), 2);
        assert_eq!(work[1].kind, BlobWorkKind::Temp);
        assert!(t.store.has_temp(&work[1].key));

        // takedown: flag only, object untouched, version advanced
        t.reader
            .update_blob_takedown_status(cid, takedown(true))
            .await
            .unwrap();
        assert!(t.reader.get_blob_metadata(cid).await.is_err());
        assert!(t.store.has_stored(cid).await.unwrap());
        assert!(!t.store.has_quarantined(&cid));
        assert_eq!(t.reader.moderation_version(cid).await.unwrap(), 1);
        // reversal with the permanent object present
        t.reader
            .update_blob_takedown_status(cid, takedown(false))
            .await
            .unwrap();
        t.reader.queue_blob_work();
        t.reader.background_queue.process_all().await;
        assert!(t.reader.get_blob_metadata(cid).await.is_ok());
        assert_eq!(t.reader.moderation_version(cid).await.unwrap(), 2);

        // dereferencing journals the permanent object instead of deleting it
        t.reader
            .process_write_blobs(vec![PreparedWrite::Delete(PreparedDelete {
                action: WriteOpAction::Delete,
                uri: uri.to_owned(),
                swap_cid: None,
            })])
            .await
            .unwrap();
        t.reader.background_queue.process_all().await;
        assert!(t.store.has_stored(cid).await.unwrap());
        assert_eq!(t.reader.nonterminal_blob_work().await.unwrap(), 0);
        assert_eq!(t.store.destructive_calls(), 0);
    }

    /// A takedown the reference implementation applied moved the object to
    /// quarantine; reversing it here restores the object by copy, confirms
    /// the copy, and only then clears the flag. Each step resumes after a
    /// crash, and the quarantined source is kept for the collector.
    #[tokio::test]
    async fn reversal_restores_a_reference_quarantine_by_copy() {
        let mut t = test_reader().await;
        t.reader.coexistence = true;
        let blob = upload(&t, b"quarantined by the reference").await;
        let cid = blob.get_cid().unwrap();
        // the reference's takedown: flag set, permanent object moved away
        t.reader
            .db
            .run(move |conn| {
                conn.execute(
                    "UPDATE blob SET \"takedownRef\" = 'ts-ref', \"tempKey\" = NULL WHERE cid = ?1",
                    [cid.to_string()],
                )?;
                Ok(())
            })
            .await
            .unwrap();
        t.store
            .put_quarantined(cid, b"quarantined by the reference".to_vec());
        assert!(!t.store.has_stored(cid).await.unwrap());

        t.reader
            .update_blob_takedown_status(cid, takedown(false))
            .await
            .unwrap();
        // the flag is not cleared until the object is back
        assert!(t.reader.get_blob_metadata(cid).await.is_err());
        let row = t.reader.blob_work().await.unwrap().remove(0);
        assert_eq!(row.kind, BlobWorkKind::Permanent);
        assert_eq!(row.state, BlobWorkState::RestorePending);
        assert_eq!(row.version, Some(1));
        assert_eq!(t.reader.nonterminal_blob_work().await.unwrap(), 1);
        // crash after the copy: the object is back, the flag is not
        t.reader
            .run_restore(&row, Some(RestoreStep::Copied))
            .await
            .unwrap();
        assert!(t.store.has_stored(cid).await.unwrap());
        assert!(t.reader.get_blob_metadata(cid).await.is_err());
        // crash after the confirmation: same
        t.reader
            .run_restore(&row, Some(RestoreStep::Confirmed))
            .await
            .unwrap();
        assert!(t.reader.get_blob_metadata(cid).await.is_err());
        // the worker finishes what was left
        t.reader.queue_blob_work();
        t.reader.background_queue.process_all().await;
        assert!(t.reader.get_blob_metadata(cid).await.is_ok());
        assert!(t.store.has_stored(cid).await.unwrap());
        assert!(t.store.has_quarantined(&cid), "the source is retained");
        let work = t.reader.blob_work().await.unwrap();
        assert_eq!(work[0].state, BlobWorkState::Done);
        assert_eq!(work[1].kind, BlobWorkKind::Quarantine);
        assert_eq!(work[1].state, BlobWorkState::GcDeferred);
        assert_eq!(t.reader.nonterminal_blob_work().await.unwrap(), 0);
        assert_eq!(t.store.destructive_calls(), 0);

        // a reversal with neither object present clears the flag, as the
        // reference does when there is nothing to move back
        let ghost = sha256_to_cid(Sha256::digest(b"ghost").to_vec());
        t.reader
            .db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO blob (cid, \"mimeType\", size, \"createdAt\", \"takedownRef\") \
                     VALUES (?1, 'text/plain', 5, '2024-01-01T00:00:00.000Z', 'ref')",
                    [ghost.to_string()],
                )?;
                Ok(())
            })
            .await
            .unwrap();
        t.reader
            .update_blob_takedown_status(ghost, takedown(false))
            .await
            .unwrap();
        assert!(t
            .reader
            .get_blob_takedown_status(ghost)
            .await
            .unwrap()
            .map(|status| !status.applied)
            .unwrap_or(false));
    }

    /// Takedown, restoration copies then crashes, restriction cleared,
    /// takedown again, restoration resumes last: the resumed restoration
    /// finds a newer version and ends superseded, and the newer takedown
    /// stands.
    #[tokio::test]
    async fn a_resumed_restoration_never_clears_a_newer_takedown() {
        let mut t = test_reader().await;
        t.reader.coexistence = true;
        let blob = upload(&t, b"aba").await;
        let cid = blob.get_cid().unwrap();
        t.reader
            .db
            .run(move |conn| {
                conn.execute(
                    "UPDATE blob SET \"takedownRef\" = 'ts-ref', \"tempKey\" = NULL WHERE cid = ?1",
                    [cid.to_string()],
                )?;
                Ok(())
            })
            .await
            .unwrap();
        t.store.put_quarantined(cid, b"aba".to_vec());

        // request the restoration; the worker copies and then crashes
        t.reader
            .update_blob_takedown_status(cid, takedown(false))
            .await
            .unwrap();
        let row = t.reader.blob_work().await.unwrap().remove(0);
        assert_eq!(row.version, Some(1));
        assert_eq!(row.state, BlobWorkState::RestorePending);
        t.reader
            .run_restore(&row, Some(RestoreStep::Copied))
            .await
            .unwrap();

        // a newer takedown lands while the restoration is pending
        t.reader
            .update_blob_takedown_status(cid, takedown(true))
            .await
            .unwrap();
        assert_eq!(t.reader.moderation_version(cid).await.unwrap(), 2);
        let superseded = t.reader.blob_work().await.unwrap().remove(0);
        assert_eq!(superseded.state, BlobWorkState::Superseded);

        // the crashed restoration resumes with its stale version
        t.reader.run_restore(&row, None).await.unwrap();
        assert!(t.reader.get_blob_metadata(cid).await.is_err());
        assert_eq!(
            t.reader.blob_work().await.unwrap()[0].state,
            BlobWorkState::Superseded
        );
        assert_eq!(t.reader.blob_work().await.unwrap().len(), 1);
        // a row without a version cannot be restored
        let bad = BlobWork {
            version: None,
            ..row
        };
        assert!(t.reader.run_restore(&bad, None).await.is_err());
        assert_eq!(t.store.destructive_calls(), 0);
    }

    /// The reference behaviour stays when this server owns the objects.
    #[tokio::test]
    async fn takedown_moves_objects_when_not_coexisting() {
        let t = test_reader().await;
        let blob = upload(&t, b"owned object").await;
        let cid = blob.get_cid().unwrap();
        t.reader
            .verify_blob_and_make_permanent(prepared_ref(&blob))
            .await
            .unwrap();
        t.reader
            .update_blob_takedown_status(cid, takedown(true))
            .await
            .unwrap();
        assert!(t.store.has_quarantined(&cid));
        assert_eq!(t.reader.moderation_version(cid).await.unwrap(), 1);
        t.reader
            .update_blob_takedown_status(cid, takedown(false))
            .await
            .unwrap();
        assert!(t.store.has_stored(cid).await.unwrap());
        assert!(t.reader.blob_work().await.unwrap().is_empty());
    }

    /// While another implementation may still serve the store's objects,
    /// dereferencing a blob journals it instead of deleting it.
    #[tokio::test]
    async fn coexistence_defers_dereferenced_blobs_instead_of_deleting() {
        let mut t = test_reader().await;
        t.reader.coexistence = true;
        let blob = upload(&t, b"keep me around").await;
        let cid = blob.get_cid().unwrap();
        let record_uri = "at://did:example:alice/app.bsky.feed.post/3jt5vlkoraa2a".to_owned();
        let create = PreparedWrite::Create(PreparedCreateOrUpdate {
            action: WriteOpAction::Create,
            uri: record_uri.clone(),
            cid,
            swap_cid: None,
            record: serde_json::from_value(serde_json::json!({
                "$type": "app.bsky.feed.post",
                "text": "with blob",
                "createdAt": "2023-01-01T00:00:00.000Z",
            }))
            .unwrap(),
            blobs: vec![prepared_ref(&blob)],
        });
        t.reader.process_write_blobs(vec![create]).await.unwrap();
        let delete = PreparedWrite::Delete(PreparedDelete {
            action: WriteOpAction::Delete,
            uri: record_uri,
            swap_cid: None,
        });
        t.reader.process_write_blobs(vec![delete]).await.unwrap();
        t.reader.background_queue.process_all().await;
        assert_eq!(t.reader.blob_count().await.unwrap(), 0);
        assert!(t.store.has_stored(cid).await.unwrap());
        let work = t.reader.blob_work().await.unwrap();
        assert_eq!(work.len(), 2);
        assert_eq!(work[0].kind, BlobWorkKind::Temp);
        assert_eq!(work[1].kind, BlobWorkKind::Permanent);
        assert_eq!(work[1].key, cid.to_string());
        assert_eq!(work[1].state, BlobWorkState::GcDeferred);
        assert_eq!(t.reader.nonterminal_blob_work().await.unwrap(), 0);
    }

    /// Outside coexistence the deletion is journaled first and runs after
    /// the write's transaction; a failing store leaves the row pending.
    #[tokio::test]
    async fn dereferenced_blob_deletion_is_journaled() {
        let t = test_reader().await;
        let blob = upload(&t, b"journal me").await;
        let cid = blob.get_cid().unwrap();
        let record_uri = "at://did:example:alice/app.bsky.feed.post/3jt5vlkoraa2a".to_owned();
        let create = PreparedWrite::Create(PreparedCreateOrUpdate {
            action: WriteOpAction::Create,
            uri: record_uri.clone(),
            cid,
            swap_cid: None,
            record: serde_json::from_value(serde_json::json!({
                "$type": "app.bsky.feed.post",
                "text": "with blob",
                "createdAt": "2023-01-01T00:00:00.000Z",
            }))
            .unwrap(),
            blobs: vec![prepared_ref(&blob)],
        });
        t.reader.process_write_blobs(vec![create]).await.unwrap();
        t.reader.background_queue.process_all().await;
        let delete = PreparedWrite::Delete(PreparedDelete {
            action: WriteOpAction::Delete,
            uri: record_uri,
            swap_cid: None,
        });
        let promoted = t
            .reader
            .promote_write_blobs(&[delete.clone()], false)
            .await
            .unwrap();
        assert!(promoted.is_empty());
        let writes = vec![delete];
        t.reader
            .db
            .tx(move |tx| apply_write_blobs_in(tx, &writes, &[], false, "2024-01-01T00:00:00.000Z"))
            .await
            .unwrap();
        let work = t.reader.blob_work().await.unwrap();
        assert_eq!(work[0].state, BlobWorkState::DeletePending);
        assert_eq!(t.reader.nonterminal_blob_work().await.unwrap(), 1);
        assert!(t.store.has_stored(cid).await.unwrap());

        t.reader.run_blob_work().await.unwrap();
        assert!(!t.store.has_stored(cid).await.unwrap());
        assert_eq!(
            t.reader.blob_work().await.unwrap()[0].state,
            BlobWorkState::Done
        );
        assert_eq!(t.reader.nonterminal_blob_work().await.unwrap(), 0);
        // nothing pending is a no-op
        t.reader.run_blob_work().await.unwrap();
    }

    #[tokio::test]
    async fn promotion_requires_the_blob_unless_tolerated() {
        let t = test_reader().await;
        let missing = PreparedBlobRef {
            cid: sha256_to_cid(Sha256::digest(b"never uploaded").to_vec()),
            mime_type: "text/plain".to_owned(),
            constraints: BlobConstraint {
                max_size: None,
                accept: None,
            },
        };
        let write = PreparedWrite::Create(PreparedCreateOrUpdate {
            action: WriteOpAction::Create,
            uri: "at://did:example:alice/app.bsky.feed.post/3jt5vlkoraa2a".to_owned(),
            cid: missing.cid,
            swap_cid: None,
            record: serde_json::from_value(serde_json::json!({
                "$type": "app.bsky.feed.post",
                "text": "with missing blob",
                "createdAt": "2023-01-01T00:00:00.000Z",
            }))
            .unwrap(),
            blobs: vec![missing],
        });
        let err = t
            .reader
            .promote_write_blobs(std::slice::from_ref(&write), false)
            .await
            .unwrap_err();
        assert!(err.to_string().starts_with("Could not find blob"));
        assert!(t
            .reader
            .promote_write_blobs(&[write], true)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn process_write_blobs_deletes_dereferenced() {
        let t = test_reader().await;
        let blob = upload(&t, b"dereference me").await;
        let cid = blob.get_cid().unwrap();
        let record_uri = "at://did:example:alice/app.bsky.feed.post/3jt5vlkoraa2a".to_owned();

        let create = PreparedWrite::Create(PreparedCreateOrUpdate {
            action: WriteOpAction::Create,
            uri: record_uri.clone(),
            cid,
            swap_cid: None,
            record: serde_json::from_value(serde_json::json!({
                "$type": "app.bsky.feed.post",
                "text": "with blob",
                "createdAt": "2023-01-01T00:00:00.000Z",
            }))
            .unwrap(),
            blobs: vec![prepared_ref(&blob)],
        });
        t.reader.process_write_blobs(vec![create]).await.unwrap();
        assert!(t.store.has_stored(cid).await.unwrap());
        assert_eq!(
            t.reader.get_records_for_blob(cid).await.unwrap(),
            [record_uri.clone()]
        );

        // deleting the record dereferences and deletes the blob
        let delete = PreparedWrite::Delete(PreparedDelete {
            action: WriteOpAction::Delete,
            uri: record_uri.clone(),
            swap_cid: None,
        });
        t.reader.process_write_blobs(vec![delete]).await.unwrap();
        t.reader.background_queue.process_all().await;
        assert_eq!(t.reader.blob_count().await.unwrap(), 0);
        assert!(!t.store.has_stored(cid).await.unwrap());
        assert!(t.reader.get_records_for_blob(cid).await.unwrap().is_empty());

        // deleting a record with no blobs is a no-op
        let delete_again = PreparedWrite::Delete(PreparedDelete {
            action: WriteOpAction::Delete,
            uri: record_uri,
            swap_cid: None,
        });
        t.reader
            .process_write_blobs(vec![delete_again])
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn process_write_blobs_handles_updates() {
        let t = test_reader().await;
        let old_blob = upload(&t, b"old media").await;
        let old_cid = old_blob.get_cid().unwrap();
        let record_uri = "at://did:example:alice/app.bsky.feed.post/3jt5vlkoraa2a".to_owned();
        t.reader
            .verify_blob_and_make_permanent(prepared_ref(&old_blob))
            .await
            .unwrap();
        t.reader
            .associate_blob(prepared_ref(&old_blob), record_uri.clone())
            .await
            .unwrap();

        let new_blob = upload(&t, b"new media").await;
        let new_cid = new_blob.get_cid().unwrap();
        let update = PreparedWrite::Update(PreparedCreateOrUpdate {
            action: WriteOpAction::Update,
            uri: record_uri.clone(),
            cid: new_cid,
            swap_cid: None,
            record: serde_json::from_value(serde_json::json!({
                "$type": "app.bsky.feed.post",
                "text": "updated",
                "createdAt": "2023-01-01T00:00:00.000Z",
            }))
            .unwrap(),
            blobs: vec![prepared_ref(&new_blob)],
        });
        t.reader.process_write_blobs(vec![update]).await.unwrap();
        t.reader.background_queue.process_all().await;

        // old blob dereferenced and deleted, new blob permanent and associated
        assert!(!t.store.has_stored(old_cid).await.unwrap());
        assert!(t.store.has_stored(new_cid).await.unwrap());
        assert_eq!(
            t.reader.get_records_for_blob(new_cid).await.unwrap(),
            [record_uri]
        );
        assert_eq!(t.reader.blob_count().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn takedown_logs_missing_blobstore_entry() {
        let t = test_reader().await;
        // row exists but blob bytes were never promoted to storage
        let blob = upload(&t, b"row only").await;
        let cid = blob.get_cid().unwrap();
        t.reader
            .update_blob_takedown_status(
                cid,
                StatusAttr {
                    applied: true,
                    r#ref: Some("ref-x".to_owned()),
                },
            )
            .await
            .unwrap();
        let status = t
            .reader
            .get_blob_takedown_status(cid)
            .await
            .unwrap()
            .unwrap();
        assert!(status.applied);
    }

    #[tokio::test]
    async fn verify_blob_accepts_within_max_size() {
        let t = test_reader().await;
        let blob = upload(&t, b"small").await;
        let mut sized = prepared_ref(&blob);
        sized.constraints.max_size = Some(1024);
        t.reader
            .verify_blob_and_make_permanent(sized)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn mixed_delete_and_create_keeps_new_blobs() {
        let t = test_reader().await;
        let old_blob = upload(&t, b"replaced media").await;
        let old_uri = "at://did:example:alice/app.bsky.feed.post/3jt5vlkoraa2a".to_owned();
        t.reader
            .verify_blob_and_make_permanent(prepared_ref(&old_blob))
            .await
            .unwrap();
        t.reader
            .associate_blob(prepared_ref(&old_blob), old_uri.clone())
            .await
            .unwrap();

        // re-upload the same bytes for a new record while deleting the old one
        let new_blob = upload(&t, b"replaced media").await;
        let new_uri = "at://did:example:alice/app.bsky.feed.post/3jt5vlkorbb2b".to_owned();
        let delete = PreparedWrite::Delete(PreparedDelete {
            action: WriteOpAction::Delete,
            uri: old_uri,
            swap_cid: None,
        });
        let create = PreparedWrite::Create(PreparedCreateOrUpdate {
            action: WriteOpAction::Create,
            uri: new_uri.clone(),
            cid: new_blob.get_cid().unwrap(),
            swap_cid: None,
            record: serde_json::from_value(serde_json::json!({
                "$type": "app.bsky.feed.post",
                "text": "recreated",
                "createdAt": "2023-01-01T00:00:00.000Z",
            }))
            .unwrap(),
            blobs: vec![prepared_ref(&new_blob)],
        });
        t.reader
            .process_write_blobs(vec![delete, create])
            .await
            .unwrap();
        t.reader.background_queue.process_all().await;
        // blob is kept because the create in the same commit references it
        let cid = new_blob.get_cid().unwrap();
        assert_eq!(t.reader.blob_count().await.unwrap(), 1);
        assert!(t.store.has_stored(cid).await.unwrap());
        assert_eq!(t.reader.get_records_for_blob(cid).await.unwrap(), [new_uri]);
    }

    #[tokio::test]
    async fn keeps_blobs_still_referenced_elsewhere() {
        let t = test_reader().await;
        let blob = upload(&t, b"shared blob").await;
        let cid = blob.get_cid().unwrap();
        let uri_one = "at://did:example:alice/app.bsky.feed.post/3jt5vlkoraa2a".to_owned();
        let uri_two = "at://did:example:alice/app.bsky.feed.post/3jt5vlkorbb2b".to_owned();
        t.reader
            .verify_blob_and_make_permanent(prepared_ref(&blob))
            .await
            .unwrap();
        t.reader
            .associate_blob(prepared_ref(&blob), uri_one.clone())
            .await
            .unwrap();
        t.reader
            .associate_blob(prepared_ref(&blob), uri_two)
            .await
            .unwrap();

        let delete = PreparedWrite::Delete(PreparedDelete {
            action: WriteOpAction::Delete,
            uri: uri_one,
            swap_cid: None,
        });
        t.reader.process_write_blobs(vec![delete]).await.unwrap();
        t.reader.background_queue.process_all().await;
        // still referenced by uri_two, so blob row and bytes remain
        assert_eq!(t.reader.blob_count().await.unwrap(), 1);
        assert!(t.store.has_stored(cid).await.unwrap());
    }
}
