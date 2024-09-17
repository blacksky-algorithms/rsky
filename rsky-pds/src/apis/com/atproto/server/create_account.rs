use crate::account_manager::helpers::account::AccountStatus;
use crate::account_manager::{AccountManager, CreateAccountOpts};
use crate::apis::com::atproto::server::safe_resolve_did_doc;
use crate::auth_verifier::UserDidAuthOptional;
use crate::config::ServerConfig;
use crate::models::{ErrorCode, ErrorMessageResponse};
use crate::repo::aws::s3::S3BlobStore;
use crate::repo::ActorStore;
use crate::SharedIdResolver;
use crate::SharedSequencer;
use anyhow::{bail, Result};
use aws_config::SdkConfig;
use email_address::*;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::server::{CreateAccountInput, CreateAccountOutput};
use secp256k1::{Keypair, Secp256k1, SecretKey};
use std::env;

#[allow(unused_assignments)]
async fn inner_server_create_account(
    mut body: CreateAccountInput,
    sequencer: &State<SharedSequencer>,
    s3_config: &State<SdkConfig>,
    id_resolver: &State<SharedIdResolver>,
) -> Result<CreateAccountOutput, anyhow::Error> {
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
            eprintln!("{:?}", error);
            bail!("Failed to create DID")
        }
    }
    let did = did.unwrap();

    let mut actor_store = ActorStore::new(did.clone(), S3BlobStore::new(did.clone(), s3_config));
    let commit = match actor_store.create_repo(signing_key, Vec::new()).await {
        Ok(commit) => commit,
        Err(error) => {
            eprintln!("{:?}", error);
            bail!("Failed to create account")
        }
    };

    let did_doc = safe_resolve_did_doc(id_resolver, &did, Some(true)).await?;

    let (access_jwt, refresh_jwt) = AccountManager::create_account(CreateAccountOpts {
        did: did.clone(),
        handle: handle.clone(),
        email,
        password,
        repo_cid: commit.cid,
        repo_rev: commit.rev.clone(),
        invite_code,
        deactivated: Some(deactivated),
    })
    .await?;

    if !deactivated {
        let mut lock = sequencer.sequencer.write().await;
        lock.sequence_identity_evt(did.clone(), Some(handle.clone()))
            .await?;
        lock.sequence_account_evt(did.clone(), AccountStatus::Active)
            .await?;
        lock.sequence_commit(did.clone(), commit.clone(), vec![])
            .await?;
    }
    AccountManager::update_repo_root(did.clone(), commit.cid, commit.rev)?;
    Ok(CreateAccountOutput {
        access_jwt,
        refresh_jwt,
        handle,
        did,
        did_doc: match did_doc {
            None => None,
            Some(did_doc) => Some(serde_json::to_value(did_doc)?),
        },
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
) -> Result<Json<CreateAccountOutput>, status::Custom<Json<ErrorMessageResponse>>> {
    let requester = match auth.access {
        Some(access) if access.credentials.is_some() => access.credentials.unwrap().iss,
        _ => None,
    };
    let input = match validate_inputs_for_local_pds(cfg, body.clone().into_inner(), requester).await
    {
        Ok(res) => res,
        Err(e) => {
            let internal_error = ErrorMessageResponse {
                code: Some(ErrorCode::BadRequest),
                message: Some(e.to_string()),
            };
            return Err(status::Custom(Status::BadRequest, Json(internal_error)));
        }
    };

    match inner_server_create_account(input, sequencer, s3_config, id_resolver).await {
        Ok(response) => Ok(Json(response)),
        Err(error) => {
            eprintln!("Internal Error: {error}");
            let internal_error = ErrorMessageResponse {
                code: Some(ErrorCode::InternalServerError),
                message: Some("Internal error".to_string()),
            };
            return Err(status::Custom(
                Status::InternalServerError,
                Json(internal_error),
            ));
        }
    }
}

pub async fn validate_inputs_for_local_pds(
    cfg: &State<ServerConfig>,
    input: CreateAccountInput,
    _requester: Option<String>,
) -> Result<CreateAccountInput> {
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
        bail!("Unsupported input: `plcOp`");
    }
    if cfg.invites.required && invite_code.is_none() {
        bail!("No invite code provided");
    }
    if email.is_none() {
        bail!("Email is required");
    };
    match email {
        None => bail!("Email is required"),
        Some(email) => {
            let e_slice: &str = &email[..]; // take a full slice of the string
            if !EmailAddress::is_valid(e_slice) {
                bail!("Invalid email");
            }
            if password.is_none() {
                bail!("Password is required");
            };
            if did.is_some() {
                bail!("Not yet allowing people to bring their own DID");
            };
            // @TODO: Normalize handle as well
            if !super::validate_handle(&handle) {
                bail!("Invalid handle");
            };
            if cfg.invites.required && invite_code.is_some() {
                AccountManager::ensure_invite_is_available(invite_code.clone().unwrap()).await?;
            }
            let handle_accnt = AccountManager::get_account(&handle, None).await?;
            let email_accnt = AccountManager::get_account_by_email(&email, None).await?;
            if handle_accnt.is_some() {
                bail!("Handle already taken: {handle}");
            } else if email_accnt.is_some() {
                bail!("Email already taken: {email}");
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
