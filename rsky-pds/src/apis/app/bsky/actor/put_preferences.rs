use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use crate::auth_verifier::AccessStandard;
use anyhow::Result;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::app::bsky::actor::PutPreferencesInput;

async fn inner_put_preferences(
    body: Json<PutPreferencesInput>,
    blobstore_factory: &State<BlobstoreFactory>,
    auth: AccessStandard,
    actor_store: &State<ActorStore>,
) -> Result<(), ApiError> {
    let PutPreferencesInput { preferences } = body.into_inner();
    let auth = auth.access.credentials.unwrap();
    let requester = auth.did.unwrap().clone();
    let actor_store = actor_store
        .transact(
            requester.clone(),
            blobstore_factory.blobstore(requester.clone()),
        )
        .await?;
    actor_store
        .pref
        .put_preferences(preferences, "app.bsky".to_string(), auth.scope.unwrap())
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
    auth: AccessStandard,
    actor_store: &State<ActorStore>,
) -> Result<(), ApiError> {
    match inner_put_preferences(body, blobstore_factory, auth, actor_store).await {
        Ok(_) => Ok(()),
        Err(error) => Err(error),
    }
}
