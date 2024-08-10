use crate::account_manager::helpers::account::AccountStatus;
use crate::account_manager::{AccountManager, CreateAccountOpts};
use crate::auth_verifier::UserDidAuthOptional;
use crate::models::{ErrorCode, ErrorMessageResponse};
use crate::repo::aws::s3::S3BlobStore;
use crate::repo::ActorStore;
use crate::DbConn;
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
async fn create(
    mut body: Json<CreateAccountInput>,
    connection: DbConn,
    sequencer: &State<SharedSequencer>,
    s3_config: &State<SdkConfig>,
) -> Result<CreateAccountOutput, anyhow::Error> {
    let CreateAccountInput {
        email,
        handle,
        mut did,
        invite_code,
        password,
        ..
    } = body.clone().into_inner();
    let deactivated = false;
    let cloned_handle = handle.clone();
    connection
        .run(move |conn| {
            match super::lookup_user_by_handle(&cloned_handle, conn) {
                Ok(_) => bail!("User already exists with handle '{}'", cloned_handle),
                Err(error) => {
                    println!("Handle is available: {error:?}");
                    Ok(())
                } // handle is available, lets go
            }
        })
        .await?;
    if let Some(input_recovery_key) = &body.recovery_key {
        body.recovery_key = Some(input_recovery_key.to_owned());
    }
    //@TODO: Lookup user by email as well
    //@TODO: Validate and require invite code

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
        did_doc: None,
    })
}

#[rocket::post(
    "/xrpc/com.atproto.server.createAccount",
    format = "json",
    data = "<body>"
)]
pub async fn server_create_account(
    body: Json<CreateAccountInput>,
    connection: DbConn,
    sequencer: &State<SharedSequencer>,
    s3_config: &State<SdkConfig>,
    auth: UserDidAuthOptional,
) -> Result<Json<CreateAccountOutput>, status::Custom<Json<ErrorMessageResponse>>> {
    // @TODO: Throw error for any plcOp input

    let _requester = match auth.access {
        Some(access) if access.credentials.is_some() => access.credentials.unwrap().iss,
        _ => None,
    };

    // @TODO: Move to validate_inputs_for_local_pds()
    let mut error_msg: Option<String> = None;
    if body.email.is_none() {
        error_msg = Some("Email is required".to_owned());
    };
    if body.password.is_none() {
        error_msg = Some("Password is required".to_owned());
    };
    if body.did.is_some() {
        error_msg = Some("Not yet allowing people to bring their own DID".to_owned());
    };
    if let Some(email) = &body.email {
        let e_slice: &str = &email[..]; // take a full slice of the string
        if !EmailAddress::is_valid(e_slice) {
            error_msg = Some("Invalid email".to_owned());
        }
    }
    // @TODO: Normalize handle as well
    if !super::validate_handle(&body.handle) {
        error_msg = Some("Invalid handle".to_owned());
    };

    // @TODO: Check that the invite code still has uses

    if error_msg.is_some() {
        let internal_error = ErrorMessageResponse {
            code: Some(ErrorCode::InternalServerError),
            message: error_msg,
        };
        return Err(status::Custom(
            Status::InternalServerError,
            Json(internal_error),
        ));
    };

    match create(body, connection, sequencer, s3_config).await {
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
