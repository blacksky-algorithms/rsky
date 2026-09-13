use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::auth_verifier::scope::{OAuthForbidden, Scoped};
use crate::auth_verifier::AccessFullCheckTakedown;
use crate::mailer;
use crate::mailer::TokenParam;
use crate::models::models::EmailTokenPurpose;
use crate::rate_limits::{Caller, RateLimits};
use rocket::State;

/// Mails the caller the token `deleteAccount` requires. Full access only,
/// from an account that is not taken down, like the reference PDS.
#[tracing::instrument(skip_all)]
#[rocket::post("/xrpc/com.atproto.server.requestAccountDelete")]
pub async fn request_account_delete(
    auth: Scoped<OAuthForbidden, AccessFullCheckTakedown>,
    account_manager: AccountManager,
    limits: &State<RateLimits>,
    caller: Caller,
) -> Result<(), ApiError> {
    let did = auth.did().await?;
    limits
        .consume_all(
            &crate::rate_limits::REQUEST_ACCOUNT_DELETE,
            &did,
            1,
            caller.bypass,
        )
        .await?;
    let account = account_manager
        .get_account(
            &did,
            Some(AvailabilityFlags {
                include_deactivated: Some(true),
                include_taken_down: Some(true),
            }),
        )
        .await?;
    let account = account.ok_or(ApiError::InvalidRequest("account not found".to_string()))?;
    let Some(email) = account.email else {
        return Err(ApiError::InvalidRequest(
            "account does not have an email address".to_string(),
        ));
    };
    let token = account_manager
        .create_email_token(&did, EmailTokenPurpose::DeleteAccount)
        .await?;
    mailer::send_account_delete(email, TokenParam { token }).await?;
    Ok(())
}
