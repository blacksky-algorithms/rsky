use crate::account_manager::AccountManager;
use crate::actor_store::aws::s3::S3BlobStore;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::repo::assert_repo_availability;
use crate::apis::ApiError;
use crate::auth_verifier;
use crate::auth_verifier::OptionalAccessOrAdminToken;
use anyhow::{bail, Result};
use aws_config::SdkConfig;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::sync::GetLatestCommitOutput;
use std::sync::Arc;

async fn inner_get_latest_commit(
    did: String,
    s3_config: &State<SdkConfig>,
    auth: OptionalAccessOrAdminToken,
    actor_store: &State<ActorStore>,
    account_manager: AccountManager,
) -> Result<GetLatestCommitOutput> {
    let is_user_or_admin = if let Some(access) = auth.access {
        auth_verifier::is_user_or_admin(access, &did)
    } else {
        false
    };
    let _ = assert_repo_availability(&did, is_user_or_admin, &account_manager).await?;

    let actor_store = actor_store
        .read(
            did.clone(),
            Arc::new(S3BlobStore::new(did.clone(), s3_config)),
        )
        .await?;
    let storage_guard = actor_store.storage.read().await;
    match storage_guard.get_root_detailed().await {
        Ok(res) => Ok(GetLatestCommitOutput {
            cid: res.cid.to_string(),
            rev: res.rev,
        }),
        Err(_) => bail!("Could not find root for DID: {did}"),
    }
}

#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.sync.getLatestCommit?<did>")]
pub async fn get_latest_commit(
    did: String,
    s3_config: &State<SdkConfig>,
    auth: OptionalAccessOrAdminToken,
    actor_store: &State<ActorStore>,
    account_manager: AccountManager,
) -> Result<Json<GetLatestCommitOutput>, ApiError> {
    match inner_get_latest_commit(did, s3_config, auth, actor_store, account_manager).await {
        Ok(res) => Ok(Json(res)),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
