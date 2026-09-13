use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::{AccountManager, ConfirmEmailOpts};
use crate::apis::ApiError;
use crate::auth_verifier::scope::{AccountEmail, Scoped};
use crate::auth_verifier::AccessStandardIncludeChecks;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::ConfirmEmailInput;

#[tracing::instrument(skip_all)]
async fn inner_confirm_email(
    body: Json<ConfirmEmailInput>,
    auth: Scoped<AccountEmail, AccessStandardIncludeChecks>,
    account_manager: AccountManager,
) -> Result<(), ApiError> {
    let did = auth.did().await?;
    let user = account_manager
        .get_account(
            &did,
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
    let ConfirmEmailInput { token, email } = body.into_inner();
    if user.email.as_deref() != Some(email.to_lowercase().as_str()) {
        return Err(ApiError::InvalidEmail);
    }
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
    match inner_confirm_email(body, auth, account_manager).await {
        Ok(()) => Ok(()),
        Err(error) => Err(error),
    }
}
