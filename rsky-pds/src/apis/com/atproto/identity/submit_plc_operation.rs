use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::server::PDS_PLC_ROTATION_KEYPAIR;
use crate::apis::com::atproto::space::kem::kem_did_key;
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

/// When this server holds an `#atproto_kem` key for the account, `op` must
/// name it, as it must name its signing key (proposal §KEM key).
///
/// Syncers encapsulate against the key in the DID document, and this server
/// MACs its answers with the private half it holds, so an operation that
/// drops the key leaves no syncer able to check a MAC, and one that names
/// another key leaves every check failing. `getRecommendedDidCredentials`
/// recommends this key, so an operation built from it passes.
///
/// As the reference checks it (bluesky-social/atproto#5706,
/// `submitPlcOperation.ts`, `if (kemKey && ...)`): an account this server
/// holds no key for has none to lose, so its entry is not checked -- an
/// operation built from a document that never named one is accepted -- and a
/// mismatch is refused with the reference's wording.
fn check_kem_key(op: &Operation, kem_key: Option<&str>) -> Result<(), ApiError> {
    match (kem_key, op.verification_methods.get("atproto_kem")) {
        (None, _) => Ok(()),
        (Some(expected), Some(key)) if key == expected => Ok(()),
        (Some(_), _) => Err(ApiError::InvalidRequest("Incorrect KEM key".to_string())),
    }
}

/// Checks `op` against this server's view of the account, and answers the
/// `#atproto_kem` key it was checked against, if this server holds one.
#[tracing::instrument(skip_all)]
async fn validate_plc_request(
    did: &str,
    op: &Operation,
    public_endpoint: &str,
    actor_store: &ActorStore,
    account_manager: &AccountManager,
) -> Result<Option<String>, ApiError> {
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

    // The key `getRecommendedDidCredentials` recommends, if the account has
    // one. Read, not generated: a key made here would be one the operation
    // could not have named.
    let kem_key = match actor_store.kem_key(did).await {
        Ok(Some(key)) => kem_did_key(&key).map(Some),
        Ok(None) => Ok(None),
        Err(error) => Err(error),
    };
    let kem_key = match kem_key {
        Ok(kem_key) => kem_key,
        Err(error) => {
            tracing::error!("Failed to load the KEM key for {did}\n{error}");
            return Err(ApiError::RuntimeError);
        }
    };
    check_kem_key(op, kem_key.as_deref())?;

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

    Ok(kem_key)
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
    let kem_key = validate_plc_request(
        did.as_str(),
        &op,
        server_config.service.public_url.as_str(),
        actor_store,
        &account_manager,
    )
    .await?;

    //Send PLC Operation to PLC Service
    do_plc_operation(server_config.identity.plc_url.as_str(), did.as_str(), op).await?;

    // When this server holds a key, the DID document now carries it. Record
    // it, so the first space write does not ask the directory again; a record
    // left from before is replaced, so it never claims a key the document
    // lacks. With no key, there is no record to keep.
    if let Some(kem_key) = &kem_key {
        if let Err(error) = actor_store.set_kem_published(&did, kem_key).await {
            tracing::warn!(%error, %did, "could not record the published KEM key");
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn op(methods: &[(&str, &str)]) -> Operation {
        Operation {
            r#type: "plc_operation".to_string(),
            rotation_keys: vec![],
            verification_methods: methods
                .iter()
                .map(|(id, key)| (id.to_string(), key.to_string()))
                .collect::<BTreeMap<_, _>>(),
            also_known_as: vec![],
            services: BTreeMap::new(),
            prev: None,
            sig: None,
        }
    }

    #[test]
    fn an_op_naming_this_servers_kem_key_passes() {
        let op = op(&[("atproto", "did:key:zSig"), ("atproto_kem", "did:key:zKem")]);
        assert!(check_kem_key(&op, Some("did:key:zKem")).is_ok());
    }

    /// With no key held here, the entry is not checked, as the reference
    /// does not check it: an operation with or without one passes.
    #[test]
    fn with_no_kem_key_held_the_entry_is_not_checked() {
        assert!(check_kem_key(&op(&[("atproto", "did:key:zSig")]), None).is_ok());
        let named = op(&[("atproto", "did:key:zSig"), ("atproto_kem", "did:key:zAny")]);
        assert!(check_kem_key(&named, None).is_ok());
    }

    /// Dropping the key leaves no syncer able to check a MAC; naming
    /// another leaves every check failing. Both are refused.
    #[test]
    fn an_op_dropping_or_replacing_the_kem_key_is_refused() {
        for op in [
            op(&[("atproto", "did:key:zSig")]),
            op(&[
                ("atproto", "did:key:zSig"),
                ("atproto_kem", "did:key:zOther"),
            ]),
        ] {
            match check_kem_key(&op, Some("did:key:zKem")) {
                Err(ApiError::InvalidRequest(message)) => {
                    assert_eq!(message, "Incorrect KEM key")
                }
                other => panic!("expected InvalidRequest, got {other:?}"),
            }
        }
    }
}
