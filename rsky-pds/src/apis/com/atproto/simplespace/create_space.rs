use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::simplespace::adapters::config_to_policy;
use crate::apis::com::atproto::space::create_space::space_create_space;
use crate::apis::ApiError;
use crate::auth_verifier::AccessSpace;
use crate::config::ServerConfig;
use rocket::serde::json::Json;
use rocket::State;
#[allow(deprecated)]
use rsky_lexicon::com::atproto::simplespace::{CreateSpaceInput, CreateSpaceOutput};
use rsky_lexicon::com::atproto::space as new_space;

/// DEPRECATED: Create a space using old simplespace API.
///
/// This endpoint is maintained for backward compatibility. New applications
/// should use com.atproto.space.createSpace instead.
///
/// Supported indefinitely (legacy API)
#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.simplespace.createSpace",
    format = "json",
    data = "<body>"
)]
pub async fn simplespace_create_space(
    body: Json<CreateSpaceInput>,
    auth: AccessSpace,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    server_config: &State<ServerConfig>,
) -> Result<Json<CreateSpaceOutput>, ApiError> {
    // Legacy API - supported indefinitely
    // Translates old format to new and forwards

    let CreateSpaceInput {
        space_type,
        skey,
        config,
    } = body.into_inner();

    // Translate old config format to new policy format
    // Note: config is Option<Config> in CreateSpaceInput - use default if None
    let config = config.unwrap_or_default();
    let policy = config_to_policy(&config)?;

    // Serialize policy to Value for the new API
    let policy_value = serde_json::to_value(&policy)
        .map_err(|e| ApiError::InvalidRequest(format!("Failed to serialize policy: {e}")))?;

    // Build new API request
    let new_input = new_space::CreateSpaceInput {
        space_type,
        skey,
        policy: policy_value,
    };

    // Forward to new implementation
    let result = space_create_space(
        Json(new_input),
        auth,
        actor_store,
        blobstore_factory,
        server_config,
    )
    .await?;

    // Return response
    Ok(Json(CreateSpaceOutput {
        uri: result.into_inner().uri,
    }))
}
