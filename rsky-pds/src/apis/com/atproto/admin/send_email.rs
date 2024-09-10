use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::auth_verifier::Moderator;
use crate::mailer::moderation::{HtmlMailOpts, ModerationMailer};
use crate::models::{ErrorCode, ErrorMessageResponse};
use anyhow::{bail, Result};
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::admin::{SendMailInput, SendMailOutput};

async fn inner_send_email(body: Json<SendMailInput>) -> Result<SendMailOutput> {
    let SendMailInput {
        content,
        recipient_did,
        subject,
        ..
    } = body.into_inner();
    let subject = subject.unwrap_or("Message via your PDS".to_string());

    let account = AccountManager::get_account(
        &recipient_did,
        Some(AvailabilityFlags {
            include_deactivated: Some(true),
            include_taken_down: Some(true),
        }),
    )
    .await?;

    match account {
        None => bail!("Recipient not found"),
        Some(account) => match account.email {
            None => bail!("account does not have an email address"),
            Some(email) => {
                ModerationMailer::send_html(HtmlMailOpts {
                    to: email,
                    subject,
                    html: content,
                })
                .await?;

                Ok(SendMailOutput { sent: true })
            }
        },
    }
}

#[rocket::post("/xrpc/com.atproto.admin.sendEmail", format = "json", data = "<body>")]
pub async fn send_email(
    body: Json<SendMailInput>,
    _auth: Moderator,
) -> Result<Json<SendMailOutput>, status::Custom<Json<ErrorMessageResponse>>> {
    match inner_send_email(body).await {
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
