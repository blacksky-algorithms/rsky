use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::auth_verifier::AccessFull;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::RevokeAppPasswordInput;

#[rocket::post(
    "/xrpc/com.atproto.server.revokeAppPassword",
    format = "json",
    data = "<body>"
)]
pub async fn revoke_app_password(
    body: Json<RevokeAppPasswordInput>,
    auth: AccessFull,
) -> Result<(), ApiError> {
    let RevokeAppPasswordInput { name } = body.into_inner();
    let requester = auth.access.credentials.unwrap().did.unwrap();

    match AccountManager::revoke_app_password(requester, name).await {
        Ok(_) => Ok(()),
        Err(error) => {
            eprintln!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
