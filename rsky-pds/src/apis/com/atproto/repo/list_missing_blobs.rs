use crate::actor_store::aws::s3::S3BlobStore;
use crate::actor_store::blob::ListMissingBlobsOpts;
use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use crate::auth_verifier::AccessFull;
use crate::db::DbConn;
use anyhow::Result;
use aws_config::SdkConfig;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::repo::ListMissingBlobsOutput;

#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.repo.listMissingBlobs?<limit>&<cursor>")]
pub async fn list_missing_blobs(
    limit: Option<u16>,
    cursor: Option<String>,
    auth: AccessFull,
    db: DbConn,
    s3_config: &State<SdkConfig>,
) -> Result<Json<ListMissingBlobsOutput>, ApiError> {
    let did = auth.access.credentials.did.unwrap();
    let limit: u16 = limit.unwrap_or(500);

    let actor_store = ActorStore::new(did.clone(), S3BlobStore::new(did.clone(), s3_config), db);

    match actor_store
        .blob
        .list_missing_blobs(ListMissingBlobsOpts { cursor, limit })
        .await
    {
        Ok(blobs) => {
            let cursor = match blobs.last() {
                Some(last_blob) => Some(last_blob.cid.clone()),
                None => None,
            };
            Ok(Json(ListMissingBlobsOutput { cursor, blobs }))
        }
        Err(error) => {
            tracing::error!("{error:?}");
            Err(ApiError::RuntimeError)
        }
    }
}
