use crate::account_manager::helpers::account::{AccountHelperError, AvailabilityFlags};
use crate::account_manager::{AccountManager, UpdateEmailOpts};
use crate::auth_verifier::AccessFull;
use crate::models::models::EmailTokenPurpose;
use crate::models::{ErrorCode, ErrorMessageResponse};
use anyhow::{bail, Result};
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::UpdateEmailInput;

async fn inner_update_email(body: Json<UpdateEmailInput>, auth: AccessFull) -> Result<()> {
    let did = auth.access.credentials.unwrap().did.unwrap();
    let UpdateEmailInput { email, token } = body.into_inner();
    if !mailchecker::is_valid(&email) {
        bail!("This email address is not supported, please use a different email.")
    }
    let account = AccountManager::get_account(
        &did,
        Some(AvailabilityFlags {
            include_deactivated: Some(true),
            include_taken_down: None,
        }),
    )
    .await?;

    if let Some(account) = account {
        // require valid token if account email is confirmed
        if let Some(_) = account.email_confirmed_at {
            if let Some(token) = token {
                AccountManager::assert_valid_email_token(
                    &did,
                    EmailTokenPurpose::UpdateEmail,
                    &token,
                )
                .await?;
            } else {
                bail!("Confirmation token required")
            }
        }
        match AccountManager::update_email(UpdateEmailOpts { did, email }).await {
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

#[rocket::post(
    "/xrpc/com.atproto.server.updateEmail",
    format = "json",
    data = "<body>"
)]
pub async fn update_email(
    body: Json<UpdateEmailInput>,
    auth: AccessFull,
) -> Result<(), status::Custom<Json<ErrorMessageResponse>>> {
    match inner_update_email(body, auth).await {
        Ok(_) => Ok(()),
        Err(error) => {
            eprintln!("@LOG: ERROR: {error}");
            let internal_error = ErrorMessageResponse {
                code: Some(ErrorCode::InternalServerError),
                message: Some(error.to_string()),
            };
            return Err(status::Custom(
                Status::InternalServerError,
                Json(internal_error),
            ));
        }
    }
}
