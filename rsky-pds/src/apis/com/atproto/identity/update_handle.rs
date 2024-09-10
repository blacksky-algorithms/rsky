use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::AccountManager;
use crate::apis::com::atproto::server::get_keys_from_private_key_str;
use crate::auth_verifier::AccessStandardCheckTakedown;
use crate::common::env::env_str;
use crate::models::{ErrorCode, ErrorMessageResponse};
use crate::{plc, SharedSequencer};
use anyhow::{bail, Result};
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::identity::UpdateHandleInput;
use std::env;

async fn inner_update_handle(
    body: Json<UpdateHandleInput>,
    sequencer: &State<SharedSequencer>,
    auth: AccessStandardCheckTakedown,
) -> Result<()> {
    let UpdateHandleInput { handle } = body.into_inner();
    let requester = auth.access.credentials.unwrap().did.unwrap();

    // @TODO: Implement normalizeAndValidateHandle()
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
        Err(error) => eprintln!(
            "Error: {}; DID: {}; Handle: {}",
            error.to_string(),
            &requester,
            &handle
        ),
    };
    match lock
        .sequence_handle_update(requester.clone(), handle.clone())
        .await
    {
        Ok(_) => (),
        Err(error) => eprintln!(
            "Error: {}; DID: {}; Handle: {}",
            error.to_string(),
            &requester,
            &handle
        ),
    };
    Ok(())
}

#[rocket::post(
    "/xrpc/com.atproto.identity.updateHandle",
    format = "json",
    data = "<body>"
)]
pub async fn update_handle(
    body: Json<UpdateHandleInput>,
    sequencer: &State<SharedSequencer>,
    auth: AccessStandardCheckTakedown,
) -> Result<(), status::Custom<Json<ErrorMessageResponse>>> {
    match inner_update_handle(body, sequencer, auth).await {
        Ok(_) => Ok(()),
        Err(error) => {
            eprintln!("@LOG: ERROR: {error}");
            let internal_error = ErrorMessageResponse {
                code: Some(ErrorCode::InternalServerError),
                message: Some(error.to_string()),
            };
            return Err(status::Custom(
                Status::InternalServerError,
                Json(internal_error),
            ));
        }
    }
}
