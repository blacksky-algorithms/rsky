use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::auth_verifier::Moderator;
use crate::mailer::moderation::{HtmlMailOpts, ModerationMailer};
use anyhow::{bail, Result};
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::admin::{SendMailInput, SendMailOutput};

async fn inner_send_email(
    body: Json<SendMailInput>,
    account_manager: AccountManager,
) -> Result<SendMailOutput> {
    let SendMailInput {
        content,
        recipient_did,
        subject,
        ..
    } = body.into_inner();
    let subject = subject.unwrap_or("Message via your PDS".to_string());

    let account = account_manager
        .get_account(
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

#[tracing::instrument(skip_all)]
#[rocket::post("/xrpc/com.atproto.admin.sendEmail", format = "json", data = "<body>")]
pub async fn send_email(
    body: Json<SendMailInput>,
    _auth: Moderator,
    account_manager: AccountManager,
) -> Result<Json<SendMailOutput>, ApiError> {
    match inner_send_email(body, account_manager).await {
        Ok(res) => Ok(Json(res)),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
