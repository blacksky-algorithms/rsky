use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::auth_verifier::AccessFull;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::RevokeAppPasswordInput;

#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.server.revokeAppPassword",
    format = "json",
    data = "<body>"
)]
pub async fn revoke_app_password(
    body: Json<RevokeAppPasswordInput>,
    auth: AccessFull,
    account_manager: AccountManager,
) -> Result<(), ApiError> {
    let RevokeAppPasswordInput { name } = body.into_inner();
    let requester = auth.access.credentials.did.unwrap();

    match account_manager.revoke_app_password(requester, name).await {
        Ok(_) => Ok(()),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
