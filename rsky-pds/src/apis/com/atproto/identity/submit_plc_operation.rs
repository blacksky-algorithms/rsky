use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::server::PDS_PLC_ROTATION_KEYPAIR;
use crate::apis::ApiError;
use crate::auth_verifier::scope::{IdentityFull, Scoped};
use crate::auth_verifier::AccessStandard;
use crate::config::ServerConfig;
use crate::plc::types::{OpOrTombstone, Operation};
use crate::{plc, SharedIdResolver, SharedSequencer};
use rocket::serde::json::Json;
use rocket::State;
use rsky_crypto::utils::encode_did_key;
use rsky_lexicon::com::atproto::identity::SubmitPlcOperationRequest;

#[tracing::instrument(skip_all)]
async fn validate_plc_request(
    did: &str,
    op: &Operation,
    public_endpoint: &str,
    actor_store: &ActorStore,
    account_manager: &AccountManager,
) -> Result<(), ApiError> {
    let public_rotation_key = encode_did_key(&PDS_PLC_ROTATION_KEYPAIR.public_key());
    if !op.rotation_keys.contains(&public_rotation_key) {
        return Err(ApiError::InvalidRequest(
            "Rotation keys do not include server's rotation key".to_string(),
        ));
    }

    let public_signing_key = match actor_store.keypair(did).await {
        Ok(keypair) => encode_did_key(&keypair.public_key()),
        Err(error) => {
            tracing::error!("Failed to load signing key for {did}\n{error}");
            return Err(ApiError::RuntimeError);
        }
    };
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

    let account = match account_manager
        .get_account(
            did,
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
    // `AccessStandard` (its pre-existing tier, app passwords included) named
    // explicitly since it differs from the guard's default `Base`.
    auth: Scoped<IdentityFull, AccessStandard>,
    sequencer: &State<SharedSequencer>,
    id_resolver: &State<SharedIdResolver>,
    server_config: &State<ServerConfig>,
    actor_store: &State<ActorStore>,
    account_manager: AccountManager,
) -> Result<(), ApiError> {
    let did = auth.did().await?;

    //Validate and transform request
    let op = validate_operation_body(body.into_inner())?;

    //Validate PLC Operation is valid
    validate_plc_request(
        did.as_str(),
        &op,
        server_config.service.public_url.as_str(),
        actor_store,
        &account_manager,
    )
    .await?;

    //Send PLC Operation to PLC Service
    do_plc_operation(server_config.identity.plc_url.as_str(), did.as_str(), op).await?;

    //Update Sequencer
    let mut seq_lock = sequencer.sequencer.write().await;
    seq_lock.sequence_identity_evt(did.clone(), None).await?;

    //Refresh DID after PLC update
    let id_lock = id_resolver.id_resolver.write().await;
    if let Err(error) = id_lock.did.ensure_resolve(&did, None).await {
        tracing::error!("Failed to fresh did after plc update\n{error}")
    };

    Ok(())
}
