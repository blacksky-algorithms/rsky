use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::{AccountManager, ConfirmEmailOpts};
use crate::apis::ApiError;
use crate::auth_verifier::AccessStandardIncludeChecks;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::ConfirmEmailInput;

#[tracing::instrument(skip_all)]
async fn inner_confirm_email(
    body: Json<ConfirmEmailInput>,
    auth: AccessStandardIncludeChecks,
) -> Result<(), ApiError> {
    let did = auth.access.credentials.unwrap().did.unwrap();

    let user;
    match AccountManager::get_account(
        &did,
        Some(AvailabilityFlags {
            include_deactivated: Some(true),
            include_taken_down: None,
        }),
    )
    .await
    {
        Ok(res) => {
            user = res;
        }
        Err(e) => {
            tracing::error!("Error: {e}");
            return Err(ApiError::RuntimeError);
        }
    }
    if let Some(user) = user {
        if let Some(user_email) = user.email {
            let ConfirmEmailInput { token, email } = body.into_inner();
            if user_email != email.to_lowercase() {
                return Err(ApiError::InvalidEmail);
            }
            match AccountManager::confirm_email(ConfirmEmailOpts {
                did: &did,
                token: &token,
            })
            .await
            {
                Ok(_) => {}
                Err(e) => {
                    tracing::error!("Error: {e}");
                    return Err(ApiError::RuntimeError);
                }
            }
            Ok(())
        } else {
            Err(ApiError::InvalidRequest("Missing Email".to_string()))
        }
    } else {
        Err(ApiError::AccountNotFound)
    }
}

#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.server.confirmEmail",
    format = "json",
    data = "<body>"
)]
pub async fn confirm_email(
    body: Json<ConfirmEmailInput>,
    auth: AccessStandardIncludeChecks,
) -> Result<(), ApiError> {
    match inner_confirm_email(body, auth).await {
        Ok(()) => Ok(()),
        Err(error) => Err(error),
    }
}
