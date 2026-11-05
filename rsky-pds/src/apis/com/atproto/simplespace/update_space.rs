use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::simplespace::adapters::config_to_policy;
use crate::apis::com::atproto::space::update_space::space_update_space;
use crate::apis::ApiError;
use crate::auth_verifier::AccessSpace;
use crate::config::ServerConfig;
use rocket::serde::json::Json;
use rocket::State;
#[allow(deprecated)]
use rsky_lexicon::com::atproto::simplespace::UpdateSpaceInput;
use rsky_lexicon::com::atproto::space as new_space;

/// DEPRECATED: Update space config using old simplespace API.
///
/// Supported indefinitely (legacy API)
#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.simplespace.updateSpace",
    format = "json",
    data = "<body>"
)]
pub async fn simplespace_update_space(
    body: Json<UpdateSpaceInput>,
    auth: AccessSpace,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    server_config: &State<ServerConfig>,
) -> Result<(), ApiError> {
    // Legacy API - supported indefinitely
    // Translates old format to new and forwards

    let UpdateSpaceInput { space, config } = body.into_inner();

    // Translate config to policy
    let policy = config_to_policy(&config)?;

    // Serialize policy to Value for the new API
    let policy_value = serde_json::to_value(&policy)
        .map_err(|e| ApiError::InvalidRequest(format!("Failed to serialize policy: {e}")))?;

    // Forward to new implementation
    space_update_space(
        Json(new_space::UpdateSpaceInput {
            space,
            policy: policy_value,
        }),
        auth,
        actor_store,
        blobstore_factory,
        server_config,
    )
    .await
}
