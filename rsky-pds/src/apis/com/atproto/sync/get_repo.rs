use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::repo::assert_repo_availability;
use crate::apis::ApiError;
use crate::auth_verifier;
use crate::auth_verifier::OptionalAccessOrAdminToken;
use crate::exports::{CarStream, ExportGuard, Exports};
use anyhow::{bail, Result};
use rocket::State;

async fn get_car_stream(
    blobstore_factory: &State<BlobstoreFactory>,
    did: String,
    since: Option<String>,
    actor_store: &State<ActorStore>,
    guard: ExportGuard,
) -> Result<CarStream> {
    let reader = actor_store
        .read(did.clone(), blobstore_factory.blobstore(did.clone()))
        .await?;
    let storage_guard = reader.storage.read().await;
    if let Ok(root) = storage_guard.get_root_detailed().await {
        actor_store.note_exposure(&did, &root.rev).await?;
    }
    match storage_guard
        .car_stream_within(since, crate::actor_store::repo::sql_repo::EXPORT_DEADLINE)
        .await
    {
        Err(_) => bail!("Could not find repo for DID: {did}"),
        Ok(stream) => Ok(CarStream::new(stream, guard)),
    }
}

async fn inner_get_repo(
    did: String,
    since: Option<String>, // The revision ('rev') of the repo to create a diff from.
    blobstore_factory: &State<BlobstoreFactory>,
    auth: OptionalAccessOrAdminToken,
    actor_store: &State<ActorStore>,
    account_manager: AccountManager,
    guard: ExportGuard,
) -> Result<CarStream> {
    let is_user_or_admin = if let Some(access) = auth.access {
        auth_verifier::is_user_or_admin(access, &did)
    } else {
        false
    };
    let _ = assert_repo_availability(&did, is_user_or_admin, &account_manager).await?;
    get_car_stream(blobstore_factory, did, since, actor_store, guard).await
}

/// Download a repository export as CAR file. Optionally only a 'diff' since a previous revision.
/// Does not require auth; implemented by PDS.
#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.sync.getRepo?<did>&<since>")]
pub async fn get_repo(
    did: String,
    since: Option<String>, // The revision ('rev') of the repo to create a diff from.
    blobstore_factory: &State<BlobstoreFactory>,
    auth: OptionalAccessOrAdminToken,
    actor_store: &State<ActorStore>,
    account_manager: AccountManager,
    exports: &State<Exports>,
) -> Result<CarStream, ApiError> {
    let guard = exports.repo_slot().await?;
    match inner_get_repo(
        did,
        since,
        blobstore_factory,
        auth,
        actor_store,
        account_manager,
        guard,
    )
    .await
    {
        Ok(res) => Ok(res),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::from(error))
        }
    }
}
