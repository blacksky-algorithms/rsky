use crate::auth_verifier::AccessStandard;
use crate::config::ServerConfig;
use crate::models::{ErrorCode, ErrorMessageResponse};
use crate::read_after_write::types::{LocalRecords, RecordDescript};
use crate::read_after_write::util::{
    format_munged_response, get_local_lag, get_repo_rev, handle_read_after_write,
    ReadAfterWriteResponse,
};
use crate::read_after_write::viewer::LocalViewer;
use crate::repo::aws::s3::S3BlobStore;
use crate::repo::types::Ids;
use crate::repo::ActorStore;
use crate::xrpc_server::types::{HandlerPipeThrough, InvalidRequestError, XRPCError};
use crate::{SharedLocalViewer, APP_USER_AGENT};
use anyhow::{anyhow, Result};
use atrium_api::app::bsky::feed::get_post_thread::{
    Parameters as AppBskyFeedGetPostThreadParams, ParametersData as AppBskyFeedGetPostThreadData,
};
use atrium_api::client::AtpServiceClient;
use atrium_api::types::LimitedU16;
use atrium_ipld::ipld::Ipld as AtriumIpld;
use atrium_xrpc_client::reqwest::ReqwestClientBuilder;
use aws_config::SdkConfig;
use futures::stream::{self, StreamExt};
use reqwest::header::HeaderMap;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::app::bsky::feed::Post;
use rsky_lexicon::app::bsky::feed::{GetPostThreadOutput, ThreadViewPost, ThreadViewPostEnum};
use rsky_syntax::aturi::AtUri;
use std::collections::BTreeMap;
use std::future::Future;
use std::ops::Deref;
use std::pin::Pin;

const METHOD_NSID: &'static str = "app.bsky.feed.getPostThread";

pub struct ReadAfterWriteNotFoundOutput {
    pub data: GetPostThreadOutput,
    pub lag: Option<usize>,
}

#[allow(non_snake_case)]
#[allow(unused_variables)]
pub async fn inner_get_post_thread(
    uri: String,
    depth: u16,
    parentHeight: u16,
    auth: AccessStandard,
    res: Result<HandlerPipeThrough>,
    s3_config: &State<SdkConfig>,
    state_local_viewer: &State<SharedLocalViewer>,
    cfg: &State<ServerConfig>,
) -> Result<ReadAfterWriteResponse<GetPostThreadOutput>> {
    let requester: String = match auth.access.credentials {
        None => "".to_string(),
        Some(credentials) => credentials.did.unwrap_or("".to_string()),
    };
    match res {
        Ok(res) => {
            let read_afer_write_response = handle_read_after_write(
                METHOD_NSID.to_string(),
                requester,
                res,
                get_post_thread_munge,
                s3_config,
                state_local_viewer,
            )
            .await?;
            Ok(read_afer_write_response)
        }
        Err(err) => match err.downcast_ref() {
            Some(InvalidRequestError::XRPCError(xrpc)) => {
                if let XRPCError::FailedResponse {
                    status,
                    error,
                    message,
                    headers,
                } = xrpc
                {
                    match error {
                        Some(error) if error == "NotFound" => {
                            let actor_store = ActorStore::new(
                                requester.clone(),
                                S3BlobStore::new(requester.clone(), s3_config),
                            );
                            let local_viewer_lock = state_local_viewer.local_viewer.read().await;
                            let local_viewer = local_viewer_lock(actor_store);
                            let local = read_after_write_not_found(
                                local_viewer,
                                uri,
                                parentHeight,
                                requester,
                                Some(headers.clone()),
                                cfg,
                            )
                            .await?;
                            match local {
                                None => Err(err),
                                Some(local) => Ok(ReadAfterWriteResponse::HandlerResponse(
                                    format_munged_response(local.data, local.lag)?,
                                )),
                            }
                        }
                        _ => Err(err),
                    }
                } else {
                    return Err(err);
                }
            }
            _ => Err(err),
        },
    }
}

/// Get posts in a thread. Does not require auth, but additional metadata and filtering
/// will be applied for authed requests.
#[allow(non_snake_case)]
#[allow(unused_variables)]
#[rocket::get("/xrpc/app.bsky.feed.getPostThread?<uri>&<depth>&<parentHeight>")]
pub async fn get_post_thread(
    uri: String,               // Reference (AT-URI) to post record.
    depth: Option<u16>,        // How many levels of reply depth should be included in response.
    parentHeight: Option<u16>, // How many levels of parent (and grandparent, etc.) post to include.
    auth: AccessStandard,
    res: Result<HandlerPipeThrough>,
    s3_config: &State<SdkConfig>,
    state_local_viewer: &State<SharedLocalViewer>,
    cfg: &State<ServerConfig>,
) -> Result<ReadAfterWriteResponse<GetPostThreadOutput>, status::Custom<Json<ErrorMessageResponse>>>
{
    let depth = depth.unwrap_or(6);
    let parentHeight = parentHeight.unwrap_or(80);
    if depth > 1000 || parentHeight > 1000 {
        let bad_request = ErrorMessageResponse {
            code: Some(ErrorCode::BadRequest),
            message: Some("invalid depth or parentHeight. Maximum is 1000.".to_string()),
        };
        return Err(status::Custom(Status::BadRequest, Json(bad_request)));
    }
    match cfg.bsky_app_view {
        None => {
            let not_found = ErrorMessageResponse {
                code: Some(ErrorCode::NotFound),
                message: Some("not found".to_string()),
            };
            return Err(status::Custom(Status::NotFound, Json(not_found)));
        }
        Some(_) => match inner_get_post_thread(
            uri,
            depth,
            parentHeight,
            auth,
            res,
            s3_config,
            state_local_viewer,
            cfg,
        )
        .await
        {
            Ok(response) => Ok(response),
            Err(err) => {
                return match err.downcast_ref() {
                    Some(InvalidRequestError::XRPCError(xrpc)) => {
                        if let XRPCError::FailedResponse {
                            status,
                            error,
                            message,
                            headers,
                        } = xrpc
                        {
                            let xrpc_error = ErrorMessageResponse {
                                code: match error {
                                    None => None,
                                    Some(error) => Some(
                                        ErrorCode::from_str(error)
                                            .unwrap_or(ErrorCode::InternalServerError),
                                    ),
                                },
                                message: match message {
                                    None => None,
                                    Some(message) => Some(message.to_string()),
                                },
                            };
                            Err(status::Custom(Status::BadRequest, Json(xrpc_error)))
                        } else {
                            let internal_error = ErrorMessageResponse {
                                code: Some(ErrorCode::InternalServerError),
                                message: Some(err.to_string()),
                            };
                            Err(status::Custom(
                                Status::InternalServerError,
                                Json(internal_error),
                            ))
                        }
                    }
                    _ => {
                        eprintln!("@LOG: ERROR: {err}");
                        let internal_error = ErrorMessageResponse {
                            code: Some(ErrorCode::InternalServerError),
                            message: Some(err.to_string()),
                        };
                        Err(status::Custom(
                            Status::InternalServerError,
                            Json(internal_error),
                        ))
                    }
                }
            }
        },
    }
}

#[allow(unused_variables)]
pub fn get_post_thread_munge(
    local_viewer: LocalViewer,
    original: GetPostThreadOutput,
    local: LocalRecords,
    requester: String,
) -> Result<GetPostThreadOutput> {
    match original.thread {
        ThreadViewPostEnum::ThreadViewPost(post) => {
            let thread =
                futures::executor::block_on(add_posts_to_thread(&local_viewer, post, local.posts))?;
            Ok(GetPostThreadOutput {
                thread: ThreadViewPostEnum::ThreadViewPost(thread),
            })
        }
        _ => Ok(original),
    }
}

pub async fn add_posts_to_thread(
    local_viewer: &LocalViewer,
    original: ThreadViewPost,
    posts: Vec<RecordDescript<Post>>,
) -> Result<ThreadViewPost> {
    let in_thread = find_posts_in_thread(&original, posts);
    if in_thread.len() == 0 {
        return Ok(original);
    }
    let mut thread = original;
    for record in in_thread {
        thread = insert_into_thread_replies(local_viewer, thread, record).await?;
    }
    Ok(thread)
}

pub fn find_posts_in_thread(
    thread: &ThreadViewPost,
    posts: Vec<RecordDescript<Post>>,
) -> Vec<RecordDescript<Post>> {
    posts
        .into_iter()
        .filter(|post| match post.record.reply {
            None => false,
            Some(ref reply) => {
                if reply.root.uri == thread.post.uri {
                    return true;
                }
                match serde_json::from_value::<Post>(thread.post.record.clone()) {
                    Err(_) => false,
                    Ok(thread_post_record) => match thread_post_record.reply {
                        None => false,
                        Some(thread_post_record_reply) => {
                            thread_post_record_reply.root.uri == reply.root.uri
                        }
                    },
                }
            }
        })
        .collect::<Vec<RecordDescript<Post>>>()
}

pub fn insert_into_thread_replies<'a>(
    local_viewer: &'a LocalViewer,
    view: ThreadViewPost,
    descript: RecordDescript<Post>,
) -> Pin<Box<dyn Future<Output = Result<ThreadViewPost>> + Send + 'a>> {
    Box::pin(async move {
        if let Some(ref reply) = descript.record.reply {
            if reply.parent.uri == view.post.uri {
                return match thread_post_view(local_viewer, descript).await? {
                    None => Ok(view),
                    Some(post_view) => {
                        let ThreadViewPost {
                            post,
                            parent,
                            replies,
                        } = view;
                        let mut view_replies = replies.unwrap_or(vec![]);
                        let mut replies =
                            vec![Box::new(ThreadViewPostEnum::ThreadViewPost(post_view))];
                        replies.append(&mut view_replies);
                        Ok(ThreadViewPost {
                            post,
                            parent,
                            replies: Some(replies),
                        })
                    }
                };
            }
        }

        let ThreadViewPost {
            post,
            parent,
            replies,
        } = view;
        match replies {
            None => Ok(ThreadViewPost {
                post,
                parent,
                replies,
            }),
            Some(ref view_replies) => {
                let replies = stream::iter(view_replies)
                    .then(|reply| {
                        let descript_clone = descript.clone();
                        async move {
                            if let ThreadViewPostEnum::ThreadViewPost(reply_post) = reply.deref() {
                                Ok(Box::new(ThreadViewPostEnum::ThreadViewPost(
                                    insert_into_thread_replies(
                                        local_viewer,
                                        reply_post.clone(),
                                        descript_clone,
                                    )
                                    .await?,
                                )))
                            } else {
                                Ok(reply.clone())
                            }
                        }
                    })
                    .collect::<Vec<_>>()
                    .await
                    .into_iter()
                    .collect::<Result<Vec<Box<ThreadViewPostEnum>>>>()?;
                Ok(ThreadViewPost {
                    post,
                    parent,
                    replies: Some(replies),
                })
            }
        }
    })
}

pub async fn thread_post_view(
    local_viewer: &LocalViewer,
    descript: RecordDescript<Post>,
) -> Result<Option<ThreadViewPost>> {
    match local_viewer.get_post(descript).await? {
        None => Ok(None),
        Some(post_view) => Ok(Some(ThreadViewPost {
            post: post_view,
            parent: None,
            replies: None,
        })),
    }
}

// Read after write on error
// ---------------------
#[allow(non_snake_case)]
pub async fn read_after_write_not_found(
    local_viewer: LocalViewer,
    uri: String,
    parentHeight: u16,
    requester: String,
    headers: Option<HeaderMap>,
    cfg: &State<ServerConfig>,
) -> Result<Option<ReadAfterWriteNotFoundOutput>> {
    match headers {
        None => Ok(None),
        Some(headers) => {
            let mut header_map = BTreeMap::new();
            for key in headers.keys() {
                let _ = match headers.get(key) {
                    Some(res_header_val) => header_map.insert(
                        key.to_string(),
                        res_header_val.clone().to_str().unwrap().to_string(),
                    ),
                    None => None,
                };
            }
            match get_repo_rev(&header_map) {
                None => Ok(None),
                Some(rev) => {
                    let uri = AtUri::new(uri, None)?;
                    if uri.get_hostname() != &requester {
                        return Ok(None);
                    }
                    let local = local_viewer.get_records_since_rev(rev).await?;
                    let found = local
                        .posts
                        .iter()
                        .find(|p| p.uri.to_string() == uri.to_string());
                    match found {
                        None => Ok(None),
                        Some(found) => {
                            let thread = thread_post_view(&local_viewer, found.clone()).await?;
                            match thread {
                                None => Ok(None),
                                Some(thread) => {
                                    let rest = local
                                        .posts
                                        .clone()
                                        .into_iter()
                                        .filter(|p| p.uri.to_string() != uri.to_string())
                                        .collect::<Vec<RecordDescript<Post>>>();
                                    let mut thread =
                                        add_posts_to_thread(&local_viewer, thread, rest).await?;
                                    let highest_parent = get_highest_parent(&thread);
                                    if let Some(highest_parent) = highest_parent {
                                        match &cfg.bsky_app_view {
                                            None => (),
                                            Some(bsky_app_view) => {
                                                let nsid = Ids::AppBskyFeedGetPostThread
                                                    .as_str()
                                                    .to_string();
                                                let headers = cfg
                                                    .appview_auth_headers(&requester, &nsid)
                                                    .await?;
                                                let client = ReqwestClientBuilder::new(
                                                    bsky_app_view.url.clone(),
                                                )
                                                .client(
                                                    reqwest::ClientBuilder::new()
                                                        .user_agent(APP_USER_AGENT)
                                                        .timeout(std::time::Duration::from_millis(
                                                            1000,
                                                        ))
                                                        .default_headers(headers)
                                                        .build()
                                                        .unwrap(),
                                                )
                                                .build();
                                                let agent = AtpServiceClient::new(client);
                                                match agent
                                                    .service
                                                    .app
                                                    .bsky
                                                    .feed
                                                    .get_post_thread(
                                                        AppBskyFeedGetPostThreadParams {
                                                            data: AppBskyFeedGetPostThreadData {
                                                                depth: Some(
                                                                    LimitedU16::try_from(0)
                                                                        .map_err(|e| anyhow!(e))?,
                                                                ),
                                                                parent_height: Some(
                                                                    LimitedU16::try_from(
                                                                        parentHeight,
                                                                    )
                                                                    .map_err(|e| anyhow!(e))?,
                                                                ),
                                                                uri: highest_parent,
                                                            },
                                                            extra_data: AtriumIpld::Null,
                                                        },
                                                    )
                                                    .await
                                                {
                                                    Err(_) => (),
                                                    Ok(parents_res) => {
                                                        let val = serde_json::to_value(
                                                            parents_res.data.thread,
                                                        )
                                                        .expect(
                                                            "Couldn't serialize atrium to value.",
                                                        );
                                                        thread.parent = Some(Box::new(serde_json::from_value::<ThreadViewPostEnum>(val).expect("Couldn't deserialize from atrium to value to ThreadViewPostEnum.")));
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    Ok(Some(ReadAfterWriteNotFoundOutput {
                                        data: GetPostThreadOutput {
                                            thread: ThreadViewPostEnum::ThreadViewPost(thread),
                                        },
                                        lag: get_local_lag(&local)?,
                                    }))
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn get_highest_parent(thread: &ThreadViewPost) -> Option<String> {
    match thread.parent {
        Some(ref boxed_parent) => match boxed_parent.deref() {
            ThreadViewPostEnum::ThreadViewPost(parent) => get_highest_parent(parent),
            _ => None,
        },
        _ => match serde_json::from_value::<Post>(thread.post.record.clone()) {
            Err(_) => None,
            Ok(thread_post_record) => match thread_post_record.reply {
                None => None,
                Some(thread_post_record_reply) => Some(thread_post_record_reply.parent.uri),
            },
        },
    }
}
