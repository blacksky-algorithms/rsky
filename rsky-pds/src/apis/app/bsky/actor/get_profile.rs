use crate::auth_verifier::AccessStandard;
use crate::models::{InternalErrorCode, InternalErrorMessageResponse};
use crate::read_after_write::types::LocalRecords;
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
) -> Result<Json<ProfileViewDetailed>, status::Custom<Json<InternalErrorMessageResponse>>> {
    let requester: Option<String> = match auth.access.credentials {
        None => None,
        Some(credentials) => credentials.did,
    };
    match requester {
        None => {
            todo!()
        }
        Some(requester) => {
            todo!()
        }
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
