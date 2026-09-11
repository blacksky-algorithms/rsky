use crate::account_manager::helpers::account::{
    format_account_status, AvailabilityFlags, FormattedAccountStatus,
};
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
    let did = auth.access.credentials.unwrap().did.unwrap();
    let flags = Some(AvailabilityFlags {
        include_deactivated: Some(true),
        include_taken_down: Some(false),
    });
    match account_manager.get_account(&did, flags).await {
        Ok(Some(user)) => {
            let FormattedAccountStatus { active, status } =
                format_account_status(Some(user.clone()));
            Ok(Json(GetSessionOutput {
                handle: user.handle.unwrap_or(INVALID_HANDLE.to_string()),
                did: user.did,
                email: user.email,
                did_doc: None,
                email_confirmed: Some(user.email_confirmed_at.is_some()),
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
