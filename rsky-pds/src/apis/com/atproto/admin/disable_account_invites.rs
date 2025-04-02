use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::auth_verifier::Moderator;
use anyhow::Result;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::admin::DisableAccountInvitesInput;

#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.admin.disableAccountInvites",
    format = "json",
    data = "<body>"
)]
pub async fn disable_account_invites(
    body: Json<DisableAccountInvitesInput>,
    _auth: Moderator,
    account_manager: AccountManager,
) -> Result<(), ApiError> {
    let DisableAccountInvitesInput { account, .. } = body.into_inner();
    match account_manager
        .set_account_invites_disabled(&account, true)
        .await
    {
        Ok(_) => Ok(()),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
