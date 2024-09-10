use crate::account_manager::AccountManager;
use crate::auth_verifier::Moderator;
use crate::models::{ErrorCode, ErrorMessageResponse};
use anyhow::Result;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::admin::EnableAccountInvitesInput;

#[rocket::post(
    "/xrpc/com.atproto.admin.enableAccountInvites",
    format = "json",
    data = "<body>"
)]
pub async fn enable_account_invites(
    body: Json<EnableAccountInvitesInput>,
    _auth: Moderator,
) -> Result<(), status::Custom<Json<ErrorMessageResponse>>> {
    let EnableAccountInvitesInput { account, .. } = body.into_inner();
    match AccountManager::set_account_invites_disabled(&account, false).await {
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
