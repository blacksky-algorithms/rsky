use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::auth_verifier::AccessStandard;
use crate::config::ServerConfig;
use crate::plc::types::{OpOrTombstone, Operation};
use crate::{plc, SharedIdResolver, SharedSequencer};
use rocket::serde::json::Json;
use rocket::State;
use rsky_crypto::utils::encode_did_key;
use rsky_lexicon::com::atproto::identity::SubmitPlcOperationRequest;
use secp256k1::{Keypair, Secp256k1, SecretKey};
use std::env;

#[tracing::instrument(skip_all)]
fn get_requester_did(auth: &AccessStandard) -> Result<String, ApiError> {
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
fn get_public_rotation_key() -> Result<String, ApiError> {
    let secp = Secp256k1::new();
    let private_rotation_key = match env::var("PDS_PLC_ROTATION_KEY_K256_PRIVATE_KEY_HEX") {
        Ok(res) => res,
        Err(error) => {
            tracing::error!("Error geting rotation private key\n{error}");
            return Err(ApiError::RuntimeError);
        }
    };
    match hex::decode(private_rotation_key.as_bytes()) {
        Ok(bytes) => match SecretKey::from_slice(&bytes) {
            Ok(secret_key) => {
                let rotation_keypair = Keypair::from_secret_key(&secp, &secret_key);
                Ok(encode_did_key(&rotation_keypair.public_key()))
            }
            Err(error) => {
                tracing::error!("Error geting rotation secret key from bytes\n{error}");
                Err(ApiError::RuntimeError)
            }
        },
        Err(error) => {
            tracing::error!("Unable to hex decode rotation key\n{error}");
            Err(ApiError::RuntimeError)
        }
    }
}

#[tracing::instrument(skip_all)]
fn get_public_signing_key() -> Result<String, ApiError> {
    let secp = Secp256k1::new();
    let private_signing_key = match env::var("PDS_REPO_SIGNING_KEY_K256_PRIVATE_KEY_HEX") {
        Ok(res) => res,
        Err(error) => {
            tracing::error!("Error geting signing private key\n{error}");
            return Err(ApiError::RuntimeError);
        }
    };
    match hex::decode(private_signing_key.as_bytes()) {
        Ok(bytes) => match SecretKey::from_slice(&bytes) {
            Ok(secret_key) => {
                let signing_keypair = Keypair::from_secret_key(&secp, &secret_key);
                Ok(encode_did_key(&signing_keypair.public_key()))
            }
            Err(error) => {
                tracing::error!("Error geting signing secret key from bytes\n{error}");
                Err(ApiError::RuntimeError)
            }
        },
        Err(error) => {
            tracing::error!("Unable to hex decode signing key\n{error}");
            Err(ApiError::RuntimeError)
        }
    }
}

#[tracing::instrument(skip_all)]
async fn validate_plc_request(
    did: &str,
    op: &Operation,
    public_endpoint: &str,
) -> Result<(), ApiError> {
    let public_rotation_key = get_public_signing_key()?;
    if !op.rotation_keys.contains(&public_rotation_key) {
        return Err(ApiError::InvalidRequest(
            "Rotation keys do not include server's rotation key".to_string(),
        ));
    }

    let public_signing_key = get_public_signing_key()?;
    match op.verification_methods.get("atproto") {
        None => {
            return Err(ApiError::InvalidRequest(
                "Incorrect signing key".to_string(),
            ))
        }
        Some(res) => {
            if res.clone() != public_signing_key {
                return Err(ApiError::InvalidRequest(
                    "Incorrect signing key".to_string(),
                ));
            }
        }
    }

    let services = op.services.get("atproto_pds");
    match services {
        None => return Err(ApiError::InvalidRequest("Missing atproto_pds".to_string())),
        Some(res) => {
            if res.r#type != "AtprotoPersonalDataServer" {
                return Err(ApiError::InvalidRequest(
                    "Incorrect type on atproto_pds service".to_string(),
                ));
            }
            if res.endpoint != *public_endpoint {
                return Err(ApiError::InvalidRequest(
                    "Incorrect endpoint on atproto_pds service".to_string(),
                ));
            }
        }
    }

    let account = match AccountManager::get_account_legacy(
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
    if let Some(handle) = account.handle {
        let op_handle = match op.also_known_as.first() {
            None => {
                return Err(ApiError::InvalidRequest(
                    "No handle provided in operation".to_string(),
                ))
            }
            Some(handle) => handle.clone(),
        };

        if op_handle != format!("at://{handle}") {
            return Err(ApiError::InvalidRequest(
                "Incorrect handle in alsoKnownAs".to_string(),
            ));
        }
    }

    Ok(())
}

#[tracing::instrument(skip_all)]
async fn do_plc_operation(plc_url: &str, did: &str, op: Operation) -> Result<(), ApiError> {
    let plc_client = plc::Client::new(plc_url.to_string());
    match plc_client
        .send_operation(&did.to_string(), &OpOrTombstone::Operation(op))
        .await
    {
        Ok(_res) => {
            tracing::info!("Successfully sent PLC Update Operation");
            Ok(())
        }
        Err(error) => {
            tracing::error!("Failed to update did:plc\n{error}");
            Err(ApiError::RuntimeError)
        }
    }
}

#[tracing::instrument(skip_all)]
fn validate_operation_body(request: SubmitPlcOperationRequest) -> Result<Operation, ApiError> {
    match serde_json::from_value::<Operation>(request.operation) {
        Ok(op) => {
            tracing::debug!("Sucessfully parsed operation body");
            Ok(op)
        }
        Err(error) => {
            tracing::error!("Error parsing operation body\n{error}");
            Err(ApiError::InvalidRequest("Invalid operation".to_string()))
        }
    }
}

#[rocket::post(
    "/xrpc/com.atproto.identity.submitPlcOperation",
    format = "json",
    data = "<body>"
)]
#[tracing::instrument(skip_all)]
pub async fn submit_plc_operation(
    body: Json<SubmitPlcOperationRequest>,
    auth: AccessStandard,
    sequencer: &State<SharedSequencer>,
    id_resolver: &State<SharedIdResolver>,
    server_config: &State<ServerConfig>,
) -> Result<(), ApiError> {
    let did = get_requester_did(&auth)?;

    //Validate and transform request
    let op = validate_operation_body(body.into_inner())?;

    //Validate PLC Operation is valid
    validate_plc_request(did.as_str(), &op, server_config.service.public_url.as_str()).await?;

    //Send PLC Operation to PLC Service
    do_plc_operation(server_config.identity.plc_url.as_str(), did.as_str(), op).await?;

    //Update Sequencer
    let mut seq_lock = sequencer.sequencer.write().await;
    seq_lock.sequence_identity_evt(did.clone(), None).await?;

    //Refresh DID after PLC update
    let mut id_lock = id_resolver.id_resolver.write().await;
    if let Err(error) = id_lock.did.ensure_resolve(&did, None).await {
        tracing::error!("Failed to fresh did after plc update\n{error}")
    };

    Ok(())
}
