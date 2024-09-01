use crate::auth_verifier::AccessStandard;
use crate::models::{ErrorCode, ErrorMessageResponse};
use crate::repo::aws::s3::S3BlobStore;
use crate::repo::ActorStore;
use anyhow::Result;
use aws_config::SdkConfig;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::app::bsky::actor::PutPreferencesInput;

async fn inner_put_preferences(
    body: Json<PutPreferencesInput>,
    s3_config: &State<SdkConfig>,
    auth: AccessStandard,
) -> Result<()> {
    let PutPreferencesInput { preferences } = body.into_inner();
    let auth = auth.access.credentials.unwrap();
    let requester = auth.did.unwrap().clone();
    let actor_store = ActorStore::new(
        requester.clone(),
        S3BlobStore::new(requester.clone(), s3_config),
    );
    actor_store
        .pref
        .put_preferences(preferences, "app.bsky".to_string(), auth.scope.unwrap())
        .await
}

#[rocket::post(
    "/xrpc/app.bsky.actor.putPreferences",
    format = "json",
    data = "<body>"
)]
pub async fn put_preferences(
    body: Json<PutPreferencesInput>,
    s3_config: &State<SdkConfig>,
    auth: AccessStandard,
) -> Result<(), status::Custom<Json<ErrorMessageResponse>>> {
    match inner_put_preferences(body, s3_config, auth).await {
        Ok(_) => Ok(()),
        Err(error) => {
            eprintln!("@LOG: ERROR: {error}");
            let internal_error = ErrorMessageResponse {
                code: Some(ErrorCode::InternalServerError),
                message: Some(error.to_string()),
            };
            return Err(status::Custom(
                Status::InternalServerError,
                Json(internal_error),
            ));
        }
    }
}
