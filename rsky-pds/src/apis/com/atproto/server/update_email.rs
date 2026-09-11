use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::{AccountManager, UpdateEmailOpts};
use crate::apis::ApiError;
use crate::auth_verifier::AccessFull;
use crate::models::models::EmailTokenPurpose;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::UpdateEmailInput;

async fn inner_update_email(
    body: Json<UpdateEmailInput>,
    auth: AccessFull,
    account_manager: AccountManager,
) -> Result<(), ApiError> {
    let did = auth.access.credentials.unwrap().did.unwrap();
    let UpdateEmailInput { email, token } = body.into_inner();
    if !mailchecker::is_valid(&email) {
        return Err(ApiError::InvalidRequest(
            "This email address is not supported, please use a different email.".to_string(),
        ));
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

    let account = account.ok_or(ApiError::InvalidRequest("account not found".to_string()))?;
    // require valid token if account email is confirmed
    if account.email_confirmed_at.is_some() {
        match token {
            Some(token) => {
                account_manager
                    .assert_valid_email_token(&did, EmailTokenPurpose::UpdateEmail, &token)
                    .await?
            }
            None => {
                return Err(ApiError::InvalidRequest(
                    "Confirmation token required".to_string(),
                ))
            }
        }
    }
    account_manager
        .update_email(UpdateEmailOpts { did, email })
        .await?;
    Ok(())
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
    inner_update_email(body, auth, account_manager).await
}
