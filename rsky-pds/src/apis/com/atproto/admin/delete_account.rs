use crate::account_manager::helpers::account::AccountStatus;
use crate::account_manager::AccountManager;
use crate::auth_verifier::AdminToken;
use crate::models::{ErrorCode, ErrorMessageResponse};
use crate::repo::aws::s3::S3BlobStore;
use crate::repo::ActorStore;
use crate::{sequencer, SharedSequencer};
use anyhow::Result;
use aws_config::SdkConfig;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::admin::DeleteAccountInput;

async fn inner_delete_account(
    body: Json<DeleteAccountInput>,
    sequencer: &State<SharedSequencer>,
    s3_config: &State<SdkConfig>,
) -> Result<()> {
    let DeleteAccountInput { did } = body.into_inner();

    let mut actor_store = ActorStore::new(did.clone(), S3BlobStore::new(did.clone(), s3_config));
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
