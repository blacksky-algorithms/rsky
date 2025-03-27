use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::{AccountManager, UpdateEmailOpts};
use crate::apis::ApiError;
use crate::auth_verifier::AdminToken;
use anyhow::{bail, Result};
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::admin::UpdateAccountEmailInput;

async fn inner_update_account_email(
    body: Json<UpdateAccountEmailInput>,
    account_manager: AccountManager,
) -> Result<()> {
    let account = account_manager
        .get_account(
            &body.account,
            Some(AvailabilityFlags {
                include_deactivated: Some(true),
                include_taken_down: Some(true),
            }),
        )
        .await?;
    match account {
        None => bail!("Account does not exist: {}", body.account),
        Some(account) => {
            account_manager
                .update_email(UpdateEmailOpts {
                    did: account.did,
                    email: body.email.clone(),
                })
                .await
        }
    }
}

#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.admin.updateAccountEmail",
    format = "json",
    data = "<body>"
)]
pub async fn update_account_email(
    body: Json<UpdateAccountEmailInput>,
    _auth: AdminToken,
    account_manager: AccountManager,
) -> Result<(), ApiError> {
    match inner_update_account_email(body, account_manager).await {
        Ok(_) => Ok(()),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
