use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::{AccountManager, UpdateEmailOpts};
use crate::apis::ApiError;
use crate::auth_verifier::scope::{OAuthForbiddenEmail, Scoped};
use crate::auth_verifier::AccessFull;
use crate::models::models::EmailTokenPurpose;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::UpdateEmailInput;

/// Changes the account's address, with the mailed token when the current
/// address is confirmed.
pub(crate) async fn update_email_for(
    did: String,
    email: String,
    token: Option<String>,
    account_manager: &AccountManager,
) -> Result<(), ApiError> {
    if !mailchecker::is_valid(&email) {
        return Err(ApiError::InvalidRequest(
            "This email address is not supported, please use a different email.".to_string(),
        ));
    }
    let account = account_manager
        .get_account(
            &did,
            Some(AvailabilityFlags {
                include_deactivated: Some(true),
                include_taken_down: None,
            }),
        )
        .await?;

    let account = account.ok_or(ApiError::InvalidRequest("account not found".to_string()))?;
    // require valid token if account email is confirmed
    if account.email_confirmed_at.is_some() {
        match token {
            Some(token) => {
                account_manager
                    .assert_valid_email_token(&did, EmailTokenPurpose::UpdateEmail, &token)
                    .await?
            }
            None => {
                return Err(ApiError::InvalidRequest(
                    "Confirmation token required".to_string(),
                ))
            }
        }
    }
    account_manager
        .update_email(UpdateEmailOpts { did, email })
        .await?;
    Ok(())
}

#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.server.updateEmail",
    format = "json",
    data = "<body>"
)]
pub async fn update_email(
    body: Json<UpdateEmailInput>,
    auth: Scoped<OAuthForbiddenEmail, AccessFull>,
    account_manager: AccountManager,
) -> Result<(), ApiError> {
    let did = auth.did().await?;
    let UpdateEmailInput { email, token } = body.into_inner();
    update_email_for(did, email, token, &account_manager).await
}
