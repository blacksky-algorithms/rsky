use crate::auth_verifier::{AccessOutput, AccessStandard};
use crate::config::ServerConfig;
use crate::models::ErrorMessageResponse;
use crate::pipethrough::{pipethrough, OverrideOpts, ProxyRequest};
use crate::read_after_write::util::ReadAfterWriteResponse;
use crate::repo::types::Ids;
use crate::xrpc_server::types::{HandlerPipeThrough, InvalidRequestError};
use crate::{SharedATPAgent, SharedIdResolver};
use anyhow::{anyhow, Result};
use atrium_api::app::bsky::feed::get_feed_generator::{
    Output as AppBskyFeedGetFeedGeneratorOutput, Parameters as AppBskyFeedGetFeedGeneratorParams,
    ParametersData as AppBskyFeedGetFeedGeneratorData,
};
use atrium_ipld::ipld::Ipld as AtriumIpld;
use rocket::http::Status;
use rocket::request::{FromRequest, Outcome, Request};
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::app::bsky::feed::AuthorFeed;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GetFeedPipeThrough {
    pub encoding: String,
    pub buffer: Vec<u8>,
    pub headers: Option<BTreeMap<String, String>>,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for GetFeedPipeThrough {
    type Error = anyhow::Error;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match AccessStandard::from_request(req).await {
            Outcome::Success(output) => {
                let AccessOutput { credentials, .. } = output.access;
                let requester: Option<String> = match credentials {
                    None => None,
                    Some(credentials) => credentials.did,
                };
                if let Some(limit) = req.query_value::<Option<u8>>("limit") {
                    match limit {
                        Ok(limit) => match limit {
                            Some(limit) if limit > 100 => {
                                return Outcome::Error((
                                    Status::BadRequest,
                                    anyhow!("`limit` is invalid"),
                                ))
                            }
                            _ => (),
                        },
                        _ => {
                            return Outcome::Error((
                                Status::BadRequest,
                                anyhow!("`limit` is invalid"),
                            ))
                        }
                    }
                }
                match req.query_value::<String>("feed") {
                    Some(Ok(feed)) => {
                        let app_view_agent = req.guard::<&State<SharedATPAgent>>().await.unwrap();
                        let cfg = req.guard::<&State<ServerConfig>>().await.unwrap();
                        match (&cfg.bsky_app_view, &app_view_agent.app_view_agent) {
                            (Some(_), Some(app_view_agent_unwrapped)) => {
                                let lock = app_view_agent_unwrapped.read().await;
                                let AppBskyFeedGetFeedGeneratorOutput { data, .. } = match lock
                                    .service
                                    .app
                                    .bsky
                                    .feed
                                    .get_feed_generator(AppBskyFeedGetFeedGeneratorParams {
                                        data: AppBskyFeedGetFeedGeneratorData { feed },
                                        extra_data: AtriumIpld::Null,
                                    })
                                    .await
                                {
                                    Ok(res) => res,
                                    Err(error) => {
                                        return Outcome::Error((
                                            Status::BadRequest,
                                            anyhow!(error.to_string()),
                                        ))
                                    }
                                };
                                let headers = req.headers().clone().into_iter().fold(
                                    BTreeMap::new(),
                                    |mut acc: BTreeMap<String, String>, cur| {
                                        let _ = acc.insert(
                                            cur.name().to_string(),
                                            cur.value().to_string(),
                                        );
                                        acc
                                    },
                                );
                                let req = ProxyRequest {
                                    headers,
                                    query: match req.uri().query() {
                                        None => None,
                                        Some(query) => Some(query.to_string()),
                                    },
                                    path: req.uri().path().to_string(),
                                    method: req.method(),
                                    id_resolver: req
                                        .guard::<&State<SharedIdResolver>>()
                                        .await
                                        .unwrap(),
                                    cfg: req.guard::<&State<ServerConfig>>().await.unwrap(),
                                };
                                match pipethrough(
                                    &req,
                                    requester,
                                    OverrideOpts {
                                        aud: Some(data.view.did.to_string()),
                                        lxm: Some(
                                            Ids::AppBskyFeedGetFeedSkeleton.as_str().to_string(),
                                        ),
                                    },
                                )
                                .await
                                {
                                    Ok(res) => Outcome::Success(Self {
                                        encoding: res.encoding,
                                        buffer: res.buffer,
                                        headers: res.headers,
                                    }),
                                    Err(error) => Outcome::Error((Status::BadRequest, error)),
                                }
                            }
                            _ => Outcome::Error((
                                Status::InternalServerError,
                                anyhow!("internal error"),
                            )),
                        }
                    }
                    _ => Outcome::Error((Status::BadRequest, anyhow!("`feed` is invalid"))),
                }
            }
            Outcome::Error(err) => Outcome::Error((
                Status::BadRequest,
                anyhow::Error::new(InvalidRequestError::AuthError(err.1)),
            )),
            _ => panic!("Unexpected outcome during Pipethrough"),
        }
    }
}

/// Get a hydrated feed from an actor's selected feed generator. Implemented by App View.
#[allow(unused_variables)]
#[rocket::get("/xrpc/app.bsky.feed.getFeed?<feed>&<limit>&<cursor>")]
pub async fn get_feed(
    feed: String,
    limit: Option<u8>,
    cursor: Option<String>,
    res: GetFeedPipeThrough,
) -> Result<ReadAfterWriteResponse<AuthorFeed>, status::Custom<Json<ErrorMessageResponse>>> {
    let res = HandlerPipeThrough {
        encoding: res.encoding,
        buffer: res.buffer,
        headers: res.headers,
    };
    Ok(ReadAfterWriteResponse::HandlerPipeThrough(res))
}
