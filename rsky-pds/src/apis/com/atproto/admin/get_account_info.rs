use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::auth_verifier::Moderator;
use anyhow::{bail, Result};
use futures::try_join;
use rocket::serde::json::Json;
use rsky_common::env::env_str;
use rsky_lexicon::com::atproto::admin::AccountView;
use rsky_syntax::handle::INVALID_HANDLE;

async fn inner_get_account_info(
    did: String,
    account_manager: AccountManager,
) -> Result<AccountView> {
    let (account, invites, invited_by) = try_join!(
        account_manager.get_account(
            &did,
            Some(AvailabilityFlags {
                include_deactivated: Some(true),
                include_taken_down: Some(true)
            })
        ),
        account_manager.get_account_invite_codes(&did),
        account_manager.get_invited_by_for_accounts(vec![did.clone()])
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

#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.admin.getAccountInfo?<did>")]
pub async fn get_account_info(
    did: String,
    _auth: Moderator,
    account_manager: AccountManager,
) -> Result<Json<AccountView>, ApiError> {
    match inner_get_account_info(did, account_manager).await {
        Ok(res) => Ok(Json(res)),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
