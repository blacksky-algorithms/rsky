use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use rocket::serde::json::Json;
use rocket::State;

#[derive(Debug, Deserialize, Serialize)]
pub struct ReserveSigningKeyInput {
    pub did: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReserveSigningKeyOutput {
    pub signing_key: String,
}

#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.server.reserveSigningKey",
    format = "json",
    data = "<body>"
)]
pub async fn reserve_signing_key(
    body: Json<ReserveSigningKeyInput>,
    actor_store: &State<ActorStore>,
) -> Result<Json<ReserveSigningKeyOutput>, ApiError> {
    let ReserveSigningKeyInput { did } = body.into_inner();
    match actor_store.reserve_keypair(did.as_deref()).await {
        Ok(signing_key) => Ok(Json(ReserveSigningKeyOutput { signing_key })),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
