use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::{AccountManager, UpdateEmailOpts};
use crate::auth_verifier::AdminToken;
use crate::models::{ErrorCode, ErrorMessageResponse};
use anyhow::{bail, Result};
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::admin::UpdateAccountEmailInput;

async fn inner_update_account_email(body: Json<UpdateAccountEmailInput>) -> Result<()> {
    let account = AccountManager::get_account(
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
            AccountManager::update_email(UpdateEmailOpts {
                did: account.did,
                email: body.email.clone(),
            })
            .await
        }
    }
}

#[rocket::post(
    "/xrpc/com.atproto.admin.updateAccountEmail",
    format = "json",
    data = "<body>"
)]
pub async fn update_account_email(
    body: Json<UpdateAccountEmailInput>,
    _auth: AdminToken,
) -> Result<(), status::Custom<Json<ErrorMessageResponse>>> {
    match inner_update_account_email(body).await {
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
