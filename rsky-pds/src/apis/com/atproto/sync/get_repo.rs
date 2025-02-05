use crate::apis::com::atproto::repo::assert_repo_availability;
use crate::apis::ApiError;
use crate::auth_verifier;
use crate::auth_verifier::OptionalAccessOrAdminToken;
use crate::db::DbConn;
use crate::repo::aws::s3::S3BlobStore;
use crate::repo::ActorStore;
use anyhow::{bail, Result};
use aws_config::SdkConfig;
use rocket::{Responder, State};

#[derive(Responder)]
#[response(status = 200, content_type = "application/vnd.ipld.car")]
pub struct BlockResponder(Vec<u8>);

async fn get_car_stream(
    s3_config: &State<SdkConfig>,
    did: String,
    since: Option<String>,
    db: DbConn,
) -> Result<Vec<u8>> {
    let actor_store = ActorStore::new(did.clone(), S3BlobStore::new(did.clone(), s3_config), db);
    let storage_guard = actor_store.storage.read().await;
    match storage_guard.get_car_stream(since).await {
        Err(_) => bail!("Could not find repo for DID: {did}"),
        Ok(carstream) => Ok(carstream),
    }
}

async fn inner_get_repo(
    did: String,
    since: Option<String>, // The revision ('rev') of the repo to create a diff from.
    s3_config: &State<SdkConfig>,
    auth: OptionalAccessOrAdminToken,
    db: DbConn,
) -> Result<Vec<u8>> {
    let is_user_or_admin = if let Some(access) = auth.access {
        auth_verifier::is_user_or_admin(access, &did)
    } else {
        false
    };
    let _ = assert_repo_availability(&did, is_user_or_admin).await?;
    get_car_stream(s3_config, did, since, db).await
}

/// Download a repository export as CAR file. Optionally only a 'diff' since a previous revision.
/// Does not require auth; implemented by PDS.
#[rocket::get("/xrpc/com.atproto.sync.getRepo?<did>&<since>")]
pub async fn get_repo(
    did: String,
    since: Option<String>, // The revision ('rev') of the repo to create a diff from.
    s3_config: &State<SdkConfig>,
    auth: OptionalAccessOrAdminToken,
    db: DbConn,
) -> Result<BlockResponder, ApiError> {
    match inner_get_repo(did, since, s3_config, auth, db).await {
        Ok(res) => Ok(BlockResponder(res)),
        Err(error) => {
            eprintln!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
