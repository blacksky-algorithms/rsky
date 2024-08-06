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
use rsky_lexicon::app::bsky::feed::{AuthorFeed, FeedViewPost, PostView};

const METHOD_NSID: &'static str = "app.bsky.feed.getActorLikes";

pub async fn inner_get_actor_likes(
    _actor: String,
    _limit: Option<u8>,
    _cursor: Option<String>,
    auth: AccessStandard,
    res: HandlerPipeThrough,
    s3_config: &State<SdkConfig>,
    state_local_viewer: &State<SharedLocalViewer>,
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
            )
            .await?;
            Ok(read_afer_write_response)
        }
    }
}

/// Get a list of posts liked by an actor. Does not require auth.
#[rocket::get("/xrpc/app.bsky.feed.getActorLikes?<actor>&<limit>&<cursor>")]
pub async fn get_actor_likes(
    actor: String,
    limit: Option<u8>,
    cursor: Option<String>,
    auth: AccessStandard,
    res: HandlerPipeThrough,
    s3_config: &State<SdkConfig>,
    state_local_viewer: &State<SharedLocalViewer>,
    cfg: &State<ServerConfig>,
) -> Result<ReadAfterWriteResponse<AuthorFeed>, status::Custom<Json<ErrorMessageResponse>>> {
    if let Some(limit) = limit {
        if limit > 100 {
            let bad_request = ErrorMessageResponse {
                code: Some(ErrorCode::BadRequest),
                message: Some("invalid limit".to_string()),
            };
            return Err(status::Custom(Status::BadRequest, Json(bad_request)));
        }
    }
    match cfg.bsky_app_view {
        None => {
            let not_found = ErrorMessageResponse {
                code: Some(ErrorCode::NotFound),
                message: Some("not found".to_string()),
            };
            return Err(status::Custom(Status::NotFound, Json(not_found)));
        }
        Some(_) => match inner_get_actor_likes(
            actor,
            limit,
            cursor,
            auth,
            res,
            s3_config,
            state_local_viewer,
        )
        .await
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

pub fn get_author_munge(
    local_viewer: LocalViewer,
    original: AuthorFeed,
    local: LocalRecords,
    requester: String,
) -> Result<AuthorFeed> {
    let feed = original.feed;
    match local.profile {
        None => Ok(AuthorFeed {
            cursor: original.cursor,
            feed,
        }),
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
            Ok(AuthorFeed {
                cursor: original.cursor,
                feed,
            })
        }
    }
}
