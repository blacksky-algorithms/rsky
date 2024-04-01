use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::apis::com::atproto::server::assert_valid_did_documents_for_service;
use crate::auth_verifier::AccessNotAppPassword;
use crate::models::{InternalErrorCode, InternalErrorMessageResponse};
use crate::repo::aws::s3::S3BlobStore;
use crate::repo::cid_set::CidSet;
use crate::repo::types::CommitData;
use crate::repo::ActorStore;
use crate::SharedSequencer;
use anyhow::{bail, Result};
use aws_config::BehaviorVersion;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::State;

async fn inner_activate_account(
    auth: AccessNotAppPassword,
    sequencer: &State<SharedSequencer>,
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
        let config = aws_config::load_defaults(BehaviorVersion::v2023_11_09()).await;

        let mut actor_store = ActorStore::new(
            requester.clone(),
            S3BlobStore::new(requester.clone(), &config),
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
        let mut lock = sequencer.sequencer.write().await;
        lock.sequence_identity_evt(requester.clone()).await?;
        lock.sequence_handle_update(
            requester.clone(),
            account.handle.unwrap_or("handle.invalid".to_owned()),
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
    auth: AccessNotAppPassword,
    sequencer: &State<SharedSequencer>,
) -> Result<(), status::Custom<Json<InternalErrorMessageResponse>>> {
    match inner_activate_account(auth, sequencer).await {
        Ok(_) => Ok(()),
        Err(error) => {
            eprintln!("Internal Error: {error}");
            let internal_error = InternalErrorMessageResponse {
                code: Some(InternalErrorCode::InternalError),
                message: Some("Internal error".to_string()),
            };
            return Err(status::Custom(
                Status::InternalServerError,
                Json(internal_error),
            ));
        }
    }
}
