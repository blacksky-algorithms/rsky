use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::auth_verifier::AccessFull;
use crate::mailer::{send_plc_operation, TokenParam};
use crate::models::models::EmailTokenPurpose;

#[rocket::post("/xrpc/com.atproto.identity.requestPlcOperationSignature")]
#[tracing::instrument(skip_all)]
pub async fn request_plc_operation_signature(auth: AccessFull) -> Result<(), ApiError> {
    let requester = auth.access.credentials.unwrap().did.unwrap();
    let availability_flags = AvailabilityFlags {
        include_taken_down: Some(true),
        include_deactivated: Some(true),
    };
    let account = AccountManager::get_account(&requester, Some(availability_flags))
        .await?
        .expect("Account not found despite valid access");

    let token =
        match AccountManager::create_email_token(&requester, EmailTokenPurpose::PlcOperation).await
        {
            Ok(res) => res,
            Err(error) => {
                tracing::error!("Failed to create plc operation token\n{error}");
                return Err(ApiError::RuntimeError);
            }
        };

    match send_plc_operation(account.email.unwrap(), TokenParam { token }).await {
        Ok(_) => {
            tracing::info!("Successfully sent PLC Operation Email");
        }
        Err(error) => {
            tracing::error!("Failed to send PLC Operation Token Email\n{error}");
            return Err(ApiError::RuntimeError);
        }
    }

    Ok(())
}
