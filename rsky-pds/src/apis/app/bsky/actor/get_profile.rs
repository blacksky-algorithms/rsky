use crate::auth_verifier::AccessStandard;
use crate::config::ServerConfig;
use crate::models::{ErrorCode, ErrorMessageResponse};
use crate::read_after_write::types::LocalRecords;
use crate::read_after_write::util::{handle_read_after_write, ReadAfterWriteResponse};
use crate::read_after_write::viewer::LocalViewer;
use crate::xrpc_server::types::HandlerPipeThrough;
use crate::SharedLocalViewer;
use anyhow::Result;
use aws_config::SdkConfig;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::app::bsky::actor::ProfileViewDetailed;

const METHOD_NSID: &'static str = "app.bsky.actor.getProfile";

pub async fn inner_get_profile(
    // Forwarded by original url in pipethrough
    _actor: String,
    auth: AccessStandard,
    res: HandlerPipeThrough,
    s3_config: &State<SdkConfig>,
    state_local_viewer: &State<SharedLocalViewer>,
) -> Result<ReadAfterWriteResponse<ProfileViewDetailed>> {
    let requester: Option<String> = match auth.access.credentials {
        None => None,
        Some(credentials) => credentials.did,
    };
    match requester {
        None => Ok(ReadAfterWriteResponse::HandlerPipeThrough(res)),
        Some(requester) => {
            let read_afer_write_response = handle_read_after_write(
                METHOD_NSID.to_string(),
                requester,
                res,
                get_profile_munge,
                s3_config,
                state_local_viewer,
            )
            .await?;
            Ok(read_afer_write_response)
        }
    }
}

/// Get detailed profile view of an actor. Does not require auth,
/// but contains relevant metadata with auth.
#[rocket::get("/xrpc/app.bsky.actor.getProfile?<actor>")]
pub async fn get_profile(
    // Handle or DID of account to fetch profile of.
    actor: String,
    auth: AccessStandard,
    res: HandlerPipeThrough,
    s3_config: &State<SdkConfig>,
    state_local_viewer: &State<SharedLocalViewer>,
    cfg: &State<ServerConfig>,
) -> Result<ReadAfterWriteResponse<ProfileViewDetailed>, status::Custom<Json<ErrorMessageResponse>>>
{
    match cfg.bsky_app_view {
        None => {
            let not_found = ErrorMessageResponse {
                code: Some(ErrorCode::NotFound),
                message: Some("not found".to_string()),
            };
            return Err(status::Custom(Status::NotFound, Json(not_found)));
        }
        Some(_) => match inner_get_profile(actor, auth, res, s3_config, state_local_viewer).await {
            Ok(response) => Ok(response),
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
        },
    }
}

pub fn get_profile_munge(
    local_viewer: LocalViewer,
    original: ProfileViewDetailed,
    local: LocalRecords,
    requester: String,
) -> Result<ProfileViewDetailed> {
    match local.profile {
        None => Ok(original),
        Some(profile) => {
            if original.did != requester {
                return Ok(original);
            }
            Ok(local_viewer.update_profile_detailed(original, profile.record))
        }
    }
}
