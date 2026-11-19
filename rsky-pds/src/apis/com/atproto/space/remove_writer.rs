use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::{parse_space_uri, require_manage, space_def_error};
use crate::apis::ApiError;
use crate::auth_verifier::AccessSpace;
use crate::space_scope::ManageOp;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::space::RemoveWriterInput;

/// Remove an account from a space's writer set. A removed account is tracked
/// again if it makes an authorized write (spec §The sync boundary).
#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.space.removeWriter",
    format = "json",
    data = "<body>"
)]
pub async fn space_remove_writer(
    body: Json<RemoveWriterInput>,
    auth: AccessSpace,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
) -> Result<(), ApiError> {
    let RemoveWriterInput { space, did: writer } = body.into_inner();
    let space_id = parse_space_uri(&space)?;
    let credentials = auth.access.credentials.expect("credentials populated");
    let did = credentials.did.clone().expect("did populated");
    require_manage(&credentials, &did, &space_id, ManageOp::Update)?;
    let reader = actor_store
        .read(did.clone(), blobstore_factory.blobstore(did.clone()))
        .await
        .map_err(|error| ApiError::BadRequest("RepoNotFound".to_string(), error.to_string()))?;
    reader
        .space
        .live_space_def(&space_id.uri())
        .await
        .map_err(space_def_error)?;
    reader
        .space
        .remove_writer(&space_id.uri(), &writer)
        .await
        .map_err(space_def_error)?;
    Ok(())
}
