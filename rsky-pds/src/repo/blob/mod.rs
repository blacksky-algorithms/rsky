use crate::common::ipld::sha256_raw_to_cid;
use crate::common::now;
use crate::db::establish_connection;
use crate::models::models;
use crate::repo::aws::s3::S3BlobStore;
use crate::repo::blob_refs::BlobRef;
use crate::repo::error::BlobError;
use crate::repo::types::{PreparedBlobRef, PreparedWrite};
use crate::{common, image};
use anyhow::{bail, Result};
use aws_sdk_s3::operation::get_object::GetObjectError;
use aws_sdk_s3::primitives::ByteStream;
use diesel::dsl::{count_distinct, exists, not};
use diesel::sql_types::{Integer, Nullable, Text};
use diesel::*;
use futures::stream::{self, StreamExt};
use futures::try_join;
use lexicon_cid::Cid;
use rocket::data::{Data, ToByteUnit};
use rocket::form::validate::Contains;
use rsky_lexicon::com::atproto::admin::StatusAttr;
use rsky_lexicon::com::atproto::repo::ListMissingBlobsRefRecordBlob;
use sha2::{Digest, Sha256};

pub struct BlobMetadata {
    pub temp_key: String,
    pub size: i64,
    pub cid: Cid,
    pub mime_type: String,
    pub width: Option<i32>,
    pub height: Option<i32>,
}

pub struct BlobReader {
    pub blobstore: S3BlobStore,
    pub did: String,
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
    pub size: i32,
    pub mime_type: Option<String>,
    pub stream: ByteStream,
}

pub struct GetBlobMetadataOutput {
    pub size: i32,
    pub mime_type: Option<String>,
}

// Basically handles getting blob records from db
impl BlobReader {
    pub fn new(blobstore: S3BlobStore) -> Self {
        BlobReader {
            did: blobstore.bucket.clone(),
            blobstore,
        }
    }

    pub async fn get_blob_metadata(&self, cid: Cid) -> Result<GetBlobMetadataOutput> {
        use crate::schema::pds::blob::dsl as BlobSchema;
        let conn = &mut establish_connection()?;

        let found = BlobSchema::blob
            .filter(BlobSchema::did.eq(&self.did))
            .filter(BlobSchema::cid.eq(&cid.to_string()))
            .filter(BlobSchema::takedownRef.is_null())
            .select(models::Blob::as_select())
            .first(conn)
            .optional()?;

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
        let blob_stream = match self.blobstore.get_stream(cid).await {
            Ok(res) => res,
            Err(e) => {
                return match e.downcast_ref() {
                    Some(GetObjectError::NoSuchKey(key)) => {
                        Err(anyhow::Error::new(GetObjectError::NoSuchKey(key.clone())))
                    }
                    _ => bail!(e.to_string()),
                }
            }
        };
        Ok(GetBlobOutput {
            size: metadata.size,
            mime_type: metadata.mime_type,
            stream: blob_stream,
        })
    }

    pub async fn get_records_for_blob(&self, cid: Cid) -> Result<Vec<String>> {
        use crate::schema::pds::record_blob::dsl as RecordBlobSchema;
        let conn = &mut establish_connection()?;

        let res = RecordBlobSchema::record_blob
            .filter(RecordBlobSchema::blobCid.eq(cid.to_string()))
            .filter(RecordBlobSchema::did.eq(&self.did))
            .select(models::RecordBlob::as_select())
            .get_results(conn)?
            .into_iter()
            .map(|row| row.record_uri)
            .collect::<Vec<String>>();

        Ok(res)
    }

    pub async fn upload_blob_and_get_metadata(
        &self,
        user_suggested_mime: String,
        blob: Data<'_>,
    ) -> Result<BlobMetadata> {
        let blob_stream = blob.open(100.mebibytes());
        let bytes = blob_stream.into_bytes().await?;
        let size = bytes.n.written;
        let bytes = bytes.into_inner();
        let (temp_key, sha256, img_info, sniffed_mime) = try_join!(
            self.blobstore.put_temp(bytes.clone()),
            sha256_stream(bytes.clone()),
            image::maybe_get_info(bytes.clone()),
            image::mime_type_from_bytes(bytes.clone())
        )?;

        let cid = sha256_raw_to_cid(sha256);
        let mime_type = sniffed_mime.unwrap_or(user_suggested_mime);

        Ok(BlobMetadata {
            temp_key,
            size: size as i64,
            cid,
            mime_type,
            width: if let Some(ref info) = img_info {
                Some(info.width.clone() as i32)
            } else {
                None
            },
            height: if let Some(info) = img_info {
                Some(info.height as i32)
            } else {
                None
            },
        })
    }

    pub async fn track_untethered_blob(&self, metadata: BlobMetadata) -> Result<BlobRef> {
        use crate::schema::pds::blob::dsl as BlobSchema;
        let conn = &mut establish_connection()?;
        let BlobMetadata {
            temp_key,
            size,
            cid,
            mime_type,
            width,
            height,
        } = metadata;
        let created_at = now();

        let found = BlobSchema::blob
            .filter(BlobSchema::did.eq(&self.did))
            .filter(BlobSchema::cid.eq(&cid.to_string()))
            .select(models::Blob::as_select())
            .first(conn)
            .optional()?;

        if let Some(found) = found {
            if found.takedown_ref.is_some() {
                bail!("Blob has been takendown, cannot re-upload")
            }
        }

        let upsert = sql_query("INSERT INTO pds.blob (cid, did, \"mimeType\", size, \"tempKey\", width, height, \"createdAt\", \"takedownRef\") \
        VALUES \
            ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
        ON CONFLICT (cid, did) DO UPDATE \
        SET \"tempKey\" = EXCLUDED.\"tempKey\" \
            WHERE pds.blob.\"tempKey\" is not null;");
        upsert
            .bind::<Text, _>(&cid.to_string())
            .bind::<Text, _>(&self.did)
            .bind::<Text, _>(&mime_type)
            .bind::<Integer, _>(size.clone() as i32)
            .bind::<Nullable<Text>, _>(Some(temp_key.clone()))
            .bind::<Nullable<Integer>, _>(width)
            .bind::<Nullable<Integer>, _>(height)
            .bind::<Text, _>(created_at)
            .bind::<Nullable<Text>, _>(None as Option<String>)
            .execute(conn)?;

        Ok(BlobRef::new(cid, mime_type, size, None))
    }

    pub async fn process_write_blobs(&self, writes: Vec<PreparedWrite>) -> Result<()> {
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
                RecordBlobSchema::did.eq(&self.did),
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

    pub async fn list_missing_blobs(
        &self,
        opts: ListMissingBlobsOpts,
    ) -> Result<Vec<ListMissingBlobsRefRecordBlob>> {
        use crate::schema::pds::blob::dsl as BlobSchema;
        use crate::schema::pds::record_blob::dsl as RecordBlobSchema;
        let conn = &mut establish_connection()?;
        let ListMissingBlobsOpts { cursor, limit } = opts;

        if limit > 1000 {
            bail!("Limit too high. Max: 1000.");
        }

        let res: Vec<models::RecordBlob> = if let Some(cursor) = cursor {
            RecordBlobSchema::record_blob
                .limit(limit as i64)
                .filter(not(exists(
                    BlobSchema::blob
                        .filter(BlobSchema::cid.eq(RecordBlobSchema::blobCid))
                        .filter(BlobSchema::did.eq(&self.did))
                        .select(models::Blob::as_select()),
                )))
                .filter(RecordBlobSchema::blobCid.gt(cursor))
                .filter(RecordBlobSchema::did.eq(&self.did))
                .select(models::RecordBlob::as_select())
                .order(RecordBlobSchema::blobCid.asc())
                .distinct_on(RecordBlobSchema::blobCid)
                .get_results(conn)?
        } else {
            RecordBlobSchema::record_blob
                .limit(limit as i64)
                .filter(not(exists(
                    BlobSchema::blob
                        .filter(BlobSchema::cid.eq(RecordBlobSchema::blobCid))
                        .filter(BlobSchema::did.eq(&self.did))
                        .select(models::Blob::as_select()),
                )))
                .filter(RecordBlobSchema::did.eq(&self.did))
                .select(models::RecordBlob::as_select())
                .order(RecordBlobSchema::blobCid.asc())
                .distinct_on(RecordBlobSchema::blobCid)
                .get_results(conn)?
        };

        Ok(res
            .into_iter()
            .map(|row| ListMissingBlobsRefRecordBlob {
                cid: row.blob_cid,
                record_uri: row.record_uri,
            })
            .collect())
    }

    pub async fn list_blobs(&self, opts: ListBlobsOpts) -> Result<Vec<String>> {
        use crate::schema::pds::record::dsl as RecordSchema;
        use crate::schema::pds::record_blob::dsl as RecordBlobSchema;
        let conn = &mut establish_connection()?;
        let ListBlobsOpts {
            since,
            cursor,
            limit,
        } = opts;

        let res: Vec<String> = if let Some(since) = since {
            let mut builder = RecordBlobSchema::record_blob
                .inner_join(
                    RecordSchema::record.on(RecordSchema::uri.eq(RecordBlobSchema::recordUri)),
                )
                .filter(RecordSchema::repoRev.gt(&since))
                .select(RecordBlobSchema::blobCid)
                .distinct()
                .order(RecordBlobSchema::blobCid.asc())
                .limit(limit as i64)
                .into_boxed();

            if let Some(cursor) = cursor {
                builder = builder.filter(RecordBlobSchema::blobCid.gt(cursor));
            }
            builder.load(conn)?
        } else {
            let mut builder = RecordBlobSchema::record_blob
                .select(RecordBlobSchema::blobCid)
                .distinct()
                .order(RecordBlobSchema::blobCid.asc())
                .limit(limit as i64)
                .into_boxed();

            if let Some(cursor) = cursor {
                builder = builder.filter(RecordBlobSchema::blobCid.gt(cursor));
            }
            builder.load(conn)?
        };
        Ok(res)
    }

    pub async fn get_blob_takedown_status(&self, cid: Cid) -> Result<Option<StatusAttr>> {
        use crate::schema::pds::blob::dsl as BlobSchema;
        let conn = &mut establish_connection()?;

        let res = BlobSchema::blob
            .filter(BlobSchema::cid.eq(cid.to_string()))
            .select(models::Blob::as_select())
            .first(conn)
            .optional()?;
        match res {
            None => Ok(None),
            Some(res) => match res.takedown_ref {
                None => Ok(Some(StatusAttr {
                    applied: false,
                    r#ref: None,
                })),
                Some(takedown_ref) => Ok(Some(StatusAttr {
                    applied: true,
                    r#ref: Some(takedown_ref),
                })),
            },
        }
    }

    // Transactors
    // -------------------

    pub async fn update_blob_takedown_status(&self, blob: Cid, takedown: StatusAttr) -> Result<()> {
        use crate::schema::pds::blob::dsl as BlobSchema;
        let conn = &mut establish_connection()?;

        let takedown_ref: Option<String> = match takedown.applied {
            true => match takedown.r#ref {
                Some(takedown_ref) => Some(takedown_ref),
                None => Some(common::now()),
            },
            false => None,
        };

        update(BlobSchema::blob)
            .filter(BlobSchema::cid.eq(blob.to_string()))
            .set(BlobSchema::takedownRef.eq(takedown_ref))
            .execute(conn)?;

        let res = match takedown.applied {
            true => self.blobstore.quarantine(blob).await,
            false => self.blobstore.unquarantine(blob).await,
        };
        match res {
            Ok(_) => Ok(()),
            Err(e) => match e.downcast_ref() {
                Some(BlobError::BlobNotFoundError) => Ok(()),
                None => Err(e),
            },
        }
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
        bail!("InvalidMimeType: Referenced MimeTy[e does not match stored blob. Expected: {:?}, Got: {:?}",found.mime_type, blob.mime_type)
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
