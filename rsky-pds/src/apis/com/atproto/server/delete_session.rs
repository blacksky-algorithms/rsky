use crate::account_manager::AccountManager;
use crate::auth_verifier::RevokeRefreshToken;
use crate::models::{ErrorCode, ErrorMessageResponse};
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;

#[rocket::post("/xrpc/com.atproto.server.deleteSession")]
pub async fn delete_session(
    auth: RevokeRefreshToken,
) -> Result<(), status::Custom<Json<ErrorMessageResponse>>> {
    match AccountManager::revoke_refresh_token(auth.id).await {
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
