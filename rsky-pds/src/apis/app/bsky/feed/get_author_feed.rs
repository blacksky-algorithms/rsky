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
use rocket::form::validate::Contains;
use rocket::State;
use serde_json::Value;

const METHOD_NSID: &str = "app.bsky.feed.getAuthorFeed";

#[allow(clippy::too_many_arguments)]
pub async fn inner_get_author_feed(
    _actor: String,
    _limit: Option<u8>,
    _cursor: Option<String>,
    _filter: Option<String>,
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

/// Get a view of an actor's 'author feed' (post and reposts by the author). Does not require auth.
#[allow(clippy::too_many_arguments)]
#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/app.bsky.feed.getAuthorFeed?<actor>&<limit>&<cursor>&<filter>")]
pub async fn get_author_feed(
    actor: String,
    limit: Option<u8>,
    cursor: Option<String>,
    filter: Option<String>, // Combinations of post/repost types to include in response.
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
            return Err(ApiError::BadRequest(
                "invalid_limit".to_string(),
                "invalid_limit".to_string(),
            ));
        }
    }
    if let Some(ref filter) = filter {
        if !vec![
            "posts_with_replies",
            "posts_no_replies",
            "posts_with_media",
            "posts_and_author_threads",
            "posts_with_video",
        ]
        .contains(filter.as_str())
        {
            return Err(ApiError::BadRequest(
                "invalid filter".to_string(),
                "invalid filter".to_string(),
            ));
        }
    }
    match cfg.bsky_app_view {
        None => {
            return Err(ApiError::BadRequest(
                "not found".to_string(),
                "not found".to_string(),
            ));
        }
        Some(_) => match inner_get_author_feed(
            actor,
            limit,
            cursor,
            filter,
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
            Err(_) => Err(ApiError::RuntimeError),
        },
    }
}

pub fn get_author_munge(
    local_viewer: LocalViewer,
    mut original: Value,
    local: LocalRecords,
    requester: String,
) -> Result<Value> {
    let Some(feed) = original.get_mut("feed").and_then(Value::as_array_mut) else {
        return Ok(original);
    };
    if !feed_json::is_users_feed(feed, &requester) {
        return Ok(original);
    }
    if let Some(profile) = local.profile {
        for item in feed
            .iter_mut()
            .filter(|item| feed_json::str_at(item, "/post/author/did") == Some(&requester))
        {
            local_viewer.overlay_profile_basic(item, &profile.record);
        }
    }
    let feed = std::mem::take(feed);
    let feed = futures::executor::block_on(
        local_viewer.format_and_insert_posts_in_feed(feed, local.posts),
    )?;
    original["feed"] = Value::Array(feed);
    Ok(original)
}
