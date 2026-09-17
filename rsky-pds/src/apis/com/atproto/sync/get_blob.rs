use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::{BlobNotFoundError, BlobstoreFactory};
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::repo::assert_repo_availability;
use crate::apis::ApiError;
use crate::auth_verifier;
use crate::auth_verifier::OptionalAccessOrAdminToken;
use crate::exports::{BlobBody, ExportGuard, Exports};
use anyhow::Result;
use aws_sdk_s3::operation::get_object::GetObjectError;
use lexicon_cid::Cid;
use rocket::State;
use std::str::FromStr;

async fn inner_get_blob(
    did: String,
    cid: String,
    blobstore_factory: &State<BlobstoreFactory>,
    auth: OptionalAccessOrAdminToken,
    actor_store: &State<ActorStore>,
    account_manager: AccountManager,
    guard: ExportGuard,
) -> Result<BlobBody> {
    let is_user_or_admin = if let Some(access) = auth.access {
        auth_verifier::is_user_or_admin(access, &did)
    } else {
        false
    };
    let _ = assert_repo_availability(&did, is_user_or_admin, &account_manager).await?;

    let cid = Cid::from_str(&cid)?;
    let actor_store = actor_store
        .read(did.clone(), blobstore_factory.blobstore(did.clone()))
        .await?;

    if !is_user_or_admin
        && actor_store.blob.get_records_for_blob(cid).await?.is_empty()
        && actor_store
            .space
            .any_space_references_blob(&cid.to_string())
            .await?
    {
        return Err(BlobNotFoundError.into());
    }

    let found = actor_store.blob.get_blob(cid).await?;
    Ok(BlobBody::new(
        found.stream.into_async_read(),
        found.size as usize,
        found.mime_type,
        guard,
    ))
}

/// Get a blob associated with a given account. Returns the full blob as originally uploaded.
/// Does not require auth; implemented by PDS.
#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.sync.getBlob?<did>&<cid>")]
pub async fn get_blob(
    did: String,
    cid: String,
    blobstore_factory: &State<BlobstoreFactory>,
    auth: OptionalAccessOrAdminToken,
    actor_store: &State<ActorStore>,
    account_manager: AccountManager,
    exports: &State<Exports>,
) -> Result<BlobBody, ApiError> {
    let guard = exports.blob_slot().await?;
    match inner_get_blob(
        did,
        cid,
        blobstore_factory,
        auth,
        actor_store,
        account_manager,
        guard,
    )
    .await
    {
        Ok(body) => Ok(body),
        Err(error) => {
            tracing::error!("Error: {}", error);
            if error.downcast_ref::<BlobNotFoundError>().is_some()
                || matches!(error.downcast_ref(), Some(GetObjectError::NoSuchKey(_)))
            {
                Err(ApiError::BlobNotFound)
            } else {
                Err(ApiError::from(error))
            }
            // @TODO: Need to update error handling to return 404 if we have it but it's in tmp
        }
    }
}
