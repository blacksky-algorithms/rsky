use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::auth_verifier::{Credentials, Refresh};
use crate::models::{ErrorCode, ErrorMessageResponse};
use crate::INVALID_HANDLE;
use anyhow::{bail, Result};
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::RefreshSessionOutput;

async fn inner_refresh_session(auth: Refresh) -> Result<RefreshSessionOutput> {
    let Credentials { did, token_id, .. } = auth.access.credentials.unwrap();
    let did = did.unwrap();
    let token_id = token_id.unwrap();
    let user = AccountManager::get_account(
        &did,
        Some(AvailabilityFlags {
            include_deactivated: Some(true),
            include_taken_down: Some(true),
        }),
    )
    .await?;

    if let Some(user) = user {
        if user.takedown_ref.is_some() {
            bail!("Account has been taken down")
        }
        let rotated = AccountManager::rotate_refresh_token(&token_id).await?;
        if let Some(rotated) = rotated {
            Ok(RefreshSessionOutput {
                handle: user.handle.unwrap_or(INVALID_HANDLE.to_string()),
                did,
                did_doc: None,
                access_jwt: rotated.0,
                refresh_jwt: rotated.1,
            })
        } else {
            bail!("Token has been revoked")
        }
    } else {
        bail!("Could not find user info for account: `{did}`")
    }
}

#[rocket::post("/xrpc/com.atproto.server.refreshSession")]
pub async fn refresh_session(
    auth: Refresh,
) -> Result<Json<RefreshSessionOutput>, status::Custom<Json<ErrorMessageResponse>>> {
    match inner_refresh_session(auth).await {
        Ok(res) => Ok(Json(res)),
        Err(error) => {
            eprintln!("Internal Error: {error}");
            let internal_error = ErrorMessageResponse {
                code: Some(ErrorCode::InternalServerError),
                message: Some("Internal error".to_string()),
            };
            return Err(status::Custom(
                Status::InternalServerError,
                Json(internal_error),
            ));
        }
    }
}
