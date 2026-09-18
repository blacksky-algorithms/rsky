use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::auth_verifier::scope::{AccountEmail, Scoped};
use crate::auth_verifier::AccessStandardIncludeChecks;
use crate::mailer;
use crate::mailer::TokenParam;
use crate::models::models::EmailTokenPurpose;
use crate::rate_limits::{Caller, RateLimits};
use anyhow::{bail, Result};
use rocket::State;

/// Mails a confirmation token to the account's address.
pub(crate) async fn request_email_confirmation_for(
    did: &str,
    account_manager: &AccountManager,
) -> Result<()> {
    let account = account_manager
        .get_account(
            did,
            Some(AvailabilityFlags {
                include_deactivated: Some(true),
                include_taken_down: Some(true),
            }),
        )
        .await?;
    if let Some(account) = account {
        if let Some(email) = account.email {
            let token = account_manager
                .create_email_token(did, EmailTokenPurpose::ConfirmEmail)
                .await?;
            mailer::send_confirm_email(email, TokenParam { token }).await?;
            Ok(())
        } else {
            bail!("Account does not have an email address")
        }
    } else {
        bail!("Account not found")
    }
}

#[tracing::instrument(skip_all)]
#[rocket::post("/xrpc/com.atproto.server.requestEmailConfirmation")]
pub async fn request_email_confirmation(
    auth: Scoped<AccountEmail, AccessStandardIncludeChecks>,
    account_manager: AccountManager,
    limits: &State<RateLimits>,
    caller: Caller,
) -> Result<(), ApiError> {
    let did = auth.did().await?;
    limits
        .consume_all(
            &crate::rate_limits::REQUEST_EMAIL_CONFIRMATION,
            &did,
            1,
            caller.bypass,
        )
        .await?;
    match request_email_confirmation_for(&did, &account_manager).await {
        Ok(_) => Ok(()),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
