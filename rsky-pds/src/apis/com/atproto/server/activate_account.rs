use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::apis::com::atproto::server::assert_valid_did_documents_for_service;
use crate::auth_verifier::AccessFull;
use crate::models::{ErrorCode, ErrorMessageResponse};
use crate::repo::aws::s3::S3BlobStore;
use crate::repo::cid_set::CidSet;
use crate::repo::types::CommitData;
use crate::repo::ActorStore;
use crate::SharedSequencer;
use crate::INVALID_HANDLE;
use anyhow::{bail, Result};
use aws_config::SdkConfig;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::State;

async fn inner_activate_account(
    auth: AccessFull,
    sequencer: &State<SharedSequencer>,
    s3_config: &State<SdkConfig>,
) -> Result<()> {
    let requester = auth.access.credentials.unwrap().did.unwrap();
    assert_valid_did_documents_for_service(requester.clone()).await?;

    let account = AccountManager::get_account(
        &requester,
        Some(AvailabilityFlags {
            include_deactivated: Some(true),
            include_taken_down: None,
        }),
    )
    .await?;

    if let Some(account) = account {
        AccountManager::activate_account(&requester).await?;

        let mut actor_store = ActorStore::new(
            requester.clone(),
            S3BlobStore::new(requester.clone(), s3_config),
        );
        let root = actor_store.storage.get_root_detailed().await?;
        let blocks = actor_store.storage.get_blocks(vec![root.cid]).await?;
        let commit_data = CommitData {
            cid: root.cid,
            rev: root.rev,
            since: None,
            prev: None,
            new_blocks: blocks.blocks,
            removed_cids: CidSet::new(None),
        };

        // @NOTE: we're over-emitting for now for backwards compatibility, can reduce this in the future
        let status = AccountManager::get_account_status(&requester).await?;
        let mut lock = sequencer.sequencer.write().await;
        lock.sequence_account_evt(requester.clone(), status).await?;
        lock.sequence_handle_update(
            requester.clone(),
            account.handle.unwrap_or(INVALID_HANDLE.to_string()),
        )
        .await?;
        lock.sequence_commit(requester, commit_data, vec![]).await?;
        Ok(())
    } else {
        bail!("User not found")
    }
}

#[rocket::post("/xrpc/com.atproto.server.activateAccount")]
pub async fn activate_account(
    auth: AccessFull,
    sequencer: &State<SharedSequencer>,
    s3_config: &State<SdkConfig>,
) -> Result<(), status::Custom<Json<ErrorMessageResponse>>> {
    match inner_activate_account(auth, sequencer, s3_config).await {
        Ok(_) => Ok(()),
        Err(error) => {
            eprintln!("Internal Error: {error}");
            let internal_error = ErrorMessageResponse {
                code: Some(ErrorCode::InternalServerError),
                message: Some("Internal error".to_string()),
            };
            return Err(status::Custom(
                Status::InternalServerError,
                Json(internal_error),
            ));
        }
    }
}
