use crate::auth_verifier::AccessFull;
use crate::models::{ErrorCode, ErrorMessageResponse};
use crate::repo::aws::s3::S3BlobStore;
use crate::repo::blob::ListMissingBlobsOpts;
use crate::repo::ActorStore;
use anyhow::Result;
use aws_config::SdkConfig;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::repo::ListMissingBlobsOutput;

#[rocket::get("/xrpc/com.atproto.repo.listMissingBlobs?<limit>&<cursor>")]
pub async fn list_missing_blobs(
    limit: Option<u16>,
    cursor: Option<String>,
    auth: AccessFull,
    s3_config: &State<SdkConfig>,
) -> Result<Json<ListMissingBlobsOutput>, status::Custom<Json<ErrorMessageResponse>>> {
    let did = auth.access.credentials.unwrap().did.unwrap();
    let limit: u16 = limit.unwrap_or(500);

    let actor_store = ActorStore::new(did.clone(), S3BlobStore::new(did.clone(), s3_config));

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
            eprintln!("{error:?}");
            let internal_error = ErrorMessageResponse {
                code: Some(ErrorCode::InternalServerError),
                message: Some(error.to_string()),
            };
            Err(status::Custom(
                Status::InternalServerError,
                Json(internal_error),
            ))
        }
    }
}
