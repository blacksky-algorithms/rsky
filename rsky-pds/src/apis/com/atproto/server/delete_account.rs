use crate::account_manager::helpers::account::{AccountStatus, AvailabilityFlags};
use crate::account_manager::AccountManager;
use crate::actor_store::aws::s3::S3BlobStore;
use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use crate::auth_verifier::AdminToken;
use crate::models::models::EmailTokenPurpose;
use crate::SharedSequencer;
use aws_config::SdkConfig;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::server::DeleteAccountInput;
use std::sync::Arc;

#[tracing::instrument(skip_all)]
async fn inner_delete_account(
    body: Json<DeleteAccountInput>,
    sequencer: &State<SharedSequencer>,
    s3_config: &State<SdkConfig>,
    actor_store: &State<ActorStore>,
    account_manager: AccountManager,
) -> Result<(), ApiError> {
    let DeleteAccountInput {
        did,
        password,
        token,
    } = body.into_inner();
    let account = account_manager
        .get_account(
            &did,
            Some(AvailabilityFlags {
                include_deactivated: Some(true),
                include_taken_down: Some(true),
            }),
        )
        .await?;
    if account.is_some() {
        let valid_pass = account_manager
            .verify_account_password(&did, &password)
            .await?;
        if !valid_pass {
            return Err(ApiError::InvalidLogin);
        }
        account_manager
            .assert_valid_email_token(&did, EmailTokenPurpose::from_str("delete_account")?, &token)
            .await?;

        actor_store
            .destroy(&did, Arc::new(S3BlobStore::new(did.clone(), s3_config)))
            .await?;
        account_manager.delete_account(&did).await?;
        let mut lock = sequencer.sequencer.write().await;
        let account_seq = lock
            .sequence_account_evt(did.clone(), AccountStatus::Deleted)
            .await?;
        lock.delete_all_for_user(&did, Some(vec![account_seq]))
            .await?;
        Ok(())
    } else {
        tracing::error!("account not found");
        Err(ApiError::RuntimeError)
    }
}

#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.server.deleteAccount",
    format = "json",
    data = "<body>"
)]
pub async fn delete_account(
    body: Json<DeleteAccountInput>,
    sequencer: &State<SharedSequencer>,
    s3_config: &State<SdkConfig>,
    actor_store: &State<ActorStore>,
    _auth: AdminToken,
    account_manager: AccountManager,
) -> Result<(), ApiError> {
    match inner_delete_account(body, sequencer, s3_config, actor_store, account_manager).await {
        Ok(_) => Ok(()),
        Err(error) => Err(error),
    }
}
