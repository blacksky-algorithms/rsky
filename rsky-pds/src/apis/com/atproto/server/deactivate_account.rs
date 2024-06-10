use crate::account_manager::AccountManager;
use crate::auth_verifier::AccessFull;
use crate::models::{InternalErrorCode, InternalErrorMessageResponse};
use anyhow::Result;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::DeactivateAccountInput;

#[rocket::post(
    "/xrpc/com.atproto.server.deactivateAccount",
    format = "json",
    data = "<body>"
)]
pub async fn deactivate_account(
    body: Json<DeactivateAccountInput>,
    auth: AccessFull,
) -> Result<(), status::Custom<Json<InternalErrorMessageResponse>>> {
    let did = auth.access.credentials.unwrap().did.unwrap();
    let DeactivateAccountInput { delete_after } = body.into_inner();
    match AccountManager::deactivate_account(&did, delete_after).await {
        Ok(()) => Ok(()),
        Err(error) => {
            eprintln!("Internal Error: {error}");
            let internal_error = InternalErrorMessageResponse {
                code: Some(InternalErrorCode::InternalError),
                message: Some("Internal error".to_string()),
            };
            return Err(status::Custom(
                Status::InternalServerError,
                Json(internal_error),
            ));
        }
    }
}
