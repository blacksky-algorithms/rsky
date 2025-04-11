use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::apis::com::atproto::server::get_keys_from_private_key_str;
use crate::apis::ApiError;
use crate::auth_verifier::AdminToken;
use crate::config::ServerConfig;
use crate::handle::{normalize_and_validate_handle, HandleValidationContext, HandleValidationOpts};
use crate::{plc, SharedIdResolver, SharedSequencer};
use anyhow::{bail, Result};
use rocket::serde::json::Json;
use rocket::State;
use rsky_common::env::env_str;
use rsky_lexicon::com::atproto::admin::UpdateAccountHandleInput;
use std::env;

async fn inner_update_account_handle(
    body: Json<UpdateAccountHandleInput>,
    sequencer: &State<SharedSequencer>,
    server_config: &State<ServerConfig>,
    id_resolver: &State<SharedIdResolver>,
    account_manager: AccountManager,
) -> Result<()> {
    let UpdateAccountHandleInput { did, handle } = body.into_inner();
    let opts = HandleValidationOpts {
        handle,
        did: Some(did.clone()),
        allow_reserved: None,
    };
    let validation_ctx = HandleValidationContext {
        server_config,
        id_resolver,
    };
    let handle = normalize_and_validate_handle(opts, validation_ctx).await?;

    let account = account_manager
        .get_account(
            &handle,
            Some(AvailabilityFlags {
                include_deactivated: Some(true),
                include_taken_down: Some(true),
            }),
        )
        .await?;

    match account {
        Some(account) if account.did != did => bail!("Handle already taken: {handle}"),
        // This makes the match case complete to make the compiler happy
        // albeit this branch of code will never be reached
        Some(_) => (),
        None => {
            let plc_url = env_str("PDS_DID_PLC_URL").unwrap_or("https://plc.directory".to_owned());
            let plc_client = plc::Client::new(plc_url);
            let private_key = env::var("PDS_PLC_ROTATION_KEY_K256_PRIVATE_KEY_HEX").unwrap();
            let (signing_key, _) = get_keys_from_private_key_str(private_key)?;
            plc_client
                .update_handle(&did, &signing_key, &handle)
                .await?;
            account_manager.update_handle(&did, &handle).await?;
        }
    }
    let mut lock = sequencer.sequencer.write().await;
    lock.sequence_identity_evt(did.clone(), Some(handle.clone()))
        .await?;
    Ok(())
}

#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.admin.updateAccountHandle",
    format = "json",
    data = "<body>"
)]
pub async fn update_account_handle(
    body: Json<UpdateAccountHandleInput>,
    sequencer: &State<SharedSequencer>,
    server_config: &State<ServerConfig>,
    id_resolver: &State<SharedIdResolver>,
    _auth: AdminToken,
    account_manager: AccountManager,
) -> Result<(), ApiError> {
    match inner_update_account_handle(body, sequencer, server_config, id_resolver, account_manager)
        .await
    {
        Ok(_) => Ok(()),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
