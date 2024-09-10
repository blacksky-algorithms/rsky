use crate::account_manager::{AccountManager, ResetPasswordOpts};
use crate::models::{ErrorCode, ErrorMessageResponse};
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::ResetPasswordInput;

#[rocket::post(
    "/xrpc/com.atproto.server.resetPassword",
    format = "json",
    data = "<body>"
)]
pub async fn reset_password(
    body: Json<ResetPasswordInput>,
) -> Result<(), status::Custom<Json<ErrorMessageResponse>>> {
    let ResetPasswordInput { token, password } = body.into_inner();
    match AccountManager::reset_password(ResetPasswordOpts { token, password }).await {
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
