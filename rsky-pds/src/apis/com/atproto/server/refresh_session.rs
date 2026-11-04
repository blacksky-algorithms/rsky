use crate::account_manager::helpers::account::{
    format_account_status, AvailabilityFlags, FormattedAccountStatus,
};
use crate::account_manager::AccountManager;
use crate::apis::com::atproto::server::did_doc_for_session;
use crate::apis::ApiError;
use crate::auth_verifier::{Credentials, Refresh};
use crate::config::ServerConfig;
use crate::SharedIdResolver;
use anyhow::Result;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::server::RefreshSessionOutput;
use rsky_syntax::handle::INVALID_HANDLE;

async fn inner_refresh_session(
    auth: Refresh,
    cfg: &ServerConfig,
    id_resolver: &SharedIdResolver,
    account_manager: AccountManager,
) -> Result<RefreshSessionOutput, ApiError> {
    let Credentials { did, token_id, .. } = auth.access.credentials.unwrap();
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
    let Some(user) = user else {
        return Err(ApiError::InvalidRequest(format!(
            "Could not find user info for account: {did}"
        )));
    };
    if user.takedown_ref.is_some() {
        return Err(ApiError::AccountTakendown);
    }
    let Some((access_jwt, refresh_jwt)) = account_manager.rotate_refresh_token(&token_id).await?
    else {
        return Err(ApiError::RefreshTokenRevoked);
    };
    let FormattedAccountStatus { active, status } = format_account_status(Some(user.clone()));
    let did_doc =
        did_doc_for_session(cfg.identity.enable_did_doc_with_session, id_resolver, &did).await;
    Ok(RefreshSessionOutput {
        handle: user.handle.unwrap_or(INVALID_HANDLE.to_string()),
        did,
        did_doc,
        access_jwt,
        refresh_jwt,
        email: user.email,
        email_confirmed: Some(user.email_confirmed_at.is_some()),
        active: Some(active),
        status: status.map(|status| status.as_str().to_owned()),
    })
}

#[tracing::instrument(skip_all)]
#[rocket::post("/xrpc/com.atproto.server.refreshSession")]
pub async fn refresh_session(
    auth: Refresh,
    cfg: &State<ServerConfig>,
    id_resolver: &State<SharedIdResolver>,
    account_manager: AccountManager,
) -> Result<Json<RefreshSessionOutput>, ApiError> {
    match inner_refresh_session(auth, cfg, id_resolver, account_manager).await {
        Ok(res) => Ok(Json(res)),
        Err(error) => Err(error),
    }
}
