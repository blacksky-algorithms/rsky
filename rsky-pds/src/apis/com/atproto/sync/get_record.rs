use crate::apis::com::atproto::repo::assert_repo_availability;
use crate::auth_verifier::OptionalAccessOrAdminToken;
use crate::models::{ErrorCode, ErrorMessageResponse};
use crate::repo::aws::s3::S3BlobStore;
use crate::repo::types::RecordPath;
use crate::repo::ActorStore;
use crate::{auth_verifier, repo};
use anyhow::{bail, Result};
use aws_config::SdkConfig;
use libipld::Cid;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::{Responder, State};
use std::str::FromStr;

#[derive(Responder)]
#[response(status = 200, content_type = "application/vnd.ipld.car")]
pub struct BlockResponder(Vec<u8>);

async fn inner_get_record(
    did: String,
    collection: String,
    rkey: String,
    commit: Option<String>,
    s3_config: &State<SdkConfig>,
    auth: OptionalAccessOrAdminToken,
) -> Result<Vec<u8>> {
    let is_user_or_admin = if let Some(access) = auth.access {
        auth_verifier::is_user_or_admin(access, &did)
    } else {
        false
    };
    let _ = assert_repo_availability(&did, is_user_or_admin).await?;
    let mut actor_store = ActorStore::new(did.clone(), S3BlobStore::new(did.clone(), s3_config));
    let commit: Option<Cid> = match commit {
        Some(commit) => Some(Cid::from_str(&commit)?),
        None => actor_store.storage.get_root().await,
    };

    match commit {
        None => bail!("Could not find repo for DID: {did}"),
        Some(commit) => {
            repo::sync::provider::get_records(
                &mut actor_store.storage,
                commit,
                vec![RecordPath { collection, rkey }],
            )
            .await
        }
    }
}

/// Get data blocks needed to prove the existence or non-existence of record in the current version
/// of repo. Does not require auth.
#[rocket::get("/xrpc/com.atproto.sync.getRecord?<did>&<collection>&<rkey>&<commit>")]
pub async fn get_record(
    did: String,
    collection: String,
    rkey: String,
    commit: Option<String>, // DEPRECATED: referenced a repo commit by CID, and retrieved record as of that commit
    s3_config: &State<SdkConfig>,
    auth: OptionalAccessOrAdminToken,
) -> Result<BlockResponder, status::Custom<Json<ErrorMessageResponse>>> {
    match inner_get_record(did, collection, rkey, commit, s3_config, auth).await {
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
