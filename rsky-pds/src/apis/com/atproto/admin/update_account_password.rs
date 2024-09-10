use crate::account_manager::{AccountManager, UpdateAccountPasswordOpts};
use crate::auth_verifier::AdminToken;
use crate::models::{ErrorCode, ErrorMessageResponse};
use anyhow::Result;
use rocket::http::Status;
use rocket::response::status;
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
) -> Result<(), status::Custom<Json<ErrorMessageResponse>>> {
    let UpdateAccountPasswordInput { did, password } = body.into_inner();
    match AccountManager::update_account_password(UpdateAccountPasswordOpts { did, password }).await
    {
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
