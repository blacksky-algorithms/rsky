use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::{assert_rpc_target, rpc_audience, ApiError};
use crate::auth_verifier::scope::{NoScopeRequired, Scoped};
use crate::pipethrough::ProxyRequest;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::app::bsky::actor::{GetPreferencesOutput, RefPreferences};
use rsky_repo::types::Ids;

async fn inner_get_preferences(
    blobstore_factory: &State<BlobstoreFactory>,
    auth: Scoped<NoScopeRequired>,
    req: ProxyRequest<'_>,
    actor_store: &State<ActorStore>,
) -> Result<GetPreferencesOutput, ApiError> {
    let credentials = auth.credentials().await?.clone().unwrap();
    let lxm = Ids::AppBskyActorGetPreferences.as_str();
    if let Some(aud) = rpc_audience(&req, lxm) {
        assert_rpc_target(&Some(credentials.clone()), lxm, &aud)?;
    }
    let requester = auth.did().await?;
    let actor_store = actor_store
        .read(
            requester.clone(),
            blobstore_factory.blobstore(requester.clone()),
        )
        .await?;
    let preferences: Vec<RefPreferences> = actor_store
        .pref
        .get_preferences(Some("app.bsky".to_string()), credentials.scope.unwrap())
        .await?;

    Ok(GetPreferencesOutput { preferences })
}

/// Get private preferences attached to the current account. Expected use is synchronization
/// between multiple devices, and import/export during account migration. Requires auth.
#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/app.bsky.actor.getPreferences")]
pub async fn get_preferences(
    blobstore_factory: &State<BlobstoreFactory>,
    auth: Scoped<NoScopeRequired>,
    req: ProxyRequest<'_>,
    actor_store: &State<ActorStore>,
) -> Result<Json<GetPreferencesOutput>, ApiError> {
    match inner_get_preferences(blobstore_factory, auth, req, actor_store).await {
        Ok(res) => Ok(Json(res)),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(error)
        }
    }
}
