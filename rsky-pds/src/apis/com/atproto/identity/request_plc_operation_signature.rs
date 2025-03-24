use crate::account_manager::helpers::account::{ActorAccount, AvailabilityFlags};
use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::auth_verifier::AccessFull;
use crate::mailer::{send_plc_operation, TokenParam};
use crate::models::models::EmailTokenPurpose;

#[tracing::instrument(skip_all)]
async fn get_requester_did(auth: &AccessFull) -> Result<String, ApiError> {
    match &auth.access.credentials {
        None => {
            tracing::error!("Failed to find access credentials");
            Err(ApiError::RuntimeError)
        }
        Some(res) => match &res.did {
            None => {
                tracing::error!("Failed to find did");
                Err(ApiError::RuntimeError)
            }
            Some(did) => Ok(did.clone()),
        },
    }
}

#[tracing::instrument(skip_all)]
async fn get_account(requester_did: &str) -> Result<ActorAccount, ApiError> {
    let availability_flags = AvailabilityFlags {
        include_taken_down: Some(true),
        include_deactivated: Some(true),
    };
    match AccountManager::get_account_legacy(&requester_did.to_string(), Some(availability_flags))
        .await
    {
        Ok(account) => match account {
            None => {
                tracing::error!("Account not found despite valid credentials");
                Err(ApiError::RuntimeError)
            }
            Some(account) => Ok(account),
        },
        Err(error) => {
            tracing::error!("Error getting account\n{error}");
            Err(ApiError::RuntimeError)
        }
    }
}

#[tracing::instrument(skip_all)]
async fn create_email_token(requester: &str) -> Result<String, ApiError> {
    match AccountManager::create_email_token(
        &requester.to_string(),
        EmailTokenPurpose::PlcOperation,
    )
    .await
    {
        Ok(res) => Ok(res),
        Err(error) => {
            tracing::error!("Failed to create plc operation token\n{error}");
            Err(ApiError::RuntimeError)
        }
    }
}

#[tracing::instrument(skip_all)]
async fn do_plc_operation(account: &ActorAccount, token: String) -> Result<(), ApiError> {
    match &account.email {
        None => {
            tracing::error!("Failed to find email for account");
            Err(ApiError::RuntimeError)
        }
        Some(email) => match send_plc_operation(email.clone(), TokenParam { token }).await {
            Ok(_) => {
                tracing::debug!("Successfully sent PLC Operation Email");
                Ok(())
            }
            Err(error) => {
                tracing::error!("Failed to send PLC Operation Token Email\n{error}");
                Err(ApiError::RuntimeError)
            }
        },
    }
}

#[rocket::post("/xrpc/com.atproto.identity.requestPlcOperationSignature")]
#[tracing::instrument(skip_all)]
pub async fn request_plc_operation_signature(auth: AccessFull) -> Result<(), ApiError> {
    let requester = get_requester_did(&auth).await?;
    let account = get_account(requester.as_str()).await?;
    let token = create_email_token(requester.as_str()).await?;
    do_plc_operation(&account, token).await?;

    Ok(())
}
