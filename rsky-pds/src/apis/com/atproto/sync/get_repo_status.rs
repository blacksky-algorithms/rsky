use crate::account_manager::helpers::account::{
    format_account_status, AccountStatus, FormattedAccountStatus,
};
use crate::account_manager::AccountManager;
use crate::actor_store::aws::s3::S3BlobStore;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::repo::assert_repo_availability;
use crate::apis::ApiError;
use crate::db::DbConn;
use anyhow::Result;
use aws_config::SdkConfig;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::sync::{GetRepoStatusOutput, RepoStatus};

async fn inner_get_repo(
    did: String,
    s3_config: &State<SdkConfig>,
    db: DbConn,
    account_manager: AccountManager,
) -> Result<GetRepoStatusOutput> {
    let account = assert_repo_availability(&did, true, &account_manager).await?;
    let FormattedAccountStatus { active, status } = format_account_status(Some(account));

    let mut rev: Option<String> = None;
    if active {
        let actor_store =
            ActorStore::new(did.clone(), S3BlobStore::new(did.clone(), s3_config), db);
        let storage_guard = actor_store.storage.read().await;
        let root = storage_guard.get_root_detailed().await?;
        rev = Some(root.rev);
    }

    Ok(GetRepoStatusOutput {
        did,
        active,
        status: match status {
            None => None,
            Some(status) => match status {
                AccountStatus::Active => None,
                AccountStatus::Takendown => Some(RepoStatus::Takedown),
                AccountStatus::Suspended => Some(RepoStatus::Suspended),
                AccountStatus::Deleted => None,
                AccountStatus::Deactivated => Some(RepoStatus::Deactivated),
                AccountStatus::Desynchronized => Some(RepoStatus::Desynchronized),
                AccountStatus::Throttled => Some(RepoStatus::Throttled),
            },
        },
        rev,
    })
}

/// Get the hosting status for a repository, on this server.
/// Expected to be implemented by PDS and Relay.
#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.sync.getRepoStatus?<did>")]
pub async fn get_repo_status(
    did: String,
    s3_config: &State<SdkConfig>,
    db: DbConn,
    account_manager: AccountManager,
) -> Result<Json<GetRepoStatusOutput>, ApiError> {
    match inner_get_repo(did, s3_config, db, account_manager).await {
        Ok(res) => Ok(Json(res)),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
