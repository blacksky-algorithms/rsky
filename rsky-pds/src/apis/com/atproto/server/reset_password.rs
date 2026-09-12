use crate::account_manager::{AccountManager, ResetPasswordOpts};
use crate::apis::ApiError;
use crate::rate_limits::{Caller, RateLimits};
use rocket::serde::json::Json;
use rocket::State;
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
    limits: &State<RateLimits>,
    caller: Caller,
) -> Result<(), ApiError> {
    limits
        .consume_all(
            &crate::rate_limits::RESET_PASSWORD,
            &caller.ip,
            1,
            caller.bypass,
        )
        .await?;
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
