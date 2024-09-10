use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::auth_verifier::AccessStandardIncludeChecks;
use crate::mailer;
use crate::mailer::TokenParam;
use crate::models::models::EmailTokenPurpose;
use crate::models::{ErrorCode, ErrorMessageResponse};
use anyhow::{bail, Result};
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::RequestEmailUpdateOutput;

async fn inner_request_email_update(
    auth: AccessStandardIncludeChecks,
) -> Result<RequestEmailUpdateOutput> {
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
            let token_required = account.email_confirmed_at.is_some();
            if token_required {
                let token =
                    AccountManager::create_email_token(&did, EmailTokenPurpose::UpdateEmail)
                        .await?;
                mailer::send_update_email(email, TokenParam { token }).await?;
            }

            Ok(RequestEmailUpdateOutput { token_required })
        } else {
            bail!("Account does not have an email address")
        }
    } else {
        bail!("Account not found")
    }
}

#[rocket::post("/xrpc/com.atproto.server.requestEmailUpdate")]
pub async fn request_email_update(
    auth: AccessStandardIncludeChecks,
) -> Result<Json<RequestEmailUpdateOutput>, status::Custom<Json<ErrorMessageResponse>>> {
    match inner_request_email_update(auth).await {
        Ok(res) => Ok(Json(res)),
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
