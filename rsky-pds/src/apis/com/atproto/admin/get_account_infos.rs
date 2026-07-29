use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::apis::com::atproto::admin::get_account_info::{
    format_account_view, manages_own_invites,
};
use crate::apis::ApiError;
use crate::auth_verifier::Moderator;
use anyhow::Result;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::admin::{AccountView, GetAccountInfosOutput};

async fn inner_get_account_infos(
    dids: Vec<String>,
    account_manager: AccountManager,
) -> Result<GetAccountInfosOutput> {
    let invited_by = account_manager
        .get_invited_by_for_accounts(dids.clone())
        .await?;
    let manages_own_invites = manages_own_invites();
    let mut infos: Vec<AccountView> = Vec::with_capacity(dids.len());
    for did in dids {
        let account = account_manager
            .get_account(
                &did,
                Some(AvailabilityFlags {
                    include_deactivated: Some(true),
                    include_taken_down: Some(true),
                }),
            )
            .await?;
        // accounts which do not exist on this server are skipped
        let Some(account) = account else { continue };
        let invites = account_manager.get_account_invite_codes(&did).await?;
        infos.push(format_account_view(
            account,
            invites,
            &invited_by,
            manages_own_invites,
        ));
    }
    Ok(GetAccountInfosOutput { infos })
}

/// Get details about some accounts.
#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.admin.getAccountInfos?<dids>")]
pub async fn get_account_infos(
    dids: Vec<String>,
    _auth: Moderator,
    account_manager: AccountManager,
) -> Result<Json<GetAccountInfosOutput>, ApiError> {
    match inner_get_account_infos(dids, account_manager).await {
        Ok(res) => Ok(Json(res)),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
