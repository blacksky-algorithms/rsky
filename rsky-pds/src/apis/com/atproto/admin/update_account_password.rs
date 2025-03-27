use crate::account_manager::{AccountManager, UpdateAccountPasswordOpts};
use crate::apis::ApiError;
use crate::auth_verifier::AdminToken;
use anyhow::Result;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::admin::UpdateAccountPasswordInput;

#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.admin.updateAccountPassword",
    format = "json",
    data = "<body>"
)]
pub async fn update_account_password(
    body: Json<UpdateAccountPasswordInput>,
    _auth: AdminToken,
    account_manager: AccountManager,
) -> Result<(), ApiError> {
    let UpdateAccountPasswordInput { did, password } = body.into_inner();
    match account_manager
        .update_account_password(UpdateAccountPasswordOpts { did, password })
        .await
    {
        Ok(_) => Ok(()),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
