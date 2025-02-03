use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::auth_verifier::AccessStandardIncludeChecks;
use crate::mailer;
use crate::mailer::TokenParam;
use crate::models::models::EmailTokenPurpose;
use anyhow::{bail, Result};

async fn inner_request_account_delete(auth: AccessStandardIncludeChecks) -> Result<()> {
    let did = auth.access.credentials.unwrap().did.unwrap();
    let account = AccountManager::get_account(
        &did,
        Some(AvailabilityFlags {
            include_deactivated: Some(true),
            include_taken_down: Some(true),
        }),
    )
    .await?;
    if let Some(account) = account {
        if let Some(email) = account.email {
            let token =
                AccountManager::create_email_token(&did, EmailTokenPurpose::DeleteAccount).await?;
            mailer::send_account_delete(email, TokenParam { token }).await?;
            Ok(())
        } else {
            bail!("Account does not have an email address")
        }
    } else {
        bail!("Account not found")
    }
}

#[rocket::post("/xrpc/com.atproto.server.requestAccountDelete")]
pub async fn request_account_delete(auth: AccessStandardIncludeChecks) -> Result<(), ApiError> {
    match inner_request_account_delete(auth).await {
        Ok(_) => Ok(()),
        Err(error) => {
            eprintln!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
