use crate::account_manager::helpers::account::{AccountStatus, AvailabilityFlags};
use crate::account_manager::AccountManager;
use crate::actor_store::aws::s3::S3BlobStore;
use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use crate::auth_verifier::AdminToken;
use crate::db::DbConn;
use crate::models::models::EmailTokenPurpose;
use crate::sequencer;
use crate::SharedSequencer;
use aws_config::SdkConfig;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::server::DeleteAccountInput;

#[tracing::instrument(skip_all)]
async fn inner_delete_account(
    body: Json<DeleteAccountInput>,
    sequencer: &State<SharedSequencer>,
    s3_config: &State<SdkConfig>,
    db: DbConn,
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

        let mut actor_store =
            ActorStore::new(did.clone(), S3BlobStore::new(did.clone(), s3_config), db);
        actor_store.destroy().await?;
        account_manager.delete_account(&did).await?;
        let mut lock = sequencer.sequencer.write().await;
        let account_seq = lock
            .sequence_account_evt(did.clone(), AccountStatus::Deleted)
            .await?;
        let tombstone_seq = lock.sequence_tombstone(did.clone()).await?;

        sequencer::delete_all_for_user(&did, Some(vec![account_seq, tombstone_seq])).await?;
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
    db: DbConn,
    _auth: AdminToken,
    account_manager: AccountManager,
) -> Result<(), ApiError> {
    match inner_delete_account(body, sequencer, s3_config, db, account_manager).await {
        Ok(_) => Ok(()),
        Err(error) => Err(error),
    }
}
