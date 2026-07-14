use crate::account_manager::helpers::account::AccountStatus;
use crate::account_manager::AccountManager;
use crate::actor_store::aws::s3::S3BlobStore;
use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use crate::auth_verifier::AdminToken;
use crate::{sequencer, SharedSequencer};
use anyhow::Result;
use aws_config::SdkConfig;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::admin::DeleteAccountInput;
use std::sync::Arc;

async fn inner_delete_account(
    body: Json<DeleteAccountInput>,
    sequencer: &State<SharedSequencer>,
    s3_config: &State<SdkConfig>,
    actor_store: &State<ActorStore>,
    account_manager: AccountManager,
) -> Result<()> {
    let DeleteAccountInput { did } = body.into_inner();

    actor_store
        .destroy(&did, Arc::new(S3BlobStore::new(did.clone(), s3_config)))
        .await?;
    account_manager.delete_account(&did).await?;
    let mut lock = sequencer.sequencer.write().await;
    let account_seq = lock
        .sequence_account_evt(did.clone(), AccountStatus::Deleted)
        .await?;

    sequencer::delete_all_for_user(&did, Some(vec![account_seq])).await?;
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
    actor_store: &State<ActorStore>,
    account_manager: AccountManager,
) -> Result<(), ApiError> {
    match inner_delete_account(body, sequencer, s3_config, actor_store, account_manager).await {
        Ok(_) => Ok(()),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
