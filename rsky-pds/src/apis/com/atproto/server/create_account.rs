use crate::account_manager::helpers::account::AccountStatus;
use crate::account_manager::{AccountManager, CreateAccountOpts};
use crate::actor_store::aws::s3::S3BlobStore;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::server::safe_resolve_did_doc;
use crate::apis::ApiError;
use crate::auth_verifier::UserDidAuthOptional;
use crate::config::ServerConfig;
use crate::db::DbConn;
use crate::handle::{normalize_and_validate_handle, HandleValidationContext, HandleValidationOpts};
use crate::plc::operations::{create_op, CreateAtprotoOpInput};
use crate::plc::types::{OpOrTombstone, Operation};
use crate::sequencer::events::sync_evt_data_from_commit;
use crate::SharedSequencer;
use crate::{plc, SharedIdResolver};
use aws_config::SdkConfig;
use email_address::*;
use rocket::serde::json::Json;
use rocket::State;
use rsky_common::env::env_str;
use rsky_crypto::utils::encode_did_key;
use rsky_lexicon::com::atproto::server::{CreateAccountInput, CreateAccountOutput};
use secp256k1::{Keypair, Secp256k1, SecretKey};
use std::env;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TransformedCreateAccountInput {
    pub email: String,
    pub handle: String,
    pub did: String,
    pub invite_code: Option<String>,
    pub password: String,
    pub signing_key: Keypair,
    pub plc_op: Option<Operation>,
    pub deactivated: bool,
}

//TODO: Potential for taking advantage of async better
#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.server.createAccount",
    format = "json",
    data = "<body>"
)]
pub async fn server_create_account(
    body: Json<CreateAccountInput>,
    auth: UserDidAuthOptional,
    sequencer: &State<SharedSequencer>,
    s3_config: &State<SdkConfig>,
    cfg: &State<ServerConfig>,
    id_resolver: &State<SharedIdResolver>,
    db: DbConn,
    blob_db: DbConn,
) -> Result<Json<CreateAccountOutput>, ApiError> {
    tracing::info!("Creating new user account");
    let requester = match auth.access {
        Some(access) if access.credentials.is_some() => access.credentials.unwrap().iss,
        _ => None,
    };
    // @TODO: Evaluate if we need to validate for entryway PDS
    let TransformedCreateAccountInput {
        email,
        handle,
        did,
        invite_code,
        password,
        deactivated,
        plc_op,
        signing_key,
    } = validate_inputs_for_local_pds(cfg, id_resolver, body.into_inner(), requester, &db).await?;

    // Create new actor repo TODO: Proper rollback
    let mut actor_store =
        ActorStore::new(did.clone(), S3BlobStore::new(did.clone(), s3_config), db);
    let commit = match actor_store
        .create_repo(signing_key, Vec::new(), &blob_db)
        .await
    {
        Ok(commit) => commit,
        Err(error) => {
            tracing::error!("Failed to create repo\n{:?}", error);
            actor_store.destroy().await?;
            return Err(ApiError::RuntimeError);
        }
    };

    // Generate a real did with PLC
    match plc_op {
        None => {}
        Some(op) => {
            let plc_url = env_str("PDS_DID_PLC_URL").unwrap_or("https://plc.directory".to_owned());
            let plc_client = plc::Client::new(plc_url);
            match plc_client
                .send_operation(&did, &OpOrTombstone::Operation(op))
                .await
            {
                Ok(_) => {
                    tracing::info!("Succesfully sent PLC Operation")
                }
                Err(_) => {
                    tracing::error!("Failed to create did:plc");
                    actor_store.destroy().await?;
                    return Err(ApiError::RuntimeError);
                }
            }
        }
    }

    let did_doc;
    match safe_resolve_did_doc(id_resolver, &did, Some(true)).await {
        Ok(res) => did_doc = res,
        Err(error) => {
            tracing::error!("Error resolving DID Doc\n{error}");
            actor_store.destroy().await?;
            return Err(ApiError::RuntimeError);
        }
    }

    // Create Account
    let (access_jwt, refresh_jwt);
    match AccountManager::create_account(
        CreateAccountOpts {
            did: did.clone(),
            handle: handle.clone(),
            email: Some(email),
            password: Some(password),
            repo_cid: commit.commit_data.cid,
            repo_rev: commit.commit_data.rev.clone(),
            invite_code,
            deactivated: Some(deactivated),
        },
        &blob_db,
    )
    .await
    {
        Ok(res) => {
            (access_jwt, refresh_jwt) = res;
        }
        Err(error) => {
            tracing::error!("Error creating account\n{error}");
            actor_store.destroy().await.unwrap();
            return Err(ApiError::RuntimeError);
        }
    }

    if !deactivated {
        let mut lock = sequencer.sequencer.write().await;
        match lock
            .sequence_identity_evt(did.clone(), Some(handle.clone()))
            .await
        {
            Ok(_) => {
                tracing::debug!("Sequenece identity event succeeded");
            }
            Err(error) => {
                tracing::error!("Sequence Identity Event failed\n{error}");
                return Err(ApiError::RuntimeError);
            }
        }
        match lock
            .sequence_account_evt(did.clone(), AccountStatus::Active)
            .await
        {
            Ok(_) => {
                tracing::debug!("Sequence account event succeeded");
            }
            Err(error) => {
                tracing::error!("Sequence Account Event failed\n{error}");
                return Err(ApiError::RuntimeError);
            }
        }
        match lock
            .sequence_commit(did.clone(), commit.clone())
            .await
        {
            Ok(_) => {
                tracing::debug!("Sequence commit succeeded");
            }
            Err(error) => {
                tracing::error!("Sequence Commit failed\n{error}");
                return Err(ApiError::RuntimeError);
            }
        }
        match lock
            .sequence_sync_evt(
                did.clone(),
                sync_evt_data_from_commit(commit.clone()).await?,
            )
            .await
        {
            Ok(_) => {
                tracing::debug!("Sequence sync event data from commit succeeded");
            }
            Err(error) => {
                tracing::error!("Sequence sync event data from commit failed\n{error}");
                return Err(ApiError::RuntimeError);
            }
        }
    }
    match AccountManager::update_repo_root(
        did.clone(),
        commit.commit_data.cid,
        commit.commit_data.rev,
        &blob_db,
    )
    .await
    {
        Ok(_) => {
            tracing::debug!("Successfully updated repo root");
        }
        Err(error) => {
            tracing::error!("Update Repo Root failed\n{error}");
            return Err(ApiError::RuntimeError);
        }
    }

    let converted_did_doc;
    match did_doc {
        None => converted_did_doc = None,
        Some(did_doc) => match serde_json::to_value(did_doc) {
            Ok(res) => converted_did_doc = Some(res),
            Err(error) => {
                tracing::error!("Did Doc failed conversion\n{error}");
                return Err(ApiError::RuntimeError);
            }
        },
    }

    Ok(Json(CreateAccountOutput {
        access_jwt,
        refresh_jwt,
        handle,
        did,
        did_doc: converted_did_doc,
    }))
}

/// Validates Create Account Parameters and builds PLC Operation if needed
pub async fn validate_inputs_for_local_pds(
    cfg: &State<ServerConfig>,
    id_resolver: &State<SharedIdResolver>,
    input: CreateAccountInput,
    requester: Option<String>,
    db: &DbConn,
) -> Result<TransformedCreateAccountInput, ApiError> {
    let did: String;
    let plc_op;
    let deactivated: bool;
    let email;
    let password;
    let invite_code;

    //PLC Op Validation
    if input.plc_op.is_some() {
        return Err(ApiError::InvalidRequest(
            "Unsupported input: `plcOp`".to_string(),
        ));
    }

    //Invite Code Validation
    if cfg.invites.required && input.invite_code.is_none() {
        return Err(ApiError::InvalidInviteCode);
    } else {
        invite_code = input.invite_code.clone();
    }

    //Email Validation
    if input.email.is_none() {
        return Err(ApiError::InvalidEmail);
    };
    match input.email {
        None => return Err(ApiError::InvalidEmail),
        Some(ref input_email) => {
            let e_slice: &str = &input_email[..]; // take a full slice of the string
            if !EmailAddress::is_valid(e_slice) {
                return Err(ApiError::InvalidEmail);
            } else {
                email = input_email.clone();
            }
        }
    }

    // Normalize and Ensure Valid Handle
    let opts = HandleValidationOpts {
        handle: input.handle.clone(),
        did: requester.clone(),
        allow_reserved: None,
    };
    let validation_ctx = HandleValidationContext {
        server_config: cfg,
        id_resolver,
    };
    let handle = normalize_and_validate_handle(opts, validation_ctx).await?;
    if !super::validate_handle(&handle) {
        return Err(ApiError::InvalidHandle);
    };

    // Check Handle and Email are still available
    let handle_accnt = AccountManager::get_account(&handle, None, db).await?;
    let email_accnt = AccountManager::get_account_by_email(&email, None, db).await?;
    if handle_accnt.is_some() {
        return Err(ApiError::HandleNotAvailable);
    } else if email_accnt.is_some() {
        return Err(ApiError::EmailNotAvailable);
    }

    // Check password  exists
    match input.password {
        None => return Err(ApiError::InvalidPassword),
        Some(ref pass) => password = pass.clone(),
    };

    // Get Signing Key
    let secp = Secp256k1::new();
    let private_key = env::var("PDS_REPO_SIGNING_KEY_K256_PRIVATE_KEY_HEX").unwrap();
    let secret_key = SecretKey::from_slice(&hex::decode(private_key.as_bytes()).unwrap()).unwrap();
    let signing_key = Keypair::from_secret_key(&secp, &secret_key);

    match input.did {
        Some(input_did) => {
            if input_did == requester.unwrap_or("n/a".to_string()) {
                return Err(ApiError::AuthRequiredError(format!(
                    "Missing auth to create account with did: {input_did}"
                )));
            }
            did = input_did;
            plc_op = None;
            deactivated = true;
        }
        None => {
            let res = format_did_and_plc_op(input, signing_key).await?;
            did = res.0;
            plc_op = Some(res.1);
            deactivated = false;
        }
    };

    Ok(TransformedCreateAccountInput {
        email,
        handle,
        did,
        invite_code,
        password,
        signing_key,
        plc_op,
        deactivated,
    })
}

#[tracing::instrument(skip_all)]
async fn format_did_and_plc_op(
    input: CreateAccountInput,
    signing_key: Keypair,
) -> Result<(String, Operation), ApiError> {
    let mut rotation_keys: Vec<String> = Vec::new();

    //Add user provided rotation key
    if let Some(recovery_key) = &input.recovery_key {
        rotation_keys.push(recovery_key.clone());
    }

    //Add PDS rotation key
    let secp = Secp256k1::new();
    let private_rotation_key = env::var("PDS_PLC_ROTATION_KEY_K256_PRIVATE_KEY_HEX").unwrap();
    let private_secret_key =
        SecretKey::from_slice(&hex::decode(private_rotation_key.as_bytes()).unwrap()).unwrap();
    let rotation_keypair = Keypair::from_secret_key(&secp, &private_secret_key);
    rotation_keys.push(encode_did_key(&rotation_keypair.public_key()));

    //Build PLC Create Operation
    let response;
    let create_op_input = CreateAtprotoOpInput {
        signing_key: encode_did_key(&signing_key.public_key()),
        handle: input.handle,
        pds: format!(
            "https://{}",
            env::var("PDS_HOSTNAME").unwrap_or("localhost".to_owned())
        ),
        rotation_keys,
    };
    match create_op(create_op_input, rotation_keypair.secret_key()).await {
        Ok(res) => {
            response = res;
        }
        Err(error) => {
            tracing::error!("{error}");
            return Err(ApiError::RuntimeError);
        }
    }

    Ok(response)
}
