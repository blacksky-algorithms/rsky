use crate::auth_verifier::AccessPrivileged;
use crate::models::{ErrorCode, ErrorMessageResponse};
use crate::pipethrough::{pipethrough, pipethrough_procedure, OverrideOpts, ProxyRequest};
use crate::read_after_write::util::ReadAfterWriteResponse;
use anyhow::Result;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rsky_lexicon::chat::bsky::convo::{
    DeleteMessageForSelfInput, DeletedMessageView, GetConvoOutput, GetLogOutput, GetMessagesOutput,
    LeaveConvoInput, LeaveConvoOutput, ListConvosOutput, MessageView, MuteConvoInput,
    MuteConvoOutput, SendMessageBatchInput, SendMessageBatchOutput, SendMessageInput,
    UnmuteConvoInput, UnmuteConvoOutput, UpdateReadInput, UpdateReadOutput,
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

#[rocket::get("/xrpc/chat.bsky.actor.exportAccountData")]
pub async fn export_account_data(
    auth: AccessPrivileged,
    req: ProxyRequest<'_>,
) -> Result<ReadAfterWriteResponse<Vec<u8>>, status::Custom<Json<ErrorMessageResponse>>> {
    let requester: Option<String> = match auth.access.credentials {
        None => None,
        Some(credentials) => credentials.did,
    };
    match pipethrough(
        &req,
        requester,
        OverrideOpts {
            aud: None,
            lxm: None,
        },
    )
    .await
    {
        Ok(res) => Ok(ReadAfterWriteResponse::HandlerPipeThrough(res)),
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
    match pipethrough(
        &req,
        requester,
        OverrideOpts {
            aud: None,
            lxm: None,
        },
    )
    .await
    {
        Ok(res) => Ok(ReadAfterWriteResponse::HandlerPipeThrough(res)),
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
    match pipethrough(
        &req,
        requester,
        OverrideOpts {
            aud: None,
            lxm: None,
        },
    )
    .await
    {
        Ok(res) => Ok(ReadAfterWriteResponse::HandlerPipeThrough(res)),
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
    match pipethrough(
        &req,
        requester,
        OverrideOpts {
            aud: None,
            lxm: None,
        },
    )
    .await
    {
        Ok(res) => Ok(ReadAfterWriteResponse::HandlerPipeThrough(res)),
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
    match pipethrough(
        &req,
        requester,
        OverrideOpts {
            aud: None,
            lxm: None,
        },
    )
    .await
    {
        Ok(res) => Ok(ReadAfterWriteResponse::HandlerPipeThrough(res)),
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

#[rocket::post("/xrpc/chat.bsky.actor.leaveConvo", format = "json", data = "<body>")]
pub async fn leave_convo(
    body: Json<LeaveConvoInput>,
    auth: AccessPrivileged,
    req: ProxyRequest<'_>,
) -> Result<ReadAfterWriteResponse<LeaveConvoOutput>, status::Custom<Json<ErrorMessageResponse>>> {
    let requester: Option<String> = match auth.access.credentials {
        None => None,
        Some(credentials) => credentials.did,
    };
    match pipethrough_procedure(&req, requester, Some(body.into_inner())).await {
        Ok(res) => Ok(ReadAfterWriteResponse::HandlerPipeThrough(res)),
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

#[allow(unused_variables)]
#[rocket::get("/xrpc/chat.bsky.actor.listConvos?<limit>&<cursor>")]
pub async fn list_convos(
    limit: Option<u8>,
    cursor: Option<String>,
    auth: AccessPrivileged,
    req: ProxyRequest<'_>,
) -> Result<ReadAfterWriteResponse<ListConvosOutput>, status::Custom<Json<ErrorMessageResponse>>> {
    let requester: Option<String> = match auth.access.credentials {
        None => None,
        Some(credentials) => credentials.did,
    };
    match pipethrough(
        &req,
        requester,
        OverrideOpts {
            aud: None,
            lxm: None,
        },
    )
    .await
    {
        Ok(res) => Ok(ReadAfterWriteResponse::HandlerPipeThrough(res)),
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

#[rocket::post("/xrpc/chat.bsky.actor.muteConvo", format = "json", data = "<body>")]
pub async fn mute_convo(
    body: Json<MuteConvoInput>,
    auth: AccessPrivileged,
    req: ProxyRequest<'_>,
) -> Result<ReadAfterWriteResponse<MuteConvoOutput>, status::Custom<Json<ErrorMessageResponse>>> {
    let requester: Option<String> = match auth.access.credentials {
        None => None,
        Some(credentials) => credentials.did,
    };
    match pipethrough_procedure(&req, requester, Some(body.into_inner())).await {
        Ok(res) => Ok(ReadAfterWriteResponse::HandlerPipeThrough(res)),
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

#[rocket::post("/xrpc/chat.bsky.actor.sendMessage", format = "json", data = "<body>")]
pub async fn send_message(
    body: Json<SendMessageInput>,
    auth: AccessPrivileged,
    req: ProxyRequest<'_>,
) -> Result<ReadAfterWriteResponse<MessageView>, status::Custom<Json<ErrorMessageResponse>>> {
    let requester: Option<String> = match auth.access.credentials {
        None => None,
        Some(credentials) => credentials.did,
    };
    match pipethrough_procedure(&req, requester, Some(body.into_inner())).await {
        Ok(res) => Ok(ReadAfterWriteResponse::HandlerPipeThrough(res)),
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

#[rocket::post(
    "/xrpc/chat.bsky.actor.sendMessageBatch",
    format = "json",
    data = "<body>"
)]
pub async fn send_message_batch(
    body: Json<SendMessageBatchInput>,
    auth: AccessPrivileged,
    req: ProxyRequest<'_>,
) -> Result<
    ReadAfterWriteResponse<SendMessageBatchOutput>,
    status::Custom<Json<ErrorMessageResponse>>,
> {
    let requester: Option<String> = match auth.access.credentials {
        None => None,
        Some(credentials) => credentials.did,
    };
    match pipethrough_procedure(&req, requester, Some(body.into_inner())).await {
        Ok(res) => Ok(ReadAfterWriteResponse::HandlerPipeThrough(res)),
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

#[rocket::post("/xrpc/chat.bsky.actor.unmuteConvo", format = "json", data = "<body>")]
pub async fn unmute_convo(
    body: Json<UnmuteConvoInput>,
    auth: AccessPrivileged,
    req: ProxyRequest<'_>,
) -> Result<ReadAfterWriteResponse<UnmuteConvoOutput>, status::Custom<Json<ErrorMessageResponse>>> {
    let requester: Option<String> = match auth.access.credentials {
        None => None,
        Some(credentials) => credentials.did,
    };
    match pipethrough_procedure(&req, requester, Some(body.into_inner())).await {
        Ok(res) => Ok(ReadAfterWriteResponse::HandlerPipeThrough(res)),
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

#[rocket::post("/xrpc/chat.bsky.actor.updateRead", format = "json", data = "<body>")]
pub async fn update_read(
    body: Json<UpdateReadInput>,
    auth: AccessPrivileged,
    req: ProxyRequest<'_>,
) -> Result<ReadAfterWriteResponse<UpdateReadOutput>, status::Custom<Json<ErrorMessageResponse>>> {
    let requester: Option<String> = match auth.access.credentials {
        None => None,
        Some(credentials) => credentials.did,
    };
    match pipethrough_procedure(&req, requester, Some(body.into_inner())).await {
        Ok(res) => Ok(ReadAfterWriteResponse::HandlerPipeThrough(res)),
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
