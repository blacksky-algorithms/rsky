use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::{assert_rpc_target, rpc_audience, ApiError};
use crate::auth_verifier::scope::{NoScopeRequired, Scoped};
use crate::pipethrough::ProxyRequest;
use anyhow::Result;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::app::bsky::actor::PutPreferencesInput;
use rsky_repo::types::Ids;

async fn inner_put_preferences(
    body: Json<PutPreferencesInput>,
    blobstore_factory: &State<BlobstoreFactory>,
    auth: Scoped<NoScopeRequired>,
    req: ProxyRequest<'_>,
    actor_store: &State<ActorStore>,
) -> Result<(), ApiError> {
    let PutPreferencesInput { preferences } = body.into_inner();
    let credentials = auth.credentials().await?.clone().unwrap();
    let lxm = Ids::AppBskyActorPutPreferences.as_str();
    if let Some(aud) = rpc_audience(&req, lxm) {
        assert_rpc_target(&Some(credentials.clone()), lxm, &aud)?;
    }
    let requester = auth.did().await?;
    let actor_store = actor_store
        .transact(
            requester.clone(),
            blobstore_factory.blobstore(requester.clone()),
        )
        .await?;
    actor_store
        .pref
        .put_preferences(
            preferences,
            "app.bsky".to_string(),
            credentials.scope.unwrap(),
        )
        .await?;
    Ok(())
}

#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/app.bsky.actor.putPreferences",
    format = "json",
    data = "<body>"
)]
pub async fn put_preferences(
    body: Json<PutPreferencesInput>,
    blobstore_factory: &State<BlobstoreFactory>,
    auth: Scoped<NoScopeRequired>,
    req: ProxyRequest<'_>,
    actor_store: &State<ActorStore>,
) -> Result<(), ApiError> {
    match inner_put_preferences(body, blobstore_factory, auth, req, actor_store).await {
        Ok(_) => Ok(()),
        Err(error) => Err(error),
    }
}
