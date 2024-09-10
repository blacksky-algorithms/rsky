use crate::account_manager::helpers::account::{AccountStatus, AvailabilityFlags};
use crate::account_manager::AccountManager;
use crate::auth_verifier::AdminToken;
use crate::models::models::EmailTokenPurpose;
use crate::models::{ErrorCode, ErrorMessageResponse};
use crate::repo::aws::s3::S3BlobStore;
use crate::repo::ActorStore;
use crate::sequencer;
use crate::SharedSequencer;
use anyhow::{bail, Result};
use aws_config::SdkConfig;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::server::DeleteAccountInput;

async fn inner_delete_account(
    body: Json<DeleteAccountInput>,
    sequencer: &State<SharedSequencer>,
    s3_config: &State<SdkConfig>,
) -> Result<()> {
    let DeleteAccountInput {
        did,
        password,
        token,
    } = body.into_inner();
    let account = AccountManager::get_account(
        &did,
        Some(AvailabilityFlags {
            include_deactivated: Some(true),
            include_taken_down: Some(true),
        }),
    )
    .await?;
    if let Some(_) = account {
        let valid_pass = AccountManager::verify_account_password(&did, &password).await?;
        if !valid_pass {
            bail!("Invalid did or password")
        }
        AccountManager::assert_valid_email_token(
            &did,
            EmailTokenPurpose::from_str("delete_account")?,
            &token,
        )
        .await?;

        let mut actor_store =
            ActorStore::new(did.clone(), S3BlobStore::new(did.clone(), s3_config));
        actor_store.destroy().await?;
        AccountManager::delete_account(&did).await?;
        let mut lock = sequencer.sequencer.write().await;
        let account_seq = lock
            .sequence_account_evt(did.clone(), AccountStatus::Deleted)
            .await?;
        let tombstone_seq = lock.sequence_tombstone(did.clone()).await?;

        sequencer::delete_all_for_user(&did, Some(vec![account_seq, tombstone_seq])).await?;
        Ok(())
    } else {
        bail!("account not found")
    }
}

#[rocket::post(
    "/xrpc/com.atproto.server.deleteAccount",
    format = "json",
    data = "<body>"
)]
pub async fn delete_account(
    body: Json<DeleteAccountInput>,
    sequencer: &State<SharedSequencer>,
    s3_config: &State<SdkConfig>,
    _auth: AdminToken,
) -> Result<(), status::Custom<Json<ErrorMessageResponse>>> {
    match inner_delete_account(body, sequencer, s3_config).await {
        Ok(_) => Ok(()),
        Err(error) => {
            eprintln!("@LOG: ERROR: {error}");
            let internal_error = ErrorMessageResponse {
                code: Some(ErrorCode::InternalServerError),
                message: Some(error.to_string()),
            };
            return Err(status::Custom(
                Status::InternalServerError,
                Json(internal_error),
            ));
        }
    }
}
