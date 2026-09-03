use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::auth_verifier::scope::{OAuthForbidden, Scoped};
use crate::auth_verifier::AccessFull;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::{CreateAppPasswordInput, CreateAppPasswordOutput};

#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.server.createAppPassword",
    format = "json",
    data = "<body>"
)]
pub async fn create_app_password(
    body: Json<CreateAppPasswordInput>,
    auth: Scoped<OAuthForbidden, AccessFull>,
    account_manager: AccountManager,
) -> Result<Json<CreateAppPasswordOutput>, ApiError> {
    let CreateAppPasswordInput { name } = body.into_inner();
    match account_manager
        .create_app_password(auth.did().await?, name)
        .await
    {
        Ok(app_password) => Ok(Json(app_password)),
        Err(error) => {
            tracing::error!("Internal Error: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
