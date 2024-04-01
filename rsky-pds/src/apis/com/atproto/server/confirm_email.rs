use rsky_lexicon::com::atproto::server::ConfirmEmailInput;
use rocket::serde::json::Json;
use anyhow::{Result, bail};
use rocket::http::Status;
use rocket::response::status;
use crate::account_manager::{AccountManager, ConfirmEmailOpts};
use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::auth_verifier::AccessCheckTakedown;
use crate::models::{InternalErrorCode, InternalErrorMessageResponse};

async fn inner_confirm_email(
    body: Json<ConfirmEmailInput>,
    auth: AccessCheckTakedown,
) -> Result<()> {
    let did = auth.access.credentials.unwrap().did.unwrap();
    
    let user = AccountManager::get_account(&did, Some(AvailabilityFlags {
        include_deactivated: Some(true),
        include_taken_down: None
    })).await?;
    if let Some(user) = user {
        if let Some(user_email) = user.email {
            let ConfirmEmailInput {token, email} = body.into_inner();
            if user_email != email.to_lowercase() {
                bail!("Invalid Email")
            }
            AccountManager::confirm_email(ConfirmEmailOpts {
                did: &did,
                token: &token
            }).await?;
            Ok(())
        } else {
            bail!("Missing Email")
        }
    } else {
        bail!("Account not found")
    }
}

#[rocket::post(
    "/xrpc/com.atproto.server.confirmEmail",
    format = "json",
    data = "<body>"
)]
pub async fn confirm_email(
    body: Json<ConfirmEmailInput>,
    auth: AccessCheckTakedown,
) -> Result<(), status::Custom<Json<InternalErrorMessageResponse>>> {
    match inner_confirm_email(body, auth).await {
        Ok(()) => Ok(()),
        Err(error) => {
            eprintln!("Internal Error: {error}");
            let internal_error = InternalErrorMessageResponse {
                code: Some(InternalErrorCode::InternalError),
                message: Some("Internal error".to_string()),
            };
            return Err(status::Custom(
                Status::InternalServerError,
                Json(internal_error),
            ));
        }
    }
}
