use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use crate::auth_verifier::AdminToken;
use crate::config::ServerConfig;
use crate::lifecycle::{self, DeletionContext, LifecycleStore};
use crate::SharedSequencer;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::admin::DeleteAccountInput;

#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.admin.deleteAccount",
    format = "json",
    data = "<body>"
)]
#[allow(clippy::too_many_arguments)]
pub async fn delete_account(
    body: Json<DeleteAccountInput>,
    sequencer: &State<SharedSequencer>,
    blobstore_factory: &State<BlobstoreFactory>,
    _auth: AdminToken,
    actor_store: &State<ActorStore>,
    lifecycle_store: &State<LifecycleStore>,
    cfg: &State<ServerConfig>,
    account_manager: AccountManager,
) -> Result<(), ApiError> {
    let DeleteAccountInput { did } = body.into_inner();
    let blobstore = (!cfg.service.coexistence).then(|| blobstore_factory.blobstore(did.clone()));
    lifecycle::delete_account(
        &DeletionContext {
            lifecycle: lifecycle_store,
            account_manager: &account_manager,
            sequencer,
            actor_store,
            blobstore,
        },
        &did,
        None,
    )
    .await?;
    Ok(())
}
