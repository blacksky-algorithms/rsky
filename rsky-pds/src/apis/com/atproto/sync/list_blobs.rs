use crate::account_manager::AccountManager;
use crate::actor_store::aws::s3::S3BlobStore;
use crate::actor_store::blob::ListBlobsOpts;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::repo::assert_repo_availability;
use crate::apis::ApiError;
use crate::auth_verifier;
use crate::auth_verifier::OptionalAccessOrAdminToken;
use crate::db::DbConn;
use anyhow::Result;
use aws_config::SdkConfig;
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
    db: DbConn,
    account_manager: AccountManager,
) -> Result<ListBlobsOutput> {
    let is_user_or_admin = if let Some(access) = auth.access {
        auth_verifier::is_user_or_admin(access, &did)
    } else {
        false
    };
    let _ = assert_repo_availability(&did, is_user_or_admin, &account_manager).await?;

    let actor_store = ActorStore::new(did.clone(), S3BlobStore::new(did.clone(), s3_config), db);
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
#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.sync.listBlobs?<did>&<since>&<limit>&<cursor>")]
pub async fn list_blobs(
    did: String,
    since: Option<String>, // Optional revision of the repo to list blobs since.
    limit: Option<u16>,
    cursor: Option<String>,
    s3_config: &State<SdkConfig>,
    auth: OptionalAccessOrAdminToken,
    db: DbConn,
    account_manager: AccountManager,
) -> Result<Json<ListBlobsOutput>, ApiError> {
    match inner_list_blobs(
        did,
        since,
        limit,
        cursor,
        s3_config,
        auth,
        db,
        account_manager,
    )
    .await
    {
        Ok(res) => Ok(Json(res)),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
