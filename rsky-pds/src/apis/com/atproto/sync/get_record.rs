use crate::account_manager::AccountManager;
use crate::actor_store::aws::s3::S3BlobStore;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::repo::assert_repo_availability;
use crate::apis::ApiError;
use crate::auth_verifier;
use crate::auth_verifier::OptionalAccessOrAdminToken;
use crate::db::DbConn;
use anyhow::{bail, Result};
use aws_config::SdkConfig;
use libipld::Cid;
use rocket::{Responder, State};
use rsky_repo::storage::types::RepoStorage;
use rsky_repo::types::RecordPath;
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
    db: DbConn,
    account_manager: AccountManager,
) -> Result<Vec<u8>> {
    let is_user_or_admin = if let Some(access) = auth.access {
        auth_verifier::is_user_or_admin(access, &did)
    } else {
        false
    };
    let _ = assert_repo_availability(&did, is_user_or_admin, &account_manager).await?;
    let actor_store = ActorStore::new(did.clone(), S3BlobStore::new(did.clone(), s3_config), db);
    let storage_guard = actor_store.storage.read().await;
    let commit: Option<Cid> = match commit {
        Some(commit) => Some(Cid::from_str(&commit)?),
        None => storage_guard.get_root().await,
    };

    match commit {
        None => bail!("Could not find repo for DID: {did}"),
        Some(commit) => {
            rsky_repo::sync::provider::get_records(
                actor_store.storage.clone(),
                commit,
                vec![RecordPath { collection, rkey }],
            )
            .await
        }
    }
}

/// Get data blocks needed to prove the existence or non-existence of record in the current version
/// of repo. Does not require auth.
#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.sync.getRecord?<did>&<collection>&<rkey>&<commit>")]
pub async fn get_record(
    did: String,
    collection: String,
    rkey: String,
    commit: Option<String>, // DEPRECATED: referenced a repo commit by CID, and retrieved record as of that commit
    s3_config: &State<SdkConfig>,
    auth: OptionalAccessOrAdminToken,
    db: DbConn,
    account_manager: AccountManager,
) -> Result<BlockResponder, ApiError> {
    match inner_get_record(
        did,
        collection,
        rkey,
        commit,
        s3_config,
        auth,
        db,
        account_manager,
    )
    .await
    {
        Ok(res) => Ok(BlockResponder(res)),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
