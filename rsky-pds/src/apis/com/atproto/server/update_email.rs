use crate::account_manager::helpers::account::{AccountHelperError, AvailabilityFlags};
use crate::account_manager::{AccountManager, UpdateEmailOpts};
use crate::apis::ApiError;
use crate::auth_verifier::AccessFull;
use crate::models::models::EmailTokenPurpose;
use anyhow::{bail, Result};
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::UpdateEmailInput;

async fn inner_update_email(
    body: Json<UpdateEmailInput>,
    auth: AccessFull,
    account_manager: AccountManager,
) -> Result<()> {
    let did = auth.access.credentials.did.unwrap();
    let UpdateEmailInput { email, token } = body.into_inner();
    if !mailchecker::is_valid(&email) {
        bail!("This email address is not supported, please use a different email.")
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

    if let Some(account) = account {
        // require valid token if account email is confirmed
        if account.email_confirmed_at.is_some() {
            if let Some(token) = token {
                account_manager
                    .assert_valid_email_token(&did, EmailTokenPurpose::UpdateEmail, &token)
                    .await?;
            } else {
                bail!("Confirmation token required")
            }
        }
        match account_manager
            .update_email(UpdateEmailOpts { did, email })
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => match e.downcast_ref() {
                Some(AccountHelperError::UserAlreadyExistsError) => {
                    bail!("This email address is already in use, please use a different email.")
                }
                _ => Err(e),
            },
        }
    } else {
        bail!("Account not found")
    }
}

#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.server.updateEmail",
    format = "json",
    data = "<body>"
)]
pub async fn update_email(
    body: Json<UpdateEmailInput>,
    auth: AccessFull,
    account_manager: AccountManager,
) -> Result<(), ApiError> {
    match inner_update_email(body, auth, account_manager).await {
        Ok(_) => Ok(()),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
