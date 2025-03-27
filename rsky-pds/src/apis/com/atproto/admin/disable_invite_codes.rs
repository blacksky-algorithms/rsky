use crate::account_manager::{AccountManager, DisableInviteCodesOpts};
use crate::apis::ApiError;
use crate::auth_verifier::Moderator;
use anyhow::{bail, Result};
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::admin::DisableInviteCodesInput;

async fn inner_disable_invite_codes(
    body: Json<DisableInviteCodesInput>,
    account_manager: AccountManager,
) -> Result<()> {
    let DisableInviteCodesInput { codes, accounts } = body.into_inner();
    let codes: Vec<String> = codes.unwrap_or_else(Vec::new);
    let accounts: Vec<String> = accounts.unwrap_or_else(Vec::new);

    if accounts.contains(&"admin".to_string()) {
        bail!("cannot disable admin invite codes")
    }

    account_manager
        .disable_invite_codes(DisableInviteCodesOpts { codes, accounts })
        .await
}

#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.admin.disableInviteCodes",
    format = "json",
    data = "<body>"
)]
pub async fn disable_invite_codes(
    body: Json<DisableInviteCodesInput>,
    _auth: Moderator,
    account_manager: AccountManager,
) -> Result<(), ApiError> {
    match inner_disable_invite_codes(body, account_manager).await {
        Ok(_) => Ok(()),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
