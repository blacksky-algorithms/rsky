use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::space_error;
use crate::apis::ApiError;
use crate::auth_verifier::AccessFull;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::space::ListSpacesOutput;

/// The spaces the caller holds a repo in.
#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.space.listSpaces?<limit>&<cursor>")]
pub async fn space_list_spaces(
    limit: Option<i64>,
    cursor: Option<String>,
    auth: AccessFull,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
) -> Result<Json<ListSpacesOutput>, ApiError> {
    let did = auth
        .access
        .credentials
        .expect("credentials populated")
        .did
        .expect("did populated");
    let limit = limit.unwrap_or(100).clamp(1, 1000) as usize;
    let reader = actor_store
        .read(did.clone(), blobstore_factory.blobstore(did.clone()))
        .await
        .map_err(|error| ApiError::BadRequest("RepoNotFound".to_string(), error.to_string()))?;
    let spaces = reader
        .space
        .list_spaces(limit, cursor)
        .await
        .map_err(space_error)?;
    let cursor = if spaces.len() == limit {
        spaces.last().cloned()
    } else {
        None
    };
    Ok(Json(ListSpacesOutput { cursor, spaces }))
}
