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
use rsky_lexicon::app::bsky::actor::{GetPreferencesOutput, RefPreferences};

async fn inner_get_preferences(
    s3_config: &State<SdkConfig>,
    auth: AccessStandard,
) -> Result<GetPreferencesOutput> {
    let auth = auth.access.credentials.unwrap();
    let requester = auth.did.unwrap().clone();
    let actor_store = ActorStore::new(
        requester.clone(),
        S3BlobStore::new(requester.clone(), s3_config),
    );
    let preferences: Vec<RefPreferences> = actor_store
        .pref
        .get_preferences(Some("app.bsky".to_string()), auth.scope.unwrap())
        .await?;

    Ok(GetPreferencesOutput { preferences })
}

/// Get private preferences attached to the current account. Expected use is synchronization
/// between multiple devices, and import/export during account migration. Requires auth.
#[rocket::get("/xrpc/app.bsky.actor.getPreferences")]
pub async fn get_preferences(
    s3_config: &State<SdkConfig>,
    auth: AccessStandard,
) -> Result<Json<GetPreferencesOutput>, status::Custom<Json<ErrorMessageResponse>>> {
    match inner_get_preferences(s3_config, auth).await {
        Ok(res) => Ok(Json(res)),
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
