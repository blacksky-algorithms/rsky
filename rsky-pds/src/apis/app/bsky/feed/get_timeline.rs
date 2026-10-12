use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use crate::auth_verifier::scope::{RpcProxy, Scoped};
use crate::config::ServerConfig;
use crate::read_after_write::types::LocalRecords;
use crate::read_after_write::util::{handle_read_after_write, ReadAfterWriteResponse};
use crate::read_after_write::viewer::LocalViewer;
use crate::xrpc_server::types::HandlerPipeThrough;
use crate::SharedLocalViewer;
use anyhow::Result;
use rocket::State;
use serde_json::Value;

const METHOD_NSID: &str = "app.bsky.feed.getTimeline";

#[allow(clippy::too_many_arguments)]
pub async fn inner_get_timeline(
    _algorithm: Option<String>,
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
                get_timeline_munge,
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

/// Get a view of the requesting account's home timeline.
/// This is expected to be some form of reverse-chronological feed.
#[allow(clippy::too_many_arguments)]
#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/app.bsky.feed.getTimeline?<algorithm>&<limit>&<cursor>")]
pub async fn get_timeline(
    algorithm: Option<String>,
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
        if !(1..=100).contains(&limit) {
            return Err(ApiError::InvalidRequest("invalid limit".to_string()));
        }
    }
    match cfg.bsky_app_view {
        None => return Err(ApiError::RuntimeError),
        Some(_) => match inner_get_timeline(
            algorithm,
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
            Err(_) => {
                return Err(ApiError::RuntimeError);
            }
        },
    }
}

pub fn get_timeline_munge(
    local_viewer: LocalViewer,
    mut original: Value,
    local: LocalRecords,
    _requester: String,
) -> Result<Value> {
    let Some(feed) = original.get_mut("feed").and_then(Value::as_array_mut) else {
        return Ok(original);
    };
    let feed = std::mem::take(feed);
    let feed = futures::executor::block_on(
        local_viewer.format_and_insert_posts_in_feed(feed, local.posts),
    )?;
    original["feed"] = Value::Array(feed);
    Ok(original)
}
