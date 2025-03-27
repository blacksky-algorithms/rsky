use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::auth_verifier::RevokeRefreshToken;

#[tracing::instrument(skip_all)]
#[rocket::post("/xrpc/com.atproto.server.deleteSession")]
pub async fn delete_session(
    auth: RevokeRefreshToken,
    account_manager: AccountManager,
) -> Result<(), ApiError> {
    match account_manager.revoke_refresh_token(auth.id).await {
        Ok(_) => Ok(()),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
