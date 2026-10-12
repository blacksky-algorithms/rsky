use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use crate::auth_verifier::scope::{RpcProxy, Scoped};
use crate::config::ServerConfig;
use crate::read_after_write::feed_json;
use crate::read_after_write::types::LocalRecords;
use crate::read_after_write::util::{handle_read_after_write, ReadAfterWriteResponse};
use crate::read_after_write::viewer::LocalViewer;
use crate::xrpc_server::types::HandlerPipeThrough;
use crate::SharedLocalViewer;
use anyhow::Result;
use rocket::State;
use serde_json::Value;

const METHOD_NSID: &str = "app.bsky.feed.getActorLikes";

#[allow(clippy::too_many_arguments)]
pub async fn inner_get_actor_likes(
    _actor: String,
    _limit: Option<u8>,
    _cursor: Option<String>,
    auth: Scoped<RpcProxy>,
    res: HandlerPipeThrough,
    blobstore_factory: &State<BlobstoreFactory>,
    state_local_viewer: &State<SharedLocalViewer>,
    actor_store: &State<ActorStore>,
    account_manager: AccountManager,
) -> Result<ReadAfterWriteResponse<Value>> {
    let requester: Option<String> = auth.did_opt().await?;
    match requester {
        None => Ok(ReadAfterWriteResponse::HandlerPipeThrough(res)),
        Some(requester) => {
            let read_afer_write_response = handle_read_after_write(
                METHOD_NSID.to_string(),
                requester,
                res,
                get_author_munge,
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

/// Get a list of posts liked by an actor. Does not require auth.
#[allow(clippy::too_many_arguments)]
#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/app.bsky.feed.getActorLikes?<actor>&<limit>&<cursor>")]
pub async fn get_actor_likes(
    actor: String,
    limit: Option<u8>,
    cursor: Option<String>,
    auth: Scoped<RpcProxy>,
    res: HandlerPipeThrough,
    blobstore_factory: &State<BlobstoreFactory>,
    state_local_viewer: &State<SharedLocalViewer>,
    cfg: &State<ServerConfig>,
    actor_store: &State<ActorStore>,
    account_manager: AccountManager,
) -> Result<ReadAfterWriteResponse<Value>, ApiError> {
    if let Some(limit) = limit {
        if limit > 100 {
            return Err(ApiError::InvalidRequest("invalid limit".to_string()));
        }
    }
    match cfg.bsky_app_view {
        None => Err(ApiError::RuntimeError),
        Some(_) => match inner_get_actor_likes(
            actor,
            limit,
            cursor,
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
            Err(error) => {
                tracing::error!("{error}");
                Err(ApiError::RuntimeError)
            }
        },
    }
}

pub fn get_author_munge(
    local_viewer: LocalViewer,
    mut original: Value,
    local: LocalRecords,
    requester: String,
) -> Result<Value> {
    if let Some(profile) = local.profile {
        if let Some(feed) = original.get_mut("feed").and_then(Value::as_array_mut) {
            for item in feed
                .iter_mut()
                .filter(|item| feed_json::str_at(item, "/post/author/did") == Some(&requester))
            {
                local_viewer.overlay_profile_basic(item, &profile.record);
            }
        }
    }
    Ok(original)
}
