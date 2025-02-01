use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::auth_verifier::AccessFull;
use anyhow::Result;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::DeactivateAccountInput;

#[rocket::post(
    "/xrpc/com.atproto.server.deactivateAccount",
    format = "json",
    data = "<body>"
)]
pub async fn deactivate_account(
    body: Json<DeactivateAccountInput>,
    auth: AccessFull,
) -> Result<(), ApiError> {
    let did = auth.access.credentials.unwrap().did.unwrap();
    let DeactivateAccountInput { delete_after } = body.into_inner();
    match AccountManager::deactivate_account(&did, delete_after).await {
        Ok(()) => Ok(()),
        Err(error) => {
            eprintln!("Internal Error: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
