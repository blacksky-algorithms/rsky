use crate::account_manager::AccountManager;
use crate::apis::{allows_email_read, ApiError};
use crate::auth_verifier::scope::{NoScopeRequired, Scoped};
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::GetSessionOutput;
use rsky_syntax::handle::INVALID_HANDLE;

#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.server.getSession")]
pub async fn get_session(
    auth: Scoped<NoScopeRequired>,
    account_manager: AccountManager,
) -> Result<Json<GetSessionOutput>, ApiError> {
    let email_visible = allows_email_read(auth.credentials().await?);
    let did = auth.did().await?;
    match account_manager.get_account(&did, None).await {
        Ok(Some(user)) => Ok(Json(GetSessionOutput {
            handle: user.handle.unwrap_or(INVALID_HANDLE.to_string()),
            did: user.did,
            email: email_visible.then_some(user.email).flatten(),
            did_doc: None,
            email_confirmed: email_visible.then_some(user.email_confirmed_at.is_some()),
        })),
        _ => Err(ApiError::AccountNotFound),
    }
}
