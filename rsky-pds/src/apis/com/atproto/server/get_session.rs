use crate::account_manager::AccountManager;
use crate::auth_verifier::AccessStandard;
use crate::models::{ErrorCode, ErrorMessageResponse};
use crate::INVALID_HANDLE;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::GetSessionOutput;

#[rocket::get("/xrpc/com.atproto.server.getSession")]
pub async fn get_session(
    auth: AccessStandard,
) -> Result<Json<GetSessionOutput>, status::Custom<Json<ErrorMessageResponse>>> {
    let did = auth.access.credentials.unwrap().did.unwrap();
    match AccountManager::get_account(&did, None).await {
        Ok(Some(user)) => Ok(Json(GetSessionOutput {
            handle: user.handle.unwrap_or(INVALID_HANDLE.to_string()),
            did: user.did,
            email: user.email,
            did_doc: None,
            email_confirmed: Some(user.email_confirmed_at.is_some()),
        })),
        _ => {
            let internal_error = ErrorMessageResponse {
                code: Some(ErrorCode::InternalServerError),
                message: Some(format!("Could not find user info for account: `{did:?}`")),
            };
            return Err(status::Custom(
                Status::InternalServerError,
                Json(internal_error),
            ));
        }
    }
}
