use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::auth_verifier::Moderator;
use anyhow::Result;
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
) -> Result<(), ApiError> {
    let EnableAccountInvitesInput { account, .. } = body.into_inner();
    match AccountManager::set_account_invites_disabled(&account, false).await {
        Ok(_) => Ok(()),
        Err(error) => {
            eprintln!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
