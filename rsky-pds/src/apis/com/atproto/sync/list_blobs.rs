use crate::apis::com::atproto::repo::assert_repo_availability;
use crate::auth_verifier;
use crate::auth_verifier::OptionalAccessOrAdminToken;
use crate::models::{ErrorCode, ErrorMessageResponse};
use crate::repo::aws::s3::S3BlobStore;
use crate::repo::blob::ListBlobsOpts;
use crate::repo::ActorStore;
use anyhow::Result;
use aws_config::SdkConfig;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::sync::ListBlobsOutput;

async fn inner_list_blobs(
    did: String,
    since: Option<String>, // Optional revision of the repo to list blobs since.
    limit: Option<u16>,
    cursor: Option<String>,
    s3_config: &State<SdkConfig>,
    auth: OptionalAccessOrAdminToken,
) -> Result<ListBlobsOutput> {
    let is_user_or_admin = if let Some(access) = auth.access {
        auth_verifier::is_user_or_admin(access, &did)
    } else {
        false
    };
    let _ = assert_repo_availability(&did, is_user_or_admin).await?;

    let actor_store = ActorStore::new(did.clone(), S3BlobStore::new(did.clone(), s3_config));
    let blob_cids = actor_store
        .blob
        .list_blobs(ListBlobsOpts {
            since,
            cursor,
            limit: limit.unwrap_or(500),
        })
        .await?;

    let last_blob: Option<String> = match blob_cids.last() {
        None => None,
        Some(last) => Some(last.clone()),
    };
    Ok(ListBlobsOutput {
        cursor: last_blob,
        cids: blob_cids,
    })
}

/// List blob CIDs for an account, since some repo revision. Does not require auth;
/// implemented by PDS
#[rocket::get("/xrpc/com.atproto.sync.listBlobs?<did>&<since>&<limit>&<cursor>")]
pub async fn list_blobs(
    did: String,
    since: Option<String>, // Optional revision of the repo to list blobs since.
    limit: Option<u16>,
    cursor: Option<String>,
    s3_config: &State<SdkConfig>,
    auth: OptionalAccessOrAdminToken,
) -> Result<Json<ListBlobsOutput>, status::Custom<Json<ErrorMessageResponse>>> {
    match inner_list_blobs(did, since, limit, cursor, s3_config, auth).await {
        Ok(res) => Ok(Json(res)),
        Err(error) => {
            eprintln!("@LOG: ERROR: {error}");
            let internal_error = ErrorMessageResponse {
                code: Some(ErrorCode::InternalServerError),
                message: Some(error.to_string()),
            };
            return Err(status::Custom(
                Status::InternalServerError,
                Json(internal_error),
            ));
        }
    }
}
