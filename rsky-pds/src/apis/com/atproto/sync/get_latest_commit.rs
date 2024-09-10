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
use rocket::State;
use rsky_lexicon::com::atproto::sync::GetLatestCommitOutput;

async fn inner_get_latest_commit(
    did: String,
    s3_config: &State<SdkConfig>,
    auth: OptionalAccessOrAdminToken,
) -> Result<GetLatestCommitOutput> {
    let is_user_or_admin = if let Some(access) = auth.access {
        auth_verifier::is_user_or_admin(access, &did)
    } else {
        false
    };
    let _ = assert_repo_availability(&did, is_user_or_admin).await?;

    let actor_store = ActorStore::new(did.clone(), S3BlobStore::new(did.clone(), s3_config));
    match actor_store.storage.get_root_detailed().await {
        Ok(res) => Ok(GetLatestCommitOutput {
            cid: res.cid.to_string(),
            rev: res.rev,
        }),
        Err(_) => bail!("Could not find root for DID: {did}"),
    }
}

#[rocket::get("/xrpc/com.atproto.sync.getLatestCommit?<did>")]
pub async fn get_latest_commit(
    did: String,
    s3_config: &State<SdkConfig>,
    auth: OptionalAccessOrAdminToken,
) -> Result<Json<GetLatestCommitOutput>, status::Custom<Json<ErrorMessageResponse>>> {
    match inner_get_latest_commit(did, s3_config, auth).await {
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
