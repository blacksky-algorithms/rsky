use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::auth_verifier::Moderator;
use anyhow::Result;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::admin::EnableAccountInvitesInput;

#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.admin.enableAccountInvites",
    format = "json",
    data = "<body>"
)]
pub async fn enable_account_invites(
    body: Json<EnableAccountInvitesInput>,
    _auth: Moderator,
    account_manager: AccountManager,
) -> Result<(), ApiError> {
    let EnableAccountInvitesInput { account, .. } = body.into_inner();
    match account_manager
        .set_account_invites_disabled(&account, false)
        .await
    {
        Ok(_) => Ok(()),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
