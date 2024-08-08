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
use rsky_lexicon::app::bsky::actor::{GetProfilesOutput, ProfileViewDetailed};

const METHOD_NSID: &'static str = "app.bsky.actor.getProfiles";

pub async fn inner_get_profiles(
    _actors: Vec<String>,
    auth: AccessStandard,
    res: HandlerPipeThrough,
    s3_config: &State<SdkConfig>,
    state_local_viewer: &State<SharedLocalViewer>,
) -> Result<ReadAfterWriteResponse<GetProfilesOutput>> {
    let requester: String = match auth.access.credentials {
        None => "".to_string(),
        Some(credentials) => credentials.did.unwrap_or("".to_string()),
    };
    let read_afer_write_response = handle_read_after_write(
        METHOD_NSID.to_string(),
        requester,
        res,
        get_profiles_munge,
        s3_config,
        state_local_viewer,
    )
    .await?;
    Ok(read_afer_write_response)
}

/// Get detailed profile views of multiple actors.
#[rocket::get("/xrpc/app.bsky.actor.getProfiles?<actors>")]
pub async fn get_profiles(
    actors: Vec<String>,
    auth: AccessStandard,
    res: HandlerPipeThrough,
    s3_config: &State<SdkConfig>,
    state_local_viewer: &State<SharedLocalViewer>,
    cfg: &State<ServerConfig>,
) -> Result<ReadAfterWriteResponse<GetProfilesOutput>, status::Custom<Json<ErrorMessageResponse>>> {
    match cfg.bsky_app_view {
        None => {
            let not_found = ErrorMessageResponse {
                code: Some(ErrorCode::NotFound),
                message: Some("not found".to_string()),
            };
            return Err(status::Custom(Status::NotFound, Json(not_found)));
        }
        Some(_) => match inner_get_profiles(actors, auth, res, s3_config, state_local_viewer).await
        {
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

pub fn get_profiles_munge(
    local_viewer: LocalViewer,
    original: GetProfilesOutput,
    local: LocalRecords,
    requester: String,
) -> Result<GetProfilesOutput> {
    match local.profile {
        None => Ok(original),
        Some(profile) => {
            let profiles = original
                .profiles
                .into_iter()
                .map(|prof| {
                    if prof.did != requester {
                        prof
                    } else {
                        local_viewer.update_profile_detailed(prof, profile.record.clone())
                    }
                })
                .collect::<Vec<ProfileViewDetailed>>();
            Ok(GetProfilesOutput { profiles })
        }
    }
}
