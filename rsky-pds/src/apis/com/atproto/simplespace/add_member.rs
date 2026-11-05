use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::memberspace::put_member::memberspace_put_member;
use crate::apis::ApiError;
use crate::auth_verifier::AccessSpace;
use crate::config::ServerConfig;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::memberspace as new_memberspace;
#[allow(deprecated)]
use rsky_lexicon::com::atproto::simplespace::AddMemberInput;

/// DEPRECATED: Add member using old simplespace API.
///
/// Old API granted full read+write access. This translates to read=true,
/// write=true in the new API.
///
/// Supported indefinitely (legacy API)
#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.simplespace.addMember",
    format = "json",
    data = "<body>"
)]
pub async fn simplespace_add_member(
    body: Json<AddMemberInput>,
    auth: AccessSpace,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    server_config: &State<ServerConfig>,
) -> Result<(), ApiError> {
    // Legacy API - supported indefinitely
    // Translates old format to new and forwards

    let AddMemberInput {
        space,
        did: member_did,
    } = body.into_inner();

    // Old API granted full access - translate to read=true, write=true
    let new_input = new_memberspace::PutMemberInput {
        space,
        did: member_did,
        read: true,
        write: true,
    };

    // Forward to new implementation
    memberspace_put_member(
        Json(new_input),
        auth,
        actor_store,
        blobstore_factory,
        server_config,
    )
    .await
}
