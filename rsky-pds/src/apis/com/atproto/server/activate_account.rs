use rocket::response::status;
use rocket::serde::json::Json;
use rocket::State;
use anyhow::{bail, Result};
use rocket::http::Status;
use crate::apis::com::atproto::server::assert_valid_did_documents_for_service;
use crate::auth_verifier::AccessNotAppPassword;
use crate::models::{InternalErrorCode, InternalErrorMessageResponse};
use crate::SharedSequencer;

async fn inner_activate_account(
    auth: AccessNotAppPassword,
    sequencer: &State<SharedSequencer>,
) -> Result<()> {
    let requester = auth.access.credentials.unwrap().did.unwrap();
    assert_valid_did_documents_for_service(requester).await?;
    
    todo!()
}

#[rocket::post("/xrpc/com.atproto.server.activateAccount")]
pub async fn activate_account(
    auth: AccessNotAppPassword,
    sequencer: &State<SharedSequencer>,
) -> Result<(), status::Custom<Json<InternalErrorMessageResponse>>> {
    match inner_activate_account(auth, sequencer).await {
        Ok(_) => Ok(()),
        Err(error) => {
            eprintln!("Internal Error: {error}");
            let internal_error = InternalErrorMessageResponse {
                code: Some(InternalErrorCode::InternalError),
                message: Some("Internal error".to_string()),
            };
            return Err(status::Custom(
                Status::InternalServerError,
                Json(internal_error),
            ));
        }
    }
}