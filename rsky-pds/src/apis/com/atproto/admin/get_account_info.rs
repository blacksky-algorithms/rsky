use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::auth_verifier::Moderator;
use crate::common::env::env_str;
use crate::models::{ErrorCode, ErrorMessageResponse};
use crate::INVALID_HANDLE;
use anyhow::{bail, Result};
use futures::try_join;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::admin::AccountView;

async fn inner_get_account_info(did: String) -> Result<AccountView> {
    let (account, invites, invited_by) = try_join!(
        AccountManager::get_account(
            &did,
            Some(AvailabilityFlags {
                include_deactivated: Some(true),
                include_taken_down: Some(true)
            })
        ),
        AccountManager::get_account_invite_codes(&did),
        AccountManager::get_invited_by_for_accounts(vec![&did])
    )?;
    if let Some(account) = account {
        let manages_own_invites = env_str("PDS_ENTRYWAY_URL").is_none();
        Ok(AccountView {
            did: account.did,
            handle: account.handle.unwrap_or(INVALID_HANDLE.to_string()),
            email: account.email,
            indexed_at: account.created_at,
            email_confirmed_at: account.email_confirmed_at,
            invited_by: match invited_by.get(&did) {
                Some(code_detail) if manages_own_invites => Some(code_detail.clone()),
                _ => None,
            },
            invites: if manages_own_invites {
                Some(invites)
            } else {
                None
            },
            invites_disabled: if manages_own_invites {
                Some(account.invites_disabled == Some(1))
            } else {
                None
            },
            related_records: None,
            invite_note: None,
        })
    } else {
        bail!("Account not found")
    }
}

#[rocket::get("/xrpc/com.atproto.admin.getAccountInfo?<did>")]
pub async fn get_account_info(
    did: String,
    _auth: Moderator,
) -> Result<Json<AccountView>, status::Custom<Json<ErrorMessageResponse>>> {
    match inner_get_account_info(did).await {
        Ok(res) => Ok(Json(res)),
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
