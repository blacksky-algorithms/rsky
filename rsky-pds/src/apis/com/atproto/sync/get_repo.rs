use crate::apis::com::atproto::repo::assert_repo_availability;
use crate::auth_verifier;
use crate::auth_verifier::OptionalAccessOrAdminToken;
use crate::models::{ErrorCode, ErrorMessageResponse};
use crate::repo::aws::s3::S3BlobStore;
use crate::repo::ActorStore;
use anyhow::{bail, Result};
use aws_config::SdkConfig;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::{Responder, State};

#[derive(Responder)]
#[response(status = 200, content_type = "application/vnd.ipld.car")]
pub struct BlockResponder(Vec<u8>);

async fn get_car_stream(
    s3_config: &State<SdkConfig>,
    did: String,
    since: Option<String>,
) -> Result<Vec<u8>> {
    let actor_store = ActorStore::new(did.clone(), S3BlobStore::new(did.clone(), s3_config));
    match actor_store.storage.get_car_stream(since).await {
        Err(_) => bail!("Could not find repo for DID: {did}"),
        Ok(carstream) => Ok(carstream),
    }
}

async fn inner_get_repo(
    did: String,
    since: Option<String>, // The revision ('rev') of the repo to create a diff from.
    s3_config: &State<SdkConfig>,
    auth: OptionalAccessOrAdminToken,
) -> Result<Vec<u8>> {
    let is_user_or_admin = if let Some(access) = auth.access {
        auth_verifier::is_user_or_admin(access, &did)
    } else {
        false
    };
    let _ = assert_repo_availability(&did, is_user_or_admin).await?;
    get_car_stream(s3_config, did, since).await
}

/// Download a repository export as CAR file. Optionally only a 'diff' since a previous revision.
/// Does not require auth; implemented by PDS.
#[rocket::get("/xrpc/com.atproto.sync.getRepo?<did>&<since>")]
pub async fn get_repo(
    did: String,
    since: Option<String>, // The revision ('rev') of the repo to create a diff from.
    s3_config: &State<SdkConfig>,
    auth: OptionalAccessOrAdminToken,
) -> Result<BlockResponder, status::Custom<Json<ErrorMessageResponse>>> {
    match inner_get_repo(did, since, s3_config, auth).await {
        Ok(res) => Ok(BlockResponder(res)),
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
