use crate::auth_verifier::AccessPrivileged;
use crate::models::{ErrorCode, ErrorMessageResponse};
use crate::pipethrough::{pipethrough_procedure, ProxyRequest};
use crate::read_after_write::util::ReadAfterWriteResponse;
use anyhow::Result;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rsky_lexicon::chat::convo::{DeleteMessageForSelfInput, DeletedMessageView};

#[rocket::post("/xrpc/chat.bsky.actor.deleteAccount")]
pub async fn delete_account(
    auth: AccessPrivileged,
    req: ProxyRequest<'_>,
) -> Result<(), status::Custom<Json<ErrorMessageResponse>>> {
    let requester: Option<String> = match auth.access.credentials {
        None => None,
        Some(credentials) => credentials.did,
    };
    match pipethrough_procedure::<()>(&req, requester, None).await {
        Ok(_) => Ok(()),
        Err(error) => {
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

#[rocket::post(
    "/xrpc/chat.bsky.convo.deleteMessageForSelf",
    format = "json",
    data = "<body>"
)]
pub async fn delete_message_for_self(
    auth: AccessPrivileged,
    body: Json<DeleteMessageForSelfInput>,
    req: ProxyRequest<'_>,
) -> Result<ReadAfterWriteResponse<DeletedMessageView>, status::Custom<Json<ErrorMessageResponse>>>
{
    let requester: Option<String> = match auth.access.credentials {
        None => None,
        Some(credentials) => credentials.did,
    };
    match pipethrough_procedure(&req, requester, Some(body.into_inner())).await {
        Ok(res) => Ok(ReadAfterWriteResponse::HandlerPipeThrough(res)),
        Err(error) => {
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
