use crate::account_manager::AccountManager;
use crate::auth_verifier::Access;
use crate::models::{InternalErrorCode, InternalErrorMessageResponse};
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::RevokeAppPasswordInput;

#[rocket::post(
    "/xrpc/com.atproto.server.revokeAppPassword",
    format = "json",
    data = "<body>"
)]
pub async fn revoke_app_password(
    body: Json<RevokeAppPasswordInput>,
    auth: Access,
) -> Result<(), status::Custom<Json<InternalErrorMessageResponse>>> {
    let RevokeAppPasswordInput { name } = body.into_inner();
    let requester = auth.access.credentials.unwrap().did.unwrap();

    match AccountManager::revoke_app_password(requester, name).await {
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
