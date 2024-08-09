use crate::auth_verifier::AccessPrivileged;
use crate::models::{ErrorCode, ErrorMessageResponse};
use crate::pipethrough::{pipethrough, pipethrough_procedure, ProxyRequest};
use crate::read_after_write::util::ReadAfterWriteResponse;
use anyhow::Result;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rsky_lexicon::chat::bsky::convo::{
    DeleteMessageForSelfInput, DeletedMessageView, GetConvoOutput, GetLogOutput, GetMessagesOutput,
    LeaveConvoOutput,
};

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

#[rocket::get("/xrpc/chat.bsky.actor.exportAccountData")]
pub async fn export_account_data(
    auth: AccessPrivileged,
    req: ProxyRequest<'_>,
) -> Result<ReadAfterWriteResponse<Vec<u8>>, status::Custom<Json<ErrorMessageResponse>>> {
    let requester: Option<String> = match auth.access.credentials {
        None => None,
        Some(credentials) => credentials.did,
    };
    match pipethrough(&req, requester, None).await {
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

#[allow(unused_variables)]
#[allow(non_snake_case)]
#[rocket::get("/xrpc/chat.bsky.actor.getConvo?<convoId>")]
pub async fn get_convo(
    convoId: String,
    auth: AccessPrivileged,
    req: ProxyRequest<'_>,
) -> Result<ReadAfterWriteResponse<GetConvoOutput>, status::Custom<Json<ErrorMessageResponse>>> {
    let requester: Option<String> = match auth.access.credentials {
        None => None,
        Some(credentials) => credentials.did,
    };
    match pipethrough(&req, requester, None).await {
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

#[allow(unused_variables)]
#[rocket::get("/xrpc/chat.bsky.actor.getConvoForMembers?<members>")]
pub async fn get_convo_for_members(
    members: Vec<String>,
    auth: AccessPrivileged,
    req: ProxyRequest<'_>,
) -> Result<ReadAfterWriteResponse<GetConvoOutput>, status::Custom<Json<ErrorMessageResponse>>> {
    let requester: Option<String> = match auth.access.credentials {
        None => None,
        Some(credentials) => credentials.did,
    };
    match pipethrough(&req, requester, None).await {
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

#[allow(unused_variables)]
#[rocket::get("/xrpc/chat.bsky.actor.getLog?<cursor>")]
pub async fn get_log(
    cursor: Option<String>,
    auth: AccessPrivileged,
    req: ProxyRequest<'_>,
) -> Result<ReadAfterWriteResponse<GetLogOutput>, status::Custom<Json<ErrorMessageResponse>>> {
    let requester: Option<String> = match auth.access.credentials {
        None => None,
        Some(credentials) => credentials.did,
    };
    match pipethrough(&req, requester, None).await {
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

#[allow(unused_variables)]
#[allow(non_snake_case)]
#[rocket::get("/xrpc/chat.bsky.actor.getMessages?<convoId>&<limit>&<cursor>")]
pub async fn get_messages(
    convoId: String,
    limit: Option<u8>,
    cursor: Option<String>,
    auth: AccessPrivileged,
    req: ProxyRequest<'_>,
) -> Result<ReadAfterWriteResponse<GetMessagesOutput>, status::Custom<Json<ErrorMessageResponse>>> {
    let requester: Option<String> = match auth.access.credentials {
        None => None,
        Some(credentials) => credentials.did,
    };
    match pipethrough(&req, requester, None).await {
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

#[allow(unused_variables)]
#[allow(non_snake_case)]
#[rocket::get("/xrpc/chat.bsky.actor.leaveConvo?<convoId>")]
pub async fn leave_convo(
    convoId: String,
    auth: AccessPrivileged,
    req: ProxyRequest<'_>,
) -> Result<ReadAfterWriteResponse<LeaveConvoOutput>, status::Custom<Json<ErrorMessageResponse>>> {
    let requester: Option<String> = match auth.access.credentials {
        None => None,
        Some(credentials) => credentials.did,
    };
    match pipethrough(&req, requester, None).await {
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