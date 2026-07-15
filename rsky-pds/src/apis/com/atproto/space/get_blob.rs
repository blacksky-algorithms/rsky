use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::{BlobNotFoundError, BlobstoreFactory};
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::{
    internal_error, open_local_repo, parse_space_uri, space_error,
};
use crate::apis::ApiError;
use crate::space_auth::{authorize_space_read, SpaceReadAuth};
use crate::space_scope::SpaceRequest;
use aws_sdk_s3::primitives::AggregatedBytes;
use lexicon_cid::Cid;
use rocket::http::Header;
use rocket::{Responder, State};
use std::str::FromStr;

#[derive(Responder)]
#[response(status = 200)]
pub struct SpaceBlobResponder(Vec<u8>, Header<'static>, Header<'static>, Header<'static>);

/// Fetch a blob referenced by a record in the space. Authorization requires
/// the blob to be referenced from the space (space_blob_ref).
#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.space.getBlob?<space>&<did>&<cid>")]
pub async fn space_get_blob(
    space: String,
    did: String,
    cid: String,
    auth: SpaceReadAuth,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    account_manager: AccountManager,
) -> Result<SpaceBlobResponder, ApiError> {
    let space_id = parse_space_uri(&space)?;
    authorize_space_read(
        &auth,
        &space_id,
        &did,
        &SpaceRequest::ReadSelf { collection: None },
    )?;
    let reader = open_local_repo(
        actor_store,
        blobstore_factory,
        &account_manager,
        &did,
        false,
    )
    .await?;
    reader
        .space
        .live_repo_state(&space_id.uri())
        .await
        .map_err(space_error)?;
    let referenced = reader
        .space
        .space_references_blob(&space_id.uri(), &cid)
        .await
        .map_err(space_error)?;
    if !referenced {
        return Err(ApiError::BlobNotFound);
    }
    let parsed_cid =
        Cid::from_str(&cid).map_err(|_| ApiError::InvalidRequest(format!("invalid cid: {cid}")))?;
    match reader.blob.get_blob(parsed_cid).await {
        Ok(found) => {
            let bytes: AggregatedBytes = found
                .stream
                .collect()
                .await
                .map_err(internal_error("blob read failed"))?;
            let bytes = bytes.to_vec();
            Ok(SpaceBlobResponder(
                bytes.clone(),
                Header::new("content-length", bytes.len().to_string()),
                Header::new(
                    "content-type",
                    found
                        .mime_type
                        .unwrap_or("application/octet-stream".to_string()),
                ),
                Header::new("content-security-policy", "default-src 'none'; sandbox"),
            ))
        }
        Err(error) => {
            if error.downcast_ref::<BlobNotFoundError>().is_some() {
                Err(ApiError::BlobNotFound)
            } else {
                tracing::error!("blob fetch failed: {error}");
                Err(ApiError::RuntimeError)
            }
        }
    }
}
