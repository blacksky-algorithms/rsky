use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::repo::assert_repo_availability;
use crate::apis::ApiError;
use crate::auth_verifier;
use crate::auth_verifier::OptionalAccessOrAdminToken;
use anyhow::{bail, Result};
use rocket::{Responder, State};

#[derive(Responder)]
#[response(status = 200, content_type = "application/vnd.ipld.car")]
pub struct CheckoutResponder(Vec<u8>);

async fn inner_get_checkout(
    did: String,
    blobstore_factory: &State<BlobstoreFactory>,
    auth: OptionalAccessOrAdminToken,
    actor_store: &State<ActorStore>,
    account_manager: AccountManager,
) -> Result<Vec<u8>> {
    let is_user_or_admin = if let Some(access) = auth.access {
        auth_verifier::is_user_or_admin(access, &did)
    } else {
        false
    };
    let _ = assert_repo_availability(&did, is_user_or_admin, &account_manager).await?;
    let actor_store = actor_store
        .read(did.clone(), blobstore_factory.blobstore(did.clone()))
        .await?;
    let storage_guard = actor_store.storage.read().await;
    match storage_guard.get_car_stream(None).await {
        Err(_) => bail!("Could not find repo for DID: {did}"),
        Ok(carstream) => Ok(carstream),
    }
}

/// DEPRECATED - please use com.atproto.sync.getRepo instead
#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.sync.getCheckout?<did>")]
pub async fn get_checkout(
    did: String,
    blobstore_factory: &State<BlobstoreFactory>,
    auth: OptionalAccessOrAdminToken,
    actor_store: &State<ActorStore>,
    account_manager: AccountManager,
) -> Result<CheckoutResponder, ApiError> {
    match inner_get_checkout(did, blobstore_factory, auth, actor_store, account_manager).await {
        Ok(res) => Ok(CheckoutResponder(res)),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
