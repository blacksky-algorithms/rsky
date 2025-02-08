use crate::apis::ApiError;
use crate::auth_verifier::{AccessFull, AccessFullImport, AccessStandardIncludeChecks};
use crate::car::read_car_with_root;
use crate::common::tid::Ticker;
use crate::db::DbConn;
use crate::repo::aws::s3::S3BlobStore;
use crate::repo::blob_refs::BlobRef;
use crate::repo::block_map::BlockMap;
use crate::repo::parse::get_and_parse_record;
use crate::repo::sync::consumer::{verify_diff, VerifyRepoInput};
use crate::repo::types::{Lex, RecordWriteDescript, VerifiedDiff};
use crate::repo::{ActorStore, Repo};
use crate::storage::types::RepoStorage;
use aws_config::SdkConfig;
use chrono::Utc;
use lexicon_cid::Cid;
use rocket::data::ToByteUnit;
use rocket::http::ContentType;
use rocket::serde::json::Json;
use rocket::{Data, State};
use rsky_syntax::aturi::AtUri;

#[tracing::instrument(skip_all)]
#[rocket::post("/xrpc/com.atproto.repo.importRepo", data = "<blob>")]
pub async fn import_repo(
    auth: AccessFullImport,
    blob: Data<'_>,
    s3_config: &State<SdkConfig>,
    db: DbConn,
) -> Result<(), ApiError> {
    let now = Utc::now();
    let rev = Ticker::new().next(None).to_string();
    let requester = auth.access.credentials.unwrap().did.unwrap();
    let mut actor_store = ActorStore::new(
        requester.clone(),
        S3BlobStore::new(requester.clone(), s3_config),
        db,
    );

    // Get current repo if it exists
    let curr_root: Option<Cid> = actor_store.get_repo_root().await;
    let curr_repo: Option<Repo> = match curr_root {
        None => None,
        Some(_root) => Some(Repo::load(actor_store.storage.clone(), curr_root).await?),
    };
    let is_create = curr_repo.is_none();

    // Process imported car
    let import_datastream = blob.open(100.megabytes());
    let import_bytes = import_datastream.into_bytes().await.unwrap().value;
    let car_with_root;
    match read_car_with_root(import_bytes).await {
        Ok(res) => {
            car_with_root = res;
        }
        Err(error) => {
            return Err(ApiError::InvalidRequest(error.to_string()));
        }
    }

    // Get verified difference from current repo and imported repo
    let mut imported_blocks: BlockMap = car_with_root.blocks;
    let imported_root: Cid = car_with_root.root;
    let opts = VerifyRepoInput {
        ensure_leaves: Some(false),
    };
    let diff: VerifiedDiff;
    match verify_diff(
        curr_repo,
        &mut imported_blocks,
        imported_root,
        None,
        None,
        Some(opts),
    )
    .await
    {
        Ok(res) => {
            diff = res;
        }
        Err(error) => {
            tracing::error!("{:?}", error);
            return Err(ApiError::RuntimeError);
        }
    }

    // Apply Commit
    let commit_data = diff.commit;
    actor_store.apply_commit(commit_data, Some(is_create)).await?;

    // write diffs
    for write in diff.writes {
        match write {
            RecordWriteDescript::Create(create_write) => {
                let uri = AtUri::make(
                    requester.clone(),
                    Some(create_write.collection.clone()),
                    Some(create_write.rkey.clone()),
                )?;

                let parsed_record;
                match get_and_parse_record(&imported_blocks, create_write.cid) {
                    Ok(record) => {
                        parsed_record = record.record;
                    }
                    Err(e) => {
                        tracing::error!("{e}");
                        return Err(ApiError::InvalidRequest(format!(
                            "Could not parse record at {collection}/{rkey}",
                            collection = create_write.collection,
                            rkey = create_write.rkey
                        )));
                    }
                }

                //Index Record
                actor_store
                    .record
                    .index_record(
                        uri,
                        create_write.cid,
                        Some(parsed_record.clone()),
                        Some(create_write.action),
                        rev.clone(),
                        Some(now.to_string()),
                    )
                    .await?;

                //Index Blob References
                let record_blobs = find_blob_refs(Lex::Map(parsed_record.clone()), 0).await;
                if !record_blobs.is_empty() {
                    //TODO insert into record_blob
                    // actor_store.record.
                    // actor_store.blob.ass
                }
            }
            RecordWriteDescript::Update(update_write) => {
                let uri = AtUri::make(
                    requester.clone(),
                    Some(update_write.collection),
                    Some(update_write.rkey),
                )?;
                let parsed_record;
                match get_and_parse_record(&imported_blocks, update_write.cid) {
                    Ok(record) => {
                        parsed_record = record.record;
                    }
                    Err(e) => {
                        tracing::error!("{e}");
                        panic!()
                    }
                }

                //Index Record
                actor_store
                    .record
                    .index_record(
                        uri,
                        update_write.cid,
                        Some(parsed_record.clone()),
                        Some(update_write.action),
                        rev.clone(),
                        Some(now.to_string()),
                    )
                    .await?;

                //Index Blob References
                let record_blobs = find_blob_refs(Lex::Map(parsed_record.clone()), 0).await;
                if !record_blobs.is_empty() {
                    //TODO insert into record_blob
                }
            }
            RecordWriteDescript::Delete(delete_write) => {
                let uri = AtUri::make(
                    requester.clone(),
                    Some(delete_write.collection),
                    Some(delete_write.rkey),
                )?;
                actor_store.record.delete_record(&uri).await?;
            }
        }
    }
    Ok(())
}

async fn find_blob_refs(val: Lex, layer: u8) -> Vec<BlobRef> {
    let result: Vec<BlobRef> = Vec::new();
    if layer > 32 {
        return result;
    }
    // // walk arrays
    // if Array.isArray(val) {
    // return val.flatMap((item) => findBlobRefs(item, layer + 1))
    // }
    // // objects
    // if val && typeof val === 'object' {
    // // convert blobs, leaving the original encoding so that we don't change CIDs on re-encode
    // if (val instanceof BlobRef) {
    // return [val]
    // }
    // // retain cids & bytes
    // if (CID.asCID(val) || val instanceof Uint8Array) {
    // return []
    // }
    // return Object.values(val).flatMap((item) => findBlobRefs(item, layer + 1))
    // }
    // // pass through
    // return []
    result
}
