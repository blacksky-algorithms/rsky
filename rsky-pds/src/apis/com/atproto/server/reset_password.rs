use crate::account_manager::{AccountManager, ResetPasswordOpts};
use crate::apis::ApiError;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::ResetPasswordInput;

#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.server.resetPassword",
    format = "json",
    data = "<body>"
)]
pub async fn reset_password(body: Json<ResetPasswordInput>) -> Result<(), ApiError> {
    let ResetPasswordInput { token, password } = body.into_inner();
    match AccountManager::reset_password(ResetPasswordOpts { token, password }).await {
        Ok(_) => Ok(()),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
