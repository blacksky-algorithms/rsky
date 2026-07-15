use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::host::{def_to_config, local_space_def};
use crate::apis::com::atproto::space::parse_space_uri;
use crate::apis::ApiError;
use crate::space_auth::SpaceCredentialAuth;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::space::{GetSpaceOutput, SpaceConfig};

/// Describe a space anchored on a local account (space-host role).
#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.space.getSpace?<space>")]
pub async fn space_get_space(
    space: String,
    auth: SpaceCredentialAuth,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
) -> Result<Json<GetSpaceOutput>, ApiError> {
    let space_id = parse_space_uri(&space)?;
    if auth.space_uri != space_id.uri() {
        return Err(ApiError::InvalidToken);
    }
    let (def, _, _) = local_space_def(actor_store, blobstore_factory, &space_id).await?;
    Ok(Json(GetSpaceOutput {
        space: space_id.uri(),
        config: SpaceConfig::Simplespace(def_to_config(&def)),
    }))
}
