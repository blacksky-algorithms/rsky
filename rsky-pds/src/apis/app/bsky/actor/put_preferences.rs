use crate::actor_store::aws::s3::S3BlobStore;
use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use crate::auth_verifier::AccessStandard;
use crate::db::DbConn;
use anyhow::Result;
use aws_config::SdkConfig;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::app::bsky::actor::PutPreferencesInput;

async fn inner_put_preferences(
    body: Json<PutPreferencesInput>,
    s3_config: &State<SdkConfig>,
    auth: AccessStandard,
    db: DbConn,
) -> Result<(), ApiError> {
    let PutPreferencesInput { preferences } = body.into_inner();
    let auth = auth.access.credentials;
    let requester = auth.did.unwrap().clone();
    let actor_store = ActorStore::new(
        requester.clone(),
        S3BlobStore::new(requester.clone(), s3_config),
        db,
    );
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
    s3_config: &State<SdkConfig>,
    auth: AccessStandard,
    db: DbConn,
) -> Result<(), ApiError> {
    match inner_put_preferences(body, s3_config, auth, db).await {
        Ok(_) => Ok(()),
        Err(error) => Err(error),
    }
}
