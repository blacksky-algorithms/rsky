use crate::account_manager::helpers::account::AccountStatus;
use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::auth_verifier::AdminToken;
use crate::db::DbConn;
use crate::repo::aws::s3::S3BlobStore;
use crate::repo::ActorStore;
use crate::{sequencer, SharedSequencer};
use anyhow::Result;
use aws_config::SdkConfig;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::admin::DeleteAccountInput;

async fn inner_delete_account(
    body: Json<DeleteAccountInput>,
    sequencer: &State<SharedSequencer>,
    s3_config: &State<SdkConfig>,
    db: DbConn,
) -> Result<()> {
    let DeleteAccountInput { did } = body.into_inner();

    let mut actor_store =
        ActorStore::new(did.clone(), S3BlobStore::new(did.clone(), s3_config), db);
    actor_store.destroy().await?;
    AccountManager::delete_account(&did).await?;
    let mut lock = sequencer.sequencer.write().await;
    let tombstone_seq = lock.sequence_tombstone(did.clone()).await?;
    let account_seq = lock
        .sequence_account_evt(did.clone(), AccountStatus::Deleted)
        .await?;

    sequencer::delete_all_for_user(&did, Some(vec![account_seq, tombstone_seq])).await?;
    Ok(())
}

#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.admin.deleteAccount",
    format = "json",
    data = "<body>"
)]
pub async fn delete_account(
    body: Json<DeleteAccountInput>,
    sequencer: &State<SharedSequencer>,
    s3_config: &State<SdkConfig>,
    _auth: AdminToken,
    db: DbConn,
) -> Result<(), ApiError> {
    match inner_delete_account(body, sequencer, s3_config, db).await {
        Ok(_) => Ok(()),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
