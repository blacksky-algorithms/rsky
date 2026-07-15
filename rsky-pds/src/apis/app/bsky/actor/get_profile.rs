use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use crate::auth_verifier::AccessStandard;
use crate::config::ServerConfig;
use crate::read_after_write::types::LocalRecords;
use crate::read_after_write::util::{handle_read_after_write, ReadAfterWriteResponse};
use crate::read_after_write::viewer::LocalViewer;
use crate::xrpc_server::types::HandlerPipeThrough;
use crate::SharedLocalViewer;
use anyhow::Result;
use rocket::State;
use rsky_lexicon::app::bsky::actor::ProfileViewDetailed;

const METHOD_NSID: &str = "app.bsky.actor.getProfile";

pub async fn inner_get_profile(
    // Forwarded by original url in pipethrough
    _actor: String,
    auth: AccessStandard,
    res: HandlerPipeThrough,
    blobstore_factory: &State<BlobstoreFactory>,
    state_local_viewer: &State<SharedLocalViewer>,
    actor_store: &State<ActorStore>,
    account_manager: AccountManager,
) -> Result<ReadAfterWriteResponse<ProfileViewDetailed>, ApiError> {
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
                blobstore_factory,
                state_local_viewer,
                actor_store,
                account_manager,
            )
            .await?;
            Ok(read_afer_write_response)
        }
    }
}

/// Get detailed profile view of an actor. Does not require auth,
/// but contains relevant metadata with auth.
#[allow(clippy::too_many_arguments)]
#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/app.bsky.actor.getProfile?<actor>")]
pub async fn get_profile(
    // Handle or DID of account to fetch profile of.
    actor: String,
    auth: AccessStandard,
    res: HandlerPipeThrough,
    blobstore_factory: &State<BlobstoreFactory>,
    state_local_viewer: &State<SharedLocalViewer>,
    cfg: &State<ServerConfig>,
    actor_store: &State<ActorStore>,
    account_manager: AccountManager,
) -> Result<ReadAfterWriteResponse<ProfileViewDetailed>, ApiError> {
    match cfg.bsky_app_view {
        None => Err(ApiError::AccountNotFound),
        Some(_) => {
            match inner_get_profile(
                actor,
                auth,
                res,
                blobstore_factory,
                state_local_viewer,
                actor_store,
                account_manager,
            )
            .await
            {
                Ok(response) => Ok(response),
                Err(error) => Err(error),
            }
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
            Ok(local_viewer.update_profile_detailed(original, profile.record, local.posts.len()))
        }
    }
}
