use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use crate::account_manager::AccountManager;
use crate::auth_verifier::RevokeRefreshToken;
use crate::models::{InternalErrorCode, InternalErrorMessageResponse};

#[rocket::post("/xrpc/com.atproto.server.deleteSession")]
pub async fn delete_session(
    auth: RevokeRefreshToken
) -> Result<(), status::Custom<Json<InternalErrorMessageResponse>>> {
    match AccountManager::revoke_refresh_token(auth.id).await {
        Ok(_) => Ok(()),
        Err(error) => {
            let internal_error = InternalErrorMessageResponse {
                code: Some(InternalErrorCode::InternalError),
                message: Some(error.to_string()),
            };
            return Err(status::Custom(
                Status::InternalServerError,
                Json(internal_error),
            ));
        }
    }
}
