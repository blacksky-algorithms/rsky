use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::repo::assert_repo_availability;
use crate::apis::ApiError;
use crate::auth_verifier;
use crate::auth_verifier::OptionalAccessOrAdminToken;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::sync::GetHeadOutput;

async fn inner_get_head(
    did: String,
    blobstore_factory: &State<BlobstoreFactory>,
    auth: OptionalAccessOrAdminToken,
    actor_store: &State<ActorStore>,
    account_manager: AccountManager,
) -> Result<GetHeadOutput, ApiError> {
    let is_user_or_admin = if let Some(access) = auth.access {
        auth_verifier::is_user_or_admin(access, &did)
    } else {
        false
    };
    let _ = assert_repo_availability(&did, is_user_or_admin, &account_manager)
        .await
        .map_err(|error| {
            tracing::error!("@LOG: ERROR: {error}");
            ApiError::RuntimeError
        })?;
    let actor_store = actor_store
        .read(did.clone(), blobstore_factory.blobstore(did.clone()))
        .await
        .map_err(|error| {
            tracing::error!("@LOG: ERROR: {error}");
            ApiError::RuntimeError
        })?;
    let storage_guard = actor_store.storage.read().await;
    match storage_guard.get_root_detailed().await {
        Ok(root) => Ok(GetHeadOutput {
            root: root.cid.to_string(),
        }),
        Err(_) => Err(ApiError::BadRequest(
            "HeadNotFound".to_string(),
            format!("Could not find root for DID: {did}"),
        )),
    }
}

/// DEPRECATED - please use com.atproto.sync.getLatestCommit instead
#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.sync.getHead?<did>")]
pub async fn get_head(
    did: String,
    blobstore_factory: &State<BlobstoreFactory>,
    auth: OptionalAccessOrAdminToken,
    actor_store: &State<ActorStore>,
    account_manager: AccountManager,
) -> Result<Json<GetHeadOutput>, ApiError> {
    let res = inner_get_head(did, blobstore_factory, auth, actor_store, account_manager).await?;
    Ok(Json(res))
}
