use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::delete_space::space_delete_space;
use crate::apis::ApiError;
use crate::auth_verifier::AccessSpace;
use crate::config::ServerConfig;
use rocket::serde::json::Json;
use rocket::State;
#[allow(deprecated)]
use rsky_lexicon::com::atproto::simplespace::DeleteSpaceInput;
use rsky_lexicon::com::atproto::space as new_space;

/// DEPRECATED: Delete space using old simplespace API.
///
/// Supported indefinitely (legacy API)
#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.simplespace.deleteSpace",
    format = "json",
    data = "<body>"
)]
pub async fn simplespace_delete_space(
    body: Json<DeleteSpaceInput>,
    auth: AccessSpace,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    server_config: &State<ServerConfig>,
) -> Result<(), ApiError> {
    // Legacy API - supported indefinitely
    // Translates old format to new and forwards

    let space = body.into_inner().space;

    // Forward directly (no translation needed)
    space_delete_space(
        Json(new_space::DeleteSpaceInput { space }),
        auth,
        actor_store,
        blobstore_factory,
        server_config,
    )
    .await
}
