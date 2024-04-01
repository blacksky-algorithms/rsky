use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::models::models::EmailTokenPurpose;
use crate::models::{InternalErrorCode, InternalErrorMessageResponse};
use crate::repo::aws::s3::S3BlobStore;
use crate::repo::ActorStore;
use crate::sequencer;
use crate::SharedSequencer;
use anyhow::{bail, Result};
use aws_config::BehaviorVersion;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::server::DeleteAccountInput;

async fn inner_delete_account(
    body: Json<DeleteAccountInput>,
    sequencer: &State<SharedSequencer>,
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
        let config = aws_config::load_defaults(BehaviorVersion::v2023_11_09()).await;

        let mut actor_store = ActorStore::new(did.clone(), S3BlobStore::new(did.clone(), &config));
        actor_store.destroy().await?;
        AccountManager::delete_account(&did).await?;
        let mut lock = sequencer.sequencer.write().await;
        lock.sequence_identity_evt(did.clone()).await?;
        lock.sequence_tombstone(did.clone()).await?;

        sequencer::delete_all_for_user(&did).await?;
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
) -> Result<(), status::Custom<Json<InternalErrorMessageResponse>>> {
    match inner_delete_account(body, sequencer).await {
        Ok(_) => Ok(()),
        Err(error) => {
            let internal_error = InternalErrorMessageResponse {
                code: Some(InternalErrorCode::InternalError),
                message: Some(error.to_string()),
            };
            return Err(status::Custom(
                Status::InternalServerError,
                Json(internal_error),
            ));
        }
    }
}
