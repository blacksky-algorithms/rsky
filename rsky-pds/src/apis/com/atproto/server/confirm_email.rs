use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::{AccountManager, ConfirmEmailOpts};
use crate::apis::ApiError;
use crate::auth_verifier::scope::{AccountEmail, Scoped};
use crate::auth_verifier::AccessStandardIncludeChecks;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::ConfirmEmailInput;

/// Confirms `email` as the account's address with the mailed token.
#[tracing::instrument(skip_all)]
pub(crate) async fn confirm_email_for(
    did: &str,
    email: &str,
    token: &str,
    account_manager: &AccountManager,
) -> Result<(), ApiError> {
    let user = account_manager
        .get_account(
            did,
            Some(AvailabilityFlags {
                include_deactivated: Some(true),
                include_taken_down: Some(true),
            }),
        )
        .await?;
    let Some(user) = user else {
        return Err(ApiError::BadRequest(
            "AccountNotFound".to_string(),
            "user not found".to_string(),
        ));
    };
    if user.email.as_deref() != Some(email.to_lowercase().as_str()) {
        return Err(ApiError::InvalidEmail);
    }
    let (did, token) = (did.to_string(), token.to_string());
    account_manager
        .confirm_email(ConfirmEmailOpts {
            did: &did,
            token: &token,
        })
        .await?;
    Ok(())
}

#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.server.confirmEmail",
    format = "json",
    data = "<body>"
)]
pub async fn confirm_email(
    body: Json<ConfirmEmailInput>,
    auth: Scoped<AccountEmail, AccessStandardIncludeChecks>,
    account_manager: AccountManager,
) -> Result<(), ApiError> {
    let did = auth.did().await?;
    let ConfirmEmailInput { token, email } = body.into_inner();
    confirm_email_for(&did, &email, &token, &account_manager).await
}
