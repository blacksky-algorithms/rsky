use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use crate::auth_verifier::AccessStandard;
use anyhow::Result;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::app::bsky::actor::{GetPreferencesOutput, RefPreferences};

async fn inner_get_preferences(
    blobstore_factory: &State<BlobstoreFactory>,
    auth: AccessStandard,
    actor_store: &State<ActorStore>,
) -> Result<GetPreferencesOutput> {
    let auth = auth.access.credentials.unwrap();
    let requester = auth.did.unwrap().clone();
    let actor_store = actor_store
        .read(
            requester.clone(),
            blobstore_factory.blobstore(requester.clone()),
        )
        .await?;
    let preferences: Vec<RefPreferences> = actor_store
        .pref
        .get_preferences(Some("app.bsky".to_string()), auth.scope.unwrap())
        .await?;

    Ok(GetPreferencesOutput { preferences })
}

/// Get private preferences attached to the current account. Expected use is synchronization
/// between multiple devices, and import/export during account migration. Requires auth.
#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/app.bsky.actor.getPreferences")]
pub async fn get_preferences(
    blobstore_factory: &State<BlobstoreFactory>,
    auth: AccessStandard,
    actor_store: &State<ActorStore>,
) -> Result<Json<GetPreferencesOutput>, ApiError> {
    match inner_get_preferences(blobstore_factory, auth, actor_store).await {
        Ok(res) => Ok(Json(res)),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
