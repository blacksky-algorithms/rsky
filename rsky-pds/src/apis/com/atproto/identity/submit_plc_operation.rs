use crate::account_manager::helpers::account::{ActorAccount, AvailabilityFlags};
use crate::account_manager::AccountManager;
use crate::apis::com::atproto::server::get_keys_from_private_key_str;
use crate::apis::ApiError;
use crate::auth_verifier::AccessStandard;
use crate::{plc, SharedIdResolver, SharedSequencer};
use crate::plc::types::{OpOrTombstone, Operation, Service};
use rocket::serde::json::Json;
use rsky_common::env::env_str;
use rsky_crypto::utils::encode_did_key;
use std::env;
use rocket::State;
use crate::config::ServerConfig;

async fn validate_submit_plc_operation_request(
    did: &str,
    op: &Operation,
    public_endpoint: &str
) -> Result<(), ApiError> {
    /// Validate Rotation Key
    let private_key = env::var("PDS_PLC_ROTATION_KEY_K256_PRIVATE_KEY_HEX").unwrap();
    let (_, public_key) = get_keys_from_private_key_str(private_key)?;
    let plc_rotation_key = encode_did_key(&public_key);
    if !op.rotation_keys.contains(&plc_rotation_key) {
        return Err(ApiError::InvalidRequest(
            "Rotation keys do not include server's rotation key".to_string(),
        ));
    }

    /// Validate Signing Key
    let private_key = env::var("PDS_REPO_SIGNING_KEY_K256_PRIVATE_KEY_HEX").unwrap();
    let (_, public_key) = get_keys_from_private_key_str(private_key)?;
    let signing_rotation_key = encode_did_key(&public_key);
    match op.verification_methods.get("atproto") {
        None => {
            return Err(ApiError::InvalidRequest("Incorrect signing key".to_string()))
        }
        Some(res) => {
            if res.clone() != signing_rotation_key {
                return Err(ApiError::InvalidRequest("Incorrect signing key".to_string()))
            }
        }
    }

    /// Validate Services
    let services = op.services.get("atproto_pds");
    match services {
        None => {
            return Err(ApiError::InvalidRequest("Missing atproto_pds".to_string()))
        }
        Some(res) => {
            if res.r#type != "AtprotoPersonalDataServer" {
                return Err(ApiError::InvalidRequest("Incorrect type on atproto_pds service".to_string()))
            }
            if res.endpoint != public_endpoint.to_string() {
                return Err(ApiError::InvalidRequest("Incorrect endpoint on atproto_pds service".to_string()))
            }
        }
    }

    /// Validate Handle
    let account = match AccountManager::get_account(
        &did.to_string(),
        Some(AvailabilityFlags {
            include_deactivated: Some(true),
            include_taken_down: None,
        }),
    )
        .await
    {
        Ok(res) => match res {
            None => {
                tracing::error!("Unable to find account with valid token");
                return Err(ApiError::RuntimeError);
            }
            Some(actor_account) => actor_account,
        },
        Err(error) => {
            tracing::error!("Error looking up account\n{error}");
            return Err(ApiError::RuntimeError);
        }
    };
    match account.handle {
        Some(handle) => {
            let op_handle = match op.also_known_as.get(0) {
                None => { return Err(ApiError::InvalidRequest("No handle provided in operation".to_string()))}
                Some(handle) => { handle.clone() }
            };

            if op_handle != format!("at://{handle}") {
                return Err(ApiError::InvalidRequest("Incorrect handle in alsoKnownAs".to_string()))
            }
        },
        None => {}
    }

    Ok(())
}

#[rocket::post(
    "/xrpc/com.atproto.identity.submitPlcOperation",
    format = "json",
    data = "<body>"
)]
#[tracing::instrument(skip_all)]
pub async fn submit_plc_operation(
    body: Json<Operation>,
    auth: AccessStandard,
    sequencer: &State<SharedSequencer>,
    id_resolver: &State<SharedIdResolver>,
    server_config: &State<ServerConfig>,
) -> Result<(), ApiError> {
    let did = auth.access.credentials.unwrap().did.unwrap();
    let op = body.into_inner();
    let public_endpoint = server_config.service.public_url.as_str();

    validate_submit_plc_operation_request(did.as_str(), &op, public_endpoint).await?;

    let plc_url = env_str("PDS_DID_PLC_URL").unwrap_or("https://plc.directory".to_owned());
    let plc_client = plc::Client::new(plc_url);
    match plc_client
        .send_operation(&did, &OpOrTombstone::Operation(op))
        .await
    {
        Ok(_) => {
            tracing::info!("Successfully sent PLC Update Operation");
        }
        Err(_) => {
            tracing::error!("Failed to update did:plc");
            return Err(ApiError::RuntimeError);
        }
    }
    let mut sequence_lock = sequencer.sequencer.write().await;
    sequence_lock.sequence_identity_evt(did.clone(), None).await?;
    let mut id_lock = id_resolver.id_resolver.write().await;
    match id_lock.did.ensure_resolve(&did, None).await {
        Err(error) => tracing::error!("Failed to fresh did after plc update\n{error}") ,
        Ok(_) => {},
    };
    Ok(())
}