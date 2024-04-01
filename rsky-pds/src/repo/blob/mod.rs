use crate::db::establish_connection;
use crate::models::models;
use crate::repo::aws::s3::S3BlobStore;
use crate::repo::types::{PreparedBlobRef, PreparedWrite};
use anyhow::{bail, Result};
use diesel::dsl::count_distinct;
use diesel::*;
use futures::stream::{self, StreamExt};
use rocket::form::validate::Contains;

pub struct BlobReader {
    pub blobstore: S3BlobStore,
    pub did: String,
}

// Basically handles getting lexicon records from db
impl BlobReader {
    pub fn new(blobstore: S3BlobStore) -> Self {
        BlobReader {
            did: blobstore.bucket.clone(),
            blobstore,
        }
    }

    pub async fn process_writes_blob(&self, writes: Vec<PreparedWrite>) -> Result<()> {
        self.delete_dereferenced_blobs(writes.clone()).await?;
        let _ = stream::iter(writes)
            .then(|write| async move {
                Ok::<(), anyhow::Error>(match write {
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
                })
            })
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
        Ok(())
    }

    pub async fn delete_dereferenced_blobs(&self, writes: Vec<PreparedWrite>) -> Result<()> {
        use crate::schema::pds::blob::dsl as BlobSchema;
        use crate::schema::pds::record_blob::dsl as RecordBlobSchema;
        let conn = &mut establish_connection()?;

        let uris: Vec<String> = writes
            .clone()
            .into_iter()
            .filter_map(|w| match w {
                PreparedWrite::Delete(w) => Some(w.uri),
                PreparedWrite::Update(w) => Some(w.uri),
                _ => None,
            })
            .collect();
        if uris.len() == 0 {
            return Ok(());
        }

        let deleted_repo_blobs: Vec<models::RecordBlob> = delete(RecordBlobSchema::record_blob)
            .filter(RecordBlobSchema::recordUri.eq_any(uris))
            .get_results(conn)?
            .into_iter()
            .collect::<Vec<models::RecordBlob>>();
        if deleted_repo_blobs.len() < 1 {
            return Ok(());
        }

        let deleted_repo_blob_cids: Vec<String> = deleted_repo_blobs
            .into_iter()
            .map(|row| row.blob_cid)
            .collect::<Vec<String>>();
        let mut duplicated_cids: Vec<String> = RecordBlobSchema::record_blob
            .select(RecordBlobSchema::blobCid)
            .filter(RecordBlobSchema::blobCid.eq_any(&deleted_repo_blob_cids))
            .load(conn)?
            .into_iter()
            .collect::<Vec<String>>();

        let mut new_blob_cids: Vec<String> = writes
            .into_iter()
            .map(|w| match w {
                PreparedWrite::Create(w) => w.blobs,
                PreparedWrite::Update(w) => w.blobs,
                PreparedWrite::Delete(_) => Vec::new(),
            })
            .collect::<Vec<Vec<PreparedBlobRef>>>()
            .into_iter()
            .flat_map(|v: Vec<PreparedBlobRef>| v.into_iter().map(|b| b.cid.to_string()))
            .collect();
        let mut cids_to_keep = Vec::new();
        cids_to_keep.append(&mut new_blob_cids);
        cids_to_keep.append(&mut duplicated_cids);

        let cids_to_delete = deleted_repo_blob_cids
            .into_iter()
            .filter_map(|cid: String| match cids_to_keep.contains(&cid) {
                true => Some(cid),
                false => None,
            })
            .collect::<Vec<String>>();
        if cids_to_delete.len() < 1 {
            return Ok(());
        }

        delete(BlobSchema::blob)
            .filter(BlobSchema::cid.eq_any(&cids_to_delete))
            .execute(conn)?;

        // Original code queues a background job to delete by CID from S3 compatible blobstore
        let _ = stream::iter(cids_to_delete)
            .then(|cid| async { Ok::<(), anyhow::Error>(self.blobstore.delete(cid).await?) })
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
        Ok(())
    }

    pub async fn verify_blob_and_make_permanent(&self, blob: PreparedBlobRef) -> Result<()> {
        use crate::schema::pds::blob::dsl as BlobSchema;
        let conn = &mut establish_connection()?;

        let found = BlobSchema::blob
            .filter(
                BlobSchema::cid
                    .eq(blob.cid.to_string())
                    .and(BlobSchema::takedownRef.is_null()),
            )
            .select(models::Blob::as_select())
            .first(conn)
            .optional()?;
        if let Some(found) = found {
            verify_blob(&blob, &found).await?;
            if let Some(ref temp_key) = found.temp_key {
                self.blobstore
                    .make_permanent(temp_key.clone(), blob.cid)
                    .await?;
            }
            update(BlobSchema::blob)
                .filter(BlobSchema::tempKey.eq(found.temp_key))
                .set(BlobSchema::tempKey.eq::<Option<String>>(None))
                .execute(conn)?;
            Ok(())
        } else {
            bail!("Cound not find blob: {:?}", blob.cid.to_string())
        }
    }

    pub async fn associate_blob(&self, blob: PreparedBlobRef, record_uri: String) -> Result<()> {
        use crate::schema::pds::record_blob::dsl as RecordBlobSchema;
        let conn = &mut establish_connection()?;

        insert_into(RecordBlobSchema::record_blob)
            .values((
                RecordBlobSchema::blobCid.eq(blob.cid.to_string()),
                RecordBlobSchema::recordUri.eq(record_uri),
            ))
            .on_conflict_do_nothing()
            .execute(conn)?;
        Ok(())
    }

    pub async fn blob_count(&self) -> Result<i64> {
        use crate::schema::pds::blob::dsl as BlobSchema;
        let conn = &mut establish_connection()?;

        let res = BlobSchema::blob
            .filter(BlobSchema::did.eq(&self.did))
            .count()
            .get_result(conn)?;
        Ok(res)
    }

    pub async fn record_blob_count(&self) -> Result<i64> {
        use crate::schema::pds::record_blob::dsl as RecordBlobSchema;
        let conn = &mut establish_connection()?;

        let res: i64 = RecordBlobSchema::record_blob
            .filter(RecordBlobSchema::did.eq(&self.did))
            .select(count_distinct(RecordBlobSchema::blobCid))
            .get_result(conn)?;

        Ok(res)
    }
}

pub async fn accepted_mime(mime: String, accepted: Vec<String>) -> bool {
    if accepted.contains("*/*".to_owned()) {
        return true;
    }
    let globs: Vec<String> = accepted
        .clone()
        .into_iter()
        .filter(|a| a.ends_with("/*"))
        .collect::<Vec<String>>();
    for glob in globs {
        let start = glob.split("/").collect::<Vec<&str>>().first().copied();
        if let Some(start) = start {
            if mime.starts_with(&format!("{start:?}/")) {
                return true;
            }
        }
    }
    return accepted.contains(mime);
}

pub async fn verify_blob(blob: &PreparedBlobRef, found: &models::Blob) -> Result<()> {
    if let Some(max_size) = blob.contraints.max_size {
        if found.size as usize > max_size {
            bail!(
                "BlobTooLarge: This file is too large. It is {:?} but the maximum size is {:?}",
                found.size,
                max_size
            )
        }
    }
    if blob.mime_type != found.mime_type {
        bail!("InvalidMimeType: Referenced MimeTy[e does not match stored blob. Expected: {:?}, Got: {:?}",found.mime_type, blob.mime_type)
    }
    if let Some(ref accept) = blob.contraints.accept {
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
