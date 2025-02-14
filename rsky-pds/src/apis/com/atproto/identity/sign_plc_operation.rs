use crate::account_manager::AccountManager;
use crate::apis::com::atproto::server::get_keys_from_private_key_str;
use crate::apis::ApiError;
use crate::auth_verifier::AccessFull;
use crate::models::models::EmailTokenPurpose;
use crate::plc;
use crate::plc::operations::create_update_op;
use crate::plc::types::{CompatibleOp, CompatibleOpOrTombstone, Operation, Service};
use rocket::serde::json::Json;
use rsky_common::env::env_str;
use rsky_lexicon::com::atproto::identity::SignPlcOperationRequest;
use std::collections::BTreeMap;

#[rocket::post(
    "/xrpc/com.atproto.identity.signPlcOperation",
    format = "json",
    data = "<body>"
)]
#[tracing::instrument(skip_all)]
pub async fn sign_plc_operation(
    body: Json<SignPlcOperationRequest>,
    auth: AccessFull,
) -> Result<Json<Operation>, ApiError> {
    let did = auth.access.credentials.unwrap().did.unwrap();
    let request = body.into_inner();
    let token = request.token.clone();

    if request.token.is_empty() {
        return Err(ApiError::InvalidRequest(
            "email confirmation token required to sign PLC operations".to_string(),
        ));
    }
    AccountManager::assert_valid_email_token_and_cleanup(
        &did,
        EmailTokenPurpose::PlcOperation,
        &token,
    )
    .await?;

    let plc_url = env_str("PDS_DID_PLC_URL").unwrap_or("https://plc.directory".to_owned());
    let plc_client = plc::Client::new(plc_url);
    let last_op: CompatibleOp = match plc_client.get_last_op(&did).await {
        Ok(res) => match res {
            CompatibleOpOrTombstone::CreateOpV1(op) => CompatibleOp::CreateOpV1(op),
            CompatibleOpOrTombstone::Operation(op) => CompatibleOp::Operation(op),
            CompatibleOpOrTombstone::Tombstone(_) => {
                return Err(ApiError::InvalidRequest("Did is tombstoned".to_string()))
            }
        },
        Err(error) => {
            tracing::error!("Error getting last PLC operation\n{error}");
            return Err(ApiError::RuntimeError);
        }
    };

    let private_key = env_str("PDS_PLC_ROTATION_KEY_K256_PRIVATE_KEY_HEX").unwrap();
    let (secret_rotation_key, _) = get_keys_from_private_key_str(private_key)?;

    let operation = match create_update_op(
        last_op,
        &secret_rotation_key,
        |normalized: Operation| -> Operation {
            let mut updated = normalized.clone();
            updated.also_known_as = request.also_known_as.clone();
            updated.services = request.services.clone();
            updated.verification_methods = request.verification_methods.clone();
            updated.rotation_keys = request.rotation_keys.clone();
            updated
        },
    )
    .await
    {
        Ok(res) => res,
        Err(error) => {
            tracing::error!("Error creating signed operation\n{error}");
            return Err(ApiError::RuntimeError);
        }
    };

    Ok(Json(operation))
}
