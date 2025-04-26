use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::auth_verifier::AccessStandard;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::GetSessionOutput;
use rsky_syntax::handle::INVALID_HANDLE;

#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.server.getSession")]
pub async fn get_session(
    auth: AccessStandard,
    account_manager: AccountManager,
) -> Result<Json<GetSessionOutput>, ApiError> {
    let did = auth.access.credentials.did.unwrap();
    match account_manager.get_account(&did, None).await {
        Ok(Some(user)) => Ok(Json(GetSessionOutput {
            handle: user.handle.unwrap_or(INVALID_HANDLE.to_string()),
            did: user.did,
            email: user.email,
            did_doc: None,
            email_confirmed: Some(user.email_confirmed_at.is_some()),
        })),
        _ => Err(ApiError::AccountNotFound),
    }
}
