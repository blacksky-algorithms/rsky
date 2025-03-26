use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::auth_verifier::AccessStandard;
use crate::config::ServerConfig;
use crate::db::DbConn;
use crate::read_after_write::types::LocalRecords;
use crate::read_after_write::util::{handle_read_after_write, ReadAfterWriteResponse};
use crate::read_after_write::viewer::LocalViewer;
use crate::xrpc_server::types::HandlerPipeThrough;
use crate::SharedLocalViewer;
use anyhow::Result;
use aws_config::SdkConfig;
use rocket::form::validate::Contains;
use rocket::State;
use rsky_lexicon::app::bsky::feed::{AuthorFeed, FeedViewPost, PostView};

const METHOD_NSID: &str = "app.bsky.feed.getAuthorFeed";

pub async fn inner_get_author_feed(
    _actor: String,
    _limit: Option<u8>,
    _cursor: Option<String>,
    _filter: Option<String>,
    auth: AccessStandard,
    res: HandlerPipeThrough,
    s3_config: &State<SdkConfig>,
    state_local_viewer: &State<SharedLocalViewer>,
    db: DbConn,
    account_manager: AccountManager,
) -> Result<ReadAfterWriteResponse<AuthorFeed>> {
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
                get_author_munge,
                s3_config,
                state_local_viewer,
                db,
                account_manager,
            )
            .await?;
            Ok(read_afer_write_response)
        }
    }
}

/// Get a view of an actor's 'author feed' (post and reposts by the author). Does not require auth.
#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/app.bsky.feed.getAuthorFeed?<actor>&<limit>&<cursor>&<filter>")]
pub async fn get_author_feed(
    actor: String,
    limit: Option<u8>,
    cursor: Option<String>,
    filter: Option<String>, // Combinations of post/repost types to include in response.
    auth: AccessStandard,
    res: HandlerPipeThrough,
    s3_config: &State<SdkConfig>,
    state_local_viewer: &State<SharedLocalViewer>,
    cfg: &State<ServerConfig>,
    db: DbConn,
    account_manager: AccountManager,
) -> Result<ReadAfterWriteResponse<AuthorFeed>, ApiError> {
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
            s3_config,
            state_local_viewer,
            db,
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
    original: AuthorFeed,
    local: LocalRecords,
    requester: String,
) -> Result<AuthorFeed> {
    if !is_users_feed(&original, &requester) {
        return Ok(original);
    }
    let feed = original.feed;
    match local.profile {
        None => {
            let feed = futures::executor::block_on(
                local_viewer.format_and_insert_posts_in_feed(feed, local.posts),
            )?;
            Ok(AuthorFeed {
                cursor: original.cursor,
                feed,
            })
        }
        Some(profile) => {
            let feed = feed
                .into_iter()
                .map(|item| {
                    if item.post.author.did == requester {
                        let FeedViewPost {
                            post,
                            reply,
                            reason,
                            feed_context,
                        } = item;
                        let PostView {
                            uri,
                            cid,
                            author,
                            record,
                            embed,
                            reply_count,
                            repost_count,
                            like_count,
                            indexed_at,
                            viewer,
                            labels,
                        } = post;
                        FeedViewPost {
                            reply,
                            reason,
                            feed_context,
                            post: PostView {
                                uri,
                                cid,
                                author: local_viewer
                                    .update_profile_view_basic(author, profile.record.clone()),
                                record,
                                embed,
                                reply_count,
                                repost_count,
                                like_count,
                                indexed_at,
                                viewer,
                                labels,
                            },
                        }
                    } else {
                        item
                    }
                })
                .collect::<Vec<FeedViewPost>>();
            let feed = futures::executor::block_on(
                local_viewer.format_and_insert_posts_in_feed(feed, local.posts),
            )?;
            Ok(AuthorFeed {
                cursor: original.cursor,
                feed,
            })
        }
    }
}

pub fn is_users_feed(feed: &AuthorFeed, requester: &String) -> bool {
    let first = feed.feed.first();
    match first {
        None => false,
        Some(first) if first.reason.is_none() && &first.post.author.did == requester => true,
        Some(first) => match first.reason {
            Some(ref reason) if &reason.by.did == requester => true,
            _ => false,
        },
    }
}
