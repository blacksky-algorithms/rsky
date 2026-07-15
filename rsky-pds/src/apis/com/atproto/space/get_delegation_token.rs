use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::parse_space_uri;
use crate::apis::ApiError;
use crate::auth_verifier::AccessFull;
use crate::space_auth::{mint_delegation_token, session_permits};
use crate::space_scope::SpaceRequest;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::space::GetDelegationTokenOutput;

/// Mint a single-use delegation token for a space. Requires a covering `read`
/// grant (spec §Delegation token).
#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.space.getDelegationToken?<space>")]
pub async fn space_get_delegation_token(
    space: String,
    auth: AccessFull,
    actor_store: &State<ActorStore>,
) -> Result<Json<GetDelegationTokenOutput>, ApiError> {
    let space_id = parse_space_uri(&space)?;
    let credentials = auth.access.credentials.expect("credentials populated");
    let did = credentials.did.clone().expect("did populated");
    if !session_permits(&credentials, &did, &space_id, &SpaceRequest::Read) {
        return Err(ApiError::AuthRequiredError(
            "session does not cover this space".to_string(),
        ));
    }
    let keypair = actor_store.keypair(&did).await.map_err(|error| {
        tracing::error!("missing actor keypair: {error}");
        ApiError::RuntimeError
    })?;
    let token = mint_delegation_token(&keypair, &did, &space_id).map_err(|error| {
        tracing::error!("delegation token mint failed: {error}");
        ApiError::RuntimeError
    })?;
    Ok(Json(GetDelegationTokenOutput { token }))
}
