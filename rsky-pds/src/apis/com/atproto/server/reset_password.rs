use crate::account_manager::{AccountManager, ResetPasswordOpts};
use crate::apis::ApiError;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::ResetPasswordInput;

/// The reference PDS refuses passwords longer than this.
const NEW_PASSWORD_MAX_LENGTH: usize = 256;

#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.server.resetPassword",
    format = "json",
    data = "<body>"
)]
pub async fn reset_password(
    body: Json<ResetPasswordInput>,
    account_manager: AccountManager,
) -> Result<(), ApiError> {
    let ResetPasswordInput { token, password } = body.into_inner();
    if password.len() > NEW_PASSWORD_MAX_LENGTH {
        return Err(ApiError::InvalidRequest(
            "Invalid password length.".to_string(),
        ));
    }
    account_manager
        .reset_password(ResetPasswordOpts { token, password })
        .await?;
    Ok(())
}
