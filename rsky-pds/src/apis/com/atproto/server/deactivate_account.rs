use crate::account_manager::helpers::account::{AccountStatus, AvailabilityFlags};
use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::auth_verifier::scope::{OAuthForbidden, Scoped};
use crate::auth_verifier::AccessFullAllowTakendown;
use crate::SharedSequencer;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::server::DeactivateAccountInput;

/// Deactivates the account and publishes the new status. With
/// `delete_credentials` every OAuth session, authorized client and app
/// password goes too, as the reference does when the account pages
/// deactivate; the XRPC method keeps them.
pub async fn deactivate_account_for(
    did: &str,
    delete_after: Option<String>,
    delete_credentials: bool,
    sequencer: &SharedSequencer,
    account_manager: &AccountManager,
) -> Result<(), ApiError> {
    let account = account_manager
        .get_account(
            did,
            Some(AvailabilityFlags {
                include_deactivated: Some(true),
                include_taken_down: Some(true),
            }),
        )
        .await?;
    if account.is_none() {
        return Err(ApiError::InvalidRequest("Account not found".to_string()));
    }
    if delete_credentials {
        account_manager
            .deactivate_account_and_credentials(did, delete_after)
            .await?;
    } else {
        account_manager
            .deactivate_account(did, delete_after)
            .await?;
    }
    let status = account_manager.get_account_status(did).await?;
    if status == AccountStatus::Deleted {
        return Err(ApiError::InvalidRequest("Account not found".to_string()));
    }
    let mut lock = sequencer.sequencer.write().await;
    lock.sequence_account_evt(did.to_string(), status).await?;
    Ok(())
}

/// Deactivates the caller's account and publishes the new status, the way
/// the reference PDS does. A taken-down account may deactivate itself with
/// its recovery session.
#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.server.deactivateAccount",
    format = "json",
    data = "<body>"
)]
pub async fn deactivate_account(
    body: Json<DeactivateAccountInput>,
    auth: Scoped<OAuthForbidden, AccessFullAllowTakendown>,
    sequencer: &State<SharedSequencer>,
    account_manager: AccountManager,
) -> Result<(), ApiError> {
    let did = auth.did().await?;
    let DeactivateAccountInput { delete_after } = body.into_inner();
    deactivate_account_for(&did, delete_after, false, sequencer, &account_manager).await
}
