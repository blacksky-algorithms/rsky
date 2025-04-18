use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::auth_verifier::{Credentials, Refresh};
use anyhow::Result;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::RefreshSessionOutput;
use rsky_syntax::handle::INVALID_HANDLE;

async fn inner_refresh_session(
    auth: Refresh,
    account_manager: AccountManager,
) -> Result<RefreshSessionOutput, ApiError> {
    let Credentials { did, token_id, .. } = auth.access.credentials;
    let did = did.unwrap();
    let token_id = token_id.unwrap();
    let user = account_manager
        .get_account(
            &did,
            Some(AvailabilityFlags {
                include_deactivated: Some(true),
                include_taken_down: Some(true),
            }),
        )
        .await?;

    if let Some(user) = user {
        if user.takedown_ref.is_some() {
            return Err(ApiError::AccountTakendown);
        }
        let rotated = account_manager.rotate_refresh_token(&token_id).await?;
        if let Some(rotated) = rotated {
            Ok(RefreshSessionOutput {
                handle: user.handle.unwrap_or(INVALID_HANDLE.to_string()),
                did,
                did_doc: None,
                access_jwt: rotated.0,
                refresh_jwt: rotated.1,
            })
        } else {
            Err(ApiError::ExpiredToken)
        }
    } else {
        Err(ApiError::AccountNotFound)
    }
}

#[tracing::instrument(skip_all)]
#[rocket::post("/xrpc/com.atproto.server.refreshSession")]
pub async fn refresh_session(
    auth: Refresh,
    account_manager: AccountManager,
) -> Result<Json<RefreshSessionOutput>, ApiError> {
    match inner_refresh_session(auth, account_manager).await {
        Ok(res) => Ok(Json(res)),
        Err(error) => Err(error),
    }
}
