use rocket::response::status;
use rocket::serde::json::Json;
use crate::models::InternalErrorMessageResponse;
use rsky_lexicon::com::atproto::server::{DeleteAccountInput};
use crate::account_manager::AccountManager;
use crate::account_manager::helpers::account::AvailabilityFlags;
use anyhow::{Result, bail};
use crate::models::models::EmailTokenPurpose;

async fn inner_delete_account(
    body: Json<DeleteAccountInput>
) -> Result<()> {
    let DeleteAccountInput { did, password, token } = body.into_inner();
    let account = AccountManager::get_account(
        &did,
        Some(AvailabilityFlags {
            include_deactivated: Some(true),
            include_taken_down: Some(true),
        })
    ).await?;
    if let Some(_) = account {
        let valid_pass = AccountManager::verify_account_password(&did, &password).await?;
        if !valid_pass {
            bail!("Invalid did or password")
        }
        AccountManager::assert_valid_email_token(
            &did,
            EmailTokenPurpose::from_str("delete_account")?,
            &password
        ).await?
        
    } else {
        bail!("account not found")
    }
    todo!()
}

#[rocket::post(
    "/xrpc/com.atproto.server.deleteAccount",
    format = "json",
    data = "<body>")]
pub async fn delete_account(
    body: Json<DeleteAccountInput>
) -> Result<(), status::Custom<Json<InternalErrorMessageResponse>>>  {
    Ok(())
}
