use crate::models::models;
use crate::repo::aws::s3::S3BlobStore;
use crate::repo::types::{PreparedBlobRef, PreparedWrite};
use anyhow::Result;
use diesel::PgConnection;
use diesel::*;
use futures::executor;
use futures::stream::{self, StreamExt};
use libipld::Cid;
use std::str::FromStr;

pub struct BlobReader {
    pub blobstore: S3BlobStore,
}

// Basically handles getting lexicon records from db
impl BlobReader {
    pub fn new(blobstore: S3BlobStore) -> Self {
        BlobReader { blobstore }
    }

    pub fn delete_dereferenced_blobs(
        &mut self,
        conn: &mut PgConnection,
        writes: Vec<PreparedWrite>,
    ) -> Result<()> {
        use crate::schema::pds::blob::dsl as BlobSchema;
        use crate::schema::pds::record_blob::dsl as RecordBlobSchema;

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
        let res = async {
            stream::iter(cids_to_delete).then(|cid| async {
                Ok::<(), anyhow::Error>(self.blobstore.delete(cid).await?)
            })
        };
        let _ = executor::block_on(res);
        Ok(())
    }
}
