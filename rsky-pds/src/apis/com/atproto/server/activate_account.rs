use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::actor_store::aws::s3::S3BlobStore;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::server::assert_valid_did_documents_for_service;
use crate::apis::ApiError;
use crate::auth_verifier::AccessFull;
use crate::SharedSequencer;
use aws_config::SdkConfig;
use rocket::State;
use rsky_syntax::handle::INVALID_HANDLE;
use std::sync::Arc;

#[tracing::instrument(skip_all)]
async fn inner_activate_account(
    auth: AccessFull,
    sequencer: &State<SharedSequencer>,
    s3_config: &State<SdkConfig>,
    actor_store: &State<ActorStore>,
    account_manager: AccountManager,
) -> Result<(), ApiError> {
    let requester = auth.access.credentials.unwrap().did.unwrap();
    assert_valid_did_documents_for_service(requester.clone()).await?;

    let account = account_manager
        .get_account(
            &requester,
            Some(AvailabilityFlags {
                include_deactivated: Some(true),
                include_taken_down: None,
            }),
        )
        .await?;

    if let Some(account) = account {
        account_manager.activate_account(&requester).await?;

        let actor_store = actor_store
            .read(
                requester.clone(),
                Arc::new(S3BlobStore::new(requester.clone(), s3_config)),
            )
            .await?;
        let sync_data = actor_store.get_sync_event_data().await?;

        // @NOTE: we're over-emitting for now for backwards compatibility, can reduce this in the future
        let status = account_manager.get_account_status(&requester).await?;
        let mut lock = sequencer.sequencer.write().await;
        lock.sequence_account_evt(requester.clone(), status).await?;

        let handle = account.handle.unwrap_or(INVALID_HANDLE.to_string());
        lock.sequence_identity_evt(requester.clone(), Some(handle))
            .await?;
        lock.sequence_sync_evt(requester, sync_data).await?;
        Ok(())
    } else {
        tracing::error!("User not found");
        Err(ApiError::RuntimeError)
    }
}

#[tracing::instrument(skip_all)]
#[rocket::post("/xrpc/com.atproto.server.activateAccount")]
pub async fn activate_account(
    auth: AccessFull,
    sequencer: &State<SharedSequencer>,
    s3_config: &State<SdkConfig>,
    actor_store: &State<ActorStore>,
    account_manager: AccountManager,
) -> Result<(), ApiError> {
    match inner_activate_account(auth, sequencer, s3_config, actor_store, account_manager).await {
        Ok(_) => Ok(()),
        Err(error) => Err(error),
    }
}
