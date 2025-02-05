use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::apis::com::atproto::server::get_keys_from_private_key_str;
use crate::apis::ApiError;
use crate::auth_verifier::AccessStandardCheckTakedown;
use crate::common::env::env_str;
use crate::config::ServerConfig;
use crate::handle::{normalize_and_validate_handle, HandleValidationContext, HandleValidationOpts};
use crate::{plc, SharedIdResolver, SharedSequencer};
use anyhow::{bail, Result};
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::identity::UpdateHandleInput;
use std::env;

#[tracing::instrument(skip_all)]
async fn inner_update_handle(
    body: Json<UpdateHandleInput>,
    sequencer: &State<SharedSequencer>,
    server_config: &State<ServerConfig>,
    id_resolver: &State<SharedIdResolver>,
    auth: AccessStandardCheckTakedown,
) -> Result<()> {
    let UpdateHandleInput { handle } = body.into_inner();
    let requester = auth.access.credentials.unwrap().did.unwrap();

    let opts = HandleValidationOpts {
        handle,
        did: Some(requester.clone()),
        allow_reserved: None,
    };
    let validation_ctx = HandleValidationContext {
        server_config,
        id_resolver,
    };
    let handle = normalize_and_validate_handle(opts, validation_ctx).await?;

    let account = AccountManager::get_account(
        &handle,
        Some(AvailabilityFlags {
            include_deactivated: Some(true),
            include_taken_down: None,
        }),
    )
    .await?;

    match account {
        Some(account) if account.did != requester => bail!("Handle already taken: {handle}"),
        Some(_) => (),
        None => {
            let plc_url = env_str("PDS_DID_PLC_URL").unwrap_or("https://plc.directory".to_owned());
            let plc_client = plc::Client::new(plc_url);
            let private_key = env::var("PDS_PLC_ROTATION_KEY_K256_PRIVATE_KEY_HEX").unwrap();
            let (signing_key, _) = get_keys_from_private_key_str(private_key)?;
            plc_client
                .update_handle(&requester, &signing_key, &handle)
                .await?;
            AccountManager::update_handle(&requester, &handle).await?;
        }
    }
    let mut lock = sequencer.sequencer.write().await;
    match lock
        .sequence_identity_evt(requester.clone(), Some(handle.clone()))
        .await
    {
        Ok(_) => (),
        Err(error) => tracing::error!("Error: {}; DID: {}; Handle: {}", error, &requester, &handle),
    };
    match lock
        .sequence_handle_update(requester.clone(), handle.clone())
        .await
    {
        Ok(_) => (),
        Err(error) => tracing::error!("Error: {}; DID: {}; Handle: {}", error, &requester, &handle),
    };
    Ok(())
}

#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.identity.updateHandle",
    format = "json",
    data = "<body>"
)]
pub async fn update_handle(
    body: Json<UpdateHandleInput>,
    sequencer: &State<SharedSequencer>,
    server_config: &State<ServerConfig>,
    id_resolver: &State<SharedIdResolver>,
    auth: AccessStandardCheckTakedown,
) -> Result<(), ApiError> {
    match inner_update_handle(body, sequencer, server_config, id_resolver, auth).await {
        Ok(_) => Ok(()),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}
