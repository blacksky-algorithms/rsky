use crate::account_manager::{AccountManager, UpdateAccountPasswordOpts};
use crate::apis::ApiError;
use crate::auth_verifier::AdminToken;
use anyhow::Result;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::admin::UpdateAccountPasswordInput;

#[rocket::post(
    "/xrpc/com.atproto.admin.updateAccountPassword",
    format = "json",
    data = "<body>"
)]
pub async fn update_account_password(
    body: Json<UpdateAccountPasswordInput>,
    _auth: AdminToken,
) -> Result<(), ApiError> {
    let UpdateAccountPasswordInput { did, password } = body.into_inner();
    match AccountManager::update_account_password(UpdateAccountPasswordOpts { did, password }).await
    {
        Ok(_) => Ok(()),
        Err(error) => {
            eprintln!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
