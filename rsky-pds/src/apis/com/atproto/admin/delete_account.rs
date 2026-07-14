use crate::account_manager::helpers::account::AccountStatus;
use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use crate::auth_verifier::AdminToken;
use crate::{sequencer, SharedSequencer};
use anyhow::Result;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::admin::DeleteAccountInput;

async fn inner_delete_account(
    body: Json<DeleteAccountInput>,
    sequencer: &State<SharedSequencer>,
    blobstore_factory: &State<BlobstoreFactory>,
    actor_store: &State<ActorStore>,
    account_manager: AccountManager,
) -> Result<()> {
    let DeleteAccountInput { did } = body.into_inner();

    actor_store
        .destroy(&did, blobstore_factory.blobstore(did.clone()))
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
    blobstore_factory: &State<BlobstoreFactory>,
    _auth: AdminToken,
    actor_store: &State<ActorStore>,
    account_manager: AccountManager,
) -> Result<(), ApiError> {
    match inner_delete_account(
        body,
        sequencer,
        blobstore_factory,
        actor_store,
        account_manager,
    )
    .await
    {
        Ok(_) => Ok(()),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
