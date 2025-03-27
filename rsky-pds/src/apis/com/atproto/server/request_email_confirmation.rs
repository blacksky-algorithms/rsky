use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::auth_verifier::AccessStandardIncludeChecks;
use crate::mailer;
use crate::mailer::TokenParam;
use crate::models::models::EmailTokenPurpose;
use anyhow::{bail, Result};

async fn inner_request_email_confirmation(
    auth: AccessStandardIncludeChecks,
    account_manager: AccountManager,
) -> Result<()> {
    let did = auth.access.credentials.unwrap().did.unwrap();
    let account = account_manager
        .get_account(
            &did,
            Some(AvailabilityFlags {
                include_deactivated: Some(true),
                include_taken_down: Some(true),
            }),
        )
        .await?;
    if let Some(account) = account {
        if let Some(email) = account.email {
            let token = account_manager
                .create_email_token(&did, EmailTokenPurpose::ConfirmEmail)
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
    auth: AccessStandardIncludeChecks,
    account_manager: AccountManager,
) -> Result<(), ApiError> {
    match inner_request_email_confirmation(auth, account_manager).await {
        Ok(_) => Ok(()),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
