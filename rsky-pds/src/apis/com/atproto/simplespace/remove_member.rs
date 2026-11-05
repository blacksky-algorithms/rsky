use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::memberspace::remove_member::memberspace_remove_member;
use crate::apis::ApiError;
use crate::auth_verifier::AccessSpace;
use crate::config::ServerConfig;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::memberspace as new_memberspace;
#[allow(deprecated)]
use rsky_lexicon::com::atproto::simplespace::RemoveMemberInput;

/// DEPRECATED: Remove member using old simplespace API.
///
/// Supported indefinitely (legacy API)
#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.simplespace.removeMember",
    format = "json",
    data = "<body>"
)]
pub async fn simplespace_remove_member(
    body: Json<RemoveMemberInput>,
    auth: AccessSpace,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    server_config: &State<ServerConfig>,
) -> Result<(), ApiError> {
    // Legacy API - supported indefinitely
    // Translates old format to new and forwards

    let RemoveMemberInput { space, did } = body.into_inner();

    // Forward directly (same input format)
    memberspace_remove_member(
        Json(new_memberspace::RemoveMemberInput { space, did }),
        auth,
        actor_store,
        blobstore_factory,
        server_config,
    )
    .await
}
