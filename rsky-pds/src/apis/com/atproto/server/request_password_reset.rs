use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::auth_verifier::AccessStandardIncludeChecks;
use crate::mailer;
use crate::mailer::IdentifierAndTokenParams;
use crate::models::models::EmailTokenPurpose;
use crate::models::{ErrorCode, ErrorMessageResponse};
use anyhow::{bail, Result};
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::RequestPasswordResetInput;

async fn inner_request_password_reset(body: Json<RequestPasswordResetInput>) -> Result<()> {
    let RequestPasswordResetInput { email } = body.into_inner();
    let email = email.to_lowercase();

    let account = AccountManager::get_account_by_email(
        &email,
        Some(AvailabilityFlags {
            include_deactivated: Some(true),
            include_taken_down: Some(true),
        }),
    )
    .await?;

    if let Some(account) = account {
        if let Some(email) = account.email {
            let token =
                AccountManager::create_email_token(&account.did, EmailTokenPurpose::ResetPassword)
                    .await?;
            mailer::send_reset_password(
                email.clone(),
                IdentifierAndTokenParams {
                    identifier: account.handle.unwrap_or(email),
                    token,
                },
            )
            .await?;
            Ok(())
        } else {
            bail!("Account does not have an email address")
        }
    } else {
        bail!("Account not found")
    }
}

#[rocket::post(
    "/xrpc/com.atproto.server.requestPasswordReset",
    format = "json",
    data = "<body>"
)]
pub async fn request_password_reset(
    body: Json<RequestPasswordResetInput>,
    _auth: AccessStandardIncludeChecks,
) -> Result<(), status::Custom<Json<ErrorMessageResponse>>> {
    match inner_request_password_reset(body).await {
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
