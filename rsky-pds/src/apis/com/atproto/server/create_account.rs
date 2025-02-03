use crate::account_manager::helpers::account::AccountStatus;
use crate::account_manager::{AccountManager, CreateAccountOpts};
use crate::apis::com::atproto::server::safe_resolve_did_doc;
use crate::apis::ApiError;
use crate::auth_verifier::UserDidAuthOptional;
use crate::config::ServerConfig;
use crate::handle::{normalize_and_validate_handle, HandleValidationContext, HandleValidationOpts};
use crate::repo::aws::s3::S3BlobStore;
use crate::repo::ActorStore;
use crate::storage::readable_blockstore::ReadableBlockstore;
use crate::storage::sql_repo::SqlRepoReader;
use crate::SharedIdResolver;
use crate::SharedSequencer;
use aws_config::SdkConfig;
use email_address::*;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::server::{CreateAccountInput, CreateAccountOutput};
use secp256k1::{Keypair, Secp256k1, SecretKey};
use std::env;
use std::fmt::Debug;

#[allow(unused_assignments)]
async fn inner_server_create_account<B: ReadableBlockstore + Clone + Debug + Send>(
    mut body: CreateAccountInput,
    sequencer: &State<SharedSequencer>,
    s3_config: &State<SdkConfig>,
    id_resolver: &State<SharedIdResolver>,
) -> Result<CreateAccountOutput, ApiError> {
    let CreateAccountInput {
        email,
        handle,
        mut did, // @TODO: Allow people to bring their own DID
        invite_code,
        password,
        ..
    } = body.clone();
    let deactivated = false;
    if let Some(input_recovery_key) = &body.recovery_key {
        body.recovery_key = Some(input_recovery_key.to_owned());
    }

    let secp = Secp256k1::new();
    let private_key = env::var("PDS_REPO_SIGNING_KEY_K256_PRIVATE_KEY_HEX").unwrap();
    let secret_key = SecretKey::from_slice(&hex::decode(private_key.as_bytes()).unwrap()).unwrap();
    let signing_key = Keypair::from_secret_key(&secp, &secret_key);
    match super::create_did_and_plc_op(&handle, &body, signing_key).await {
        Ok(did_resp) => {
            did = Some(did_resp);
        }
        Err(error) => {
            eprintln!("Failed to create  DID\n{:?}", error);
            return Err(ApiError::RuntimeError);
        }
    }
    let did = did.unwrap();

    let actor_store = ActorStore::new(did.clone(), S3BlobStore::new(did.clone(), s3_config));
    let commit = match actor_store.create_repo(signing_key, Vec::new()).await {
        Ok(commit) => commit,
        Err(error) => {
            eprintln!("Failed to create account\n{:?}", error);
            return Err(ApiError::RuntimeError);
        }
    };

    let did_doc;
    match safe_resolve_did_doc(id_resolver, &did, Some(true)).await {
        Ok(res) => did_doc = res,
        Err(error) => {
            eprintln!("Error resolving DID Doc\n{error}");
            return Err(ApiError::RuntimeError);
        }
    }

    let (access_jwt, refresh_jwt);
    match AccountManager::create_account(CreateAccountOpts {
        did: did.clone(),
        handle: handle.clone(),
        email,
        password,
        repo_cid: commit.cid,
        repo_rev: commit.rev.clone(),
        invite_code,
        deactivated: Some(deactivated),
    })
    .await
    {
        Ok(res) => {
            (access_jwt, refresh_jwt) = res;
        }
        Err(error) => {
            eprintln!("Error creating account\n{error}");
            return Err(ApiError::RuntimeError);
        }
    }

    if !deactivated {
        let mut lock = sequencer.sequencer.write().await;
        match lock
            .sequence_identity_evt(did.clone(), Some(handle.clone()))
            .await
        {
            Ok(_) => {}
            Err(error) => {
                eprintln!("Sequence Identity Event failed\n{error}");
                return Err(ApiError::RuntimeError);
            }
        }
        match lock
            .sequence_account_evt(did.clone(), AccountStatus::Active)
            .await
        {
            Ok(_) => {}
            Err(error) => {
                eprintln!("Sequence Account Event failed\n{error}");
                return Err(ApiError::RuntimeError);
            }
        }
        match lock
            .sequence_commit(did.clone(), commit.clone(), vec![])
            .await
        {
            Ok(_) => {}
            Err(error) => {
                eprintln!("Sequence Commit failed\n{error}");
                return Err(ApiError::RuntimeError);
            }
        }
    }
    match AccountManager::update_repo_root(did.clone(), commit.cid, commit.rev) {
        Ok(_) => {}
        Err(error) => {
            eprintln!("Update Repo Root failed\n{error}");
            return Err(ApiError::RuntimeError);
        }
    }

    let converted_did_doc;
    match did_doc {
        None => converted_did_doc = None,
        Some(did_doc) => match serde_json::to_value(did_doc) {
            Ok(res) => converted_did_doc = Some(res),
            Err(error) => {
                eprintln!("Did Doc failed conversion\n{error}");
                return Err(ApiError::RuntimeError);
            }
        },
    }
    Ok(CreateAccountOutput {
        access_jwt,
        refresh_jwt,
        handle,
        did,
        did_doc: converted_did_doc,
    })
}

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
) -> Result<Json<CreateAccountOutput>, ApiError> {
    let requester = match auth.access {
        Some(access) if access.credentials.is_some() => access.credentials.unwrap().iss,
        _ => None,
    };
    let input =
        match validate_inputs_for_local_pds(cfg, id_resolver, body.clone().into_inner(), requester)
            .await
        {
            Ok(res) => res,
            Err(e) => return Err(e),
        };

    match inner_server_create_account::<SqlRepoReader>(input, sequencer, s3_config, id_resolver)
        .await
    {
        Ok(response) => Ok(Json(response)),
        Err(error) => Err(error),
    }
}

pub async fn validate_inputs_for_local_pds(
    cfg: &State<ServerConfig>,
    id_resolver: &State<SharedIdResolver>,
    input: CreateAccountInput,
    requester: Option<String>,
) -> Result<CreateAccountInput, ApiError> {
    let CreateAccountInput {
        email,
        handle,
        did,
        invite_code,
        verification_code,
        verification_phone,
        password,
        recovery_key,
        plc_op,
    } = input;

    if plc_op.is_some() {
        return Err(ApiError::InvalidRequest(
            "Unsupported input: `plcOp`".to_string(),
        ));
    }
    if cfg.invites.required && invite_code.is_none() {
        return Err(ApiError::InvalidInviteCode);
    }
    if email.is_none() {
        return Err(ApiError::InvalidEmail);
    };
    match email {
        None => Err(ApiError::InvalidEmail),
        Some(email) => {
            let e_slice: &str = &email[..]; // take a full slice of the string
            if !EmailAddress::is_valid(e_slice) {
                return Err(ApiError::InvalidEmail);
            }
            if password.is_none() {
                return Err(ApiError::InvalidPassword);
            };
            //TODO Not yet allowing people to bring their own DID
            if did.is_some() {
                return Err(ApiError::InvalidRequest(
                    "Not yet allowing people to bring their own DID".to_string(),
                ));
            };
            let opts = HandleValidationOpts {
                handle: handle.clone(),
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
            if cfg.invites.required && invite_code.is_some() {
                AccountManager::ensure_invite_is_available(invite_code.clone().unwrap()).await?;
            }
            let handle_accnt = AccountManager::get_account(&handle, None).await?;
            let email_accnt = AccountManager::get_account_by_email(&email, None).await?;
            if handle_accnt.is_some() {
                return Err(ApiError::HandleNotAvailable);
            } else if email_accnt.is_some() {
                return Err(ApiError::EmailNotAvailable);
            }
            Ok(CreateAccountInput {
                email: Some(email),
                handle,
                did,
                invite_code,
                verification_code,
                verification_phone,
                password,
                recovery_key,
                plc_op,
            })
        }
    }
}
