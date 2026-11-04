use crate::account_manager::helpers::account::{
    format_account_status, AvailabilityFlags, FormattedAccountStatus,
};
use crate::account_manager::AccountManager;
use crate::apis::com::atproto::server::did_doc_for_session;
use crate::apis::{allows_email_read, ApiError};
use crate::auth_verifier::scope::{NoScopeRequired, Scoped};
use crate::config::ServerConfig;
use crate::SharedIdResolver;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::server::GetSessionOutput;
use rsky_syntax::handle::INVALID_HANDLE;

#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.server.getSession")]
pub async fn get_session(
    auth: Scoped<NoScopeRequired>,
    cfg: &State<ServerConfig>,
    id_resolver: &State<SharedIdResolver>,
    account_manager: AccountManager,
) -> Result<Json<GetSessionOutput>, ApiError> {
    let show_email = allows_email_read(auth.credentials().await?);
    let did = auth.did().await?;
    let flags = Some(AvailabilityFlags {
        include_deactivated: Some(true),
        include_taken_down: Some(false),
    });
    match account_manager.get_account(&did, flags).await {
        Ok(Some(user)) => {
            let FormattedAccountStatus { active, status } =
                format_account_status(Some(user.clone()));
            let did_doc = did_doc_for_session(
                cfg.identity.enable_did_doc_with_session,
                id_resolver,
                &user.did,
            )
            .await;
            Ok(Json(GetSessionOutput {
                handle: user.handle.unwrap_or(INVALID_HANDLE.to_string()),
                did: user.did,
                email: user.email.filter(|_| show_email),
                did_doc,
                email_confirmed: show_email.then_some(user.email_confirmed_at.is_some()),
                active: Some(active),
                status: status.map(|status| status.as_str().to_owned()),
            }))
        }
        Ok(None) => Err(ApiError::InvalidRequest(format!(
            "Could not find user info for account: {did}"
        ))),
        Err(error) => {
            tracing::error!("{error:?}");
            Err(ApiError::RuntimeError)
        }
    }
}
