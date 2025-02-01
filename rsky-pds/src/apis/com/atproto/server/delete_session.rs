use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::auth_verifier::RevokeRefreshToken;

#[rocket::post("/xrpc/com.atproto.server.deleteSession")]
pub async fn delete_session(auth: RevokeRefreshToken) -> Result<(), ApiError> {
    match AccountManager::revoke_refresh_token(auth.id).await {
        Ok(_) => Ok(()),
        Err(error) => {
            eprintln!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
