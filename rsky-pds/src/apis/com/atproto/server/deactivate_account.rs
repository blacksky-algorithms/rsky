use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::auth_verifier::scope::{AccountStatus, Scoped};
use crate::auth_verifier::AccessFull;
use anyhow::Result;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::DeactivateAccountInput;

#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.server.deactivateAccount",
    format = "json",
    data = "<body>"
)]
pub async fn deactivate_account(
    body: Json<DeactivateAccountInput>,
    auth: Scoped<AccountStatus, AccessFull>,
    account_manager: AccountManager,
) -> Result<(), ApiError> {
    let did = auth.did().await?;
    let DeactivateAccountInput { delete_after } = body.into_inner();
    match account_manager.deactivate_account(&did, delete_after).await {
        Ok(()) => Ok(()),
        Err(error) => {
            tracing::error!("Internal Error: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
