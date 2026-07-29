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
use rsky_repo::types::{PreparedBlobRef, PreparedWrite};
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
    ) -> Self {
        BlobReader {
            blobstore,
            db,
            background_queue,
        }
    }

    pub async fn get_blob_metadata(&self, cid: Cid) -> Result<GetBlobMetadataOutput> {
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
        match found {
            None => bail!("Blob not found"),
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
                        temp_key,
                        width,
                        height,
                        now()
                    ],
                )?;
                Ok(())
            })
            .await?;
        Ok(BlobRef::new(cid, mime_type, size, None))
    }

    pub async fn process_write_blobs(&self, writes: Vec<PreparedWrite>) -> Result<()> {
        self.delete_dereferenced_blobs(writes.clone()).await?;
        for write in writes {
            match write {
                PreparedWrite::Create(w) => {
                    for blob in w.blobs {
                        self.verify_blob_and_make_permanent(blob.clone()).await?;
                        self.associate_blob(blob, w.uri.clone()).await?;
                    }
                }
                PreparedWrite::Update(w) => {
                    for blob in w.blobs {
                        self.verify_blob_and_make_permanent(blob.clone()).await?;
                        self.associate_blob(blob, w.uri.clone()).await?;
                    }
                }
                _ => (),
            }
        }
        Ok(())
    }

    pub async fn delete_dereferenced_blobs(&self, writes: Vec<PreparedWrite>) -> Result<()> {
        let uris: Vec<String> = writes
            .clone()
            .into_iter()
            .filter_map(|w| match w {
                PreparedWrite::Delete(w) => Some(w.uri),
                PreparedWrite::Update(w) => Some(w.uri),
                _ => None,
            })
            .collect();
        if uris.is_empty() {
            return Ok(());
        }

        let deleted_repo_blob_cids: Vec<String> = self
            .db
            .run(move |conn| {
                let sql = format!(
                    "DELETE FROM record_blob WHERE \"recordUri\" IN ({}) RETURNING \"blobCid\"",
                    placeholders(uris.len())
                );
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt
                    .query_map(rusqlite::params_from_iter(uris.iter()), |row| {
                        row.get::<_, String>(0)
                    })?
                    .collect::<Result<Vec<String>, rusqlite::Error>>()?;
                Ok(rows)
            })
            .await?;
        if deleted_repo_blob_cids.is_empty() {
            return Ok(());
        }

        let cids_for_lookup = deleted_repo_blob_cids.clone();
        let mut duplicated_cids: Vec<String> = self
            .db
            .run(move |conn| {
                let sql = format!(
                    "SELECT \"blobCid\" FROM record_blob WHERE \"blobCid\" IN ({})",
                    placeholders(cids_for_lookup.len())
                );
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt
                    .query_map(rusqlite::params_from_iter(cids_for_lookup.iter()), |row| {
                        row.get::<_, String>(0)
                    })?
                    .collect::<Result<Vec<String>, rusqlite::Error>>()?;
                Ok(rows)
            })
            .await?;

        let mut new_blob_cids: Vec<String> = writes
            .into_iter()
            .flat_map(|w| match w {
                PreparedWrite::Create(w) => w.blobs,
                PreparedWrite::Update(w) => w.blobs,
                PreparedWrite::Delete(_) => Vec::new(),
            })
            .map(|b| b.cid.to_string())
            .collect();
        let mut cids_to_keep = Vec::new();
        cids_to_keep.append(&mut new_blob_cids);
        cids_to_keep.append(&mut duplicated_cids);

        let cids_to_delete = deleted_repo_blob_cids
            .into_iter()
            .filter(|cid| !cids_to_keep.contains(cid))
            .collect::<Vec<String>>();
        if cids_to_delete.is_empty() {
            return Ok(());
        }

        let cids_for_delete = cids_to_delete.clone();
        self.db
            .run(move |conn| {
                let sql = format!(
                    "DELETE FROM blob WHERE cid IN ({})",
                    placeholders(cids_for_delete.len())
                );
                conn.execute(&sql, rusqlite::params_from_iter(cids_for_delete.iter()))?;
                Ok(())
            })
            .await?;

        let blobstore = self.blobstore.clone();
        self.background_queue.add(async move {
            let cids = cids_to_delete
                .into_iter()
                .map(|cid| Cid::from_str(&cid).map_err(anyhow::Error::new))
                .collect::<Result<Vec<Cid>>>()?;
            blobstore.delete_many(cids).await
        });
        Ok(())
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
                self.blobstore
                    .make_permanent(temp_key.clone(), blob.cid)
                    .await?;
                self.db
                    .run(move |conn| {
                        conn.execute(
                            "UPDATE blob SET \"tempKey\" = NULL WHERE \"tempKey\" = ?1",
                            [temp_key.clone()],
                        )?;
                        Ok(())
                    })
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

    pub async fn update_blob_takedown_status(&self, blob: Cid, takedown: StatusAttr) -> Result<()> {
        let takedown_ref: Option<String> = match takedown.applied {
            true => match takedown.r#ref {
                Some(takedown_ref) => Some(takedown_ref),
                None => Some(now()),
            },
            false => None,
        };
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE blob SET \"takedownRef\" = ?1 WHERE cid = ?2",
                    rusqlite::params![takedown_ref, blob.to_string()],
                )?;
                Ok(())
            })
            .await?;

        let res = match takedown.applied {
            true => self.blobstore.quarantine(blob).await,
            false => self.blobstore.unquarantine(blob).await,
        };
        if let Err(err) = res {
            tracing::error!(?err, cid = %blob, "could not update blob takedown status in blobstore");
        }
        Ok(())
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
        let reader = BlobReader::new(store.clone(), db, BackgroundQueue::default());
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
