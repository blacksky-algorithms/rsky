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
    account_manager: AccountManager,
) -> Result<Json<Operation>, ApiError> {
    let did = auth.access.credentials.did.unwrap();
    let request = body.into_inner();
    let token = request.token.clone();

    if request.token.is_empty() {
        return Err(ApiError::InvalidRequest(
            "email confirmation token required to sign PLC operations".to_string(),
        ));
    }
    account_manager
        .assert_valid_email_token_and_cleanup(&did, EmailTokenPurpose::PlcOperation, &token)
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

    //If request doesn't contain field, check last op for field. In the case of CreateOpV1,
    // we don't set it (which is aligned with BSky Implementation
    let also_known_as = match request.also_known_as {
        None => match last_op {
            CompatibleOp::CreateOpV1(_) => None,
            CompatibleOp::Operation(ref op) => Some(op.also_known_as.clone()),
        },
        Some(res) => Some(res),
    };
    let services = match request.services {
        None => match last_op {
            CompatibleOp::CreateOpV1(_) => None,
            CompatibleOp::Operation(ref op) => Some(op.services.clone()),
        },
        Some(res) => match serde_json::from_value::<BTreeMap<String, Service>>(res) {
            Ok(services) if !services.is_empty() => Some(services),
            _ => match last_op {
                CompatibleOp::CreateOpV1(_) => None,
                CompatibleOp::Operation(ref op) => Some(op.services.clone()),
            },
        },
    };
    let verification_methods = match request.verification_methods {
        None => match last_op {
            CompatibleOp::CreateOpV1(_) => None,
            CompatibleOp::Operation(ref op) => Some(op.verification_methods.clone()),
        },
        Some(res) => Some(res),
    };
    let rotation_keys = match request.rotation_keys {
        None => match last_op {
            CompatibleOp::CreateOpV1(_) => None,
            CompatibleOp::Operation(ref op) => Some(op.rotation_keys.clone()),
        },
        Some(res) => Some(res),
    };
    let operation = match create_update_op(
        last_op,
        &secret_rotation_key,
        |normalized: Operation| -> Operation {
            let mut updated = normalized.clone();
            if let Some(also_known_as) = &also_known_as {
                updated.also_known_as = also_known_as.clone();
            }
            if let Some(services) = &services {
                updated.services = services.clone();
            }
            if let Some(verification_methods) = &verification_methods {
                updated.verification_methods = verification_methods.clone();
            }
            if let Some(rotation_keys) = &rotation_keys {
                updated.rotation_keys = rotation_keys.clone();
            }
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
