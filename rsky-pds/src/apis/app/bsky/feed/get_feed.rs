use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use crate::auth_verifier::scope::{RpcProxy, Scoped};
use crate::config::ServerConfig;
use crate::pipethrough::{pipethrough, OverrideOpts, ProxyRequest};
use crate::read_after_write::util::ReadAfterWriteResponse;
use crate::xrpc_server::types::HandlerPipeThrough;
use crate::SharedIdResolver;
use anyhow::{anyhow, Result};
use rocket::http::Status;
use rocket::request::{FromRequest, Outcome, Request};
use rocket::State;
use rsky_lexicon::app::bsky::feed::AuthorFeed;
use rsky_repo::types::Ids;
use rsky_syntax::aturi::AtUri;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GetFeedPipeThrough {
    pub encoding: String,
    #[serde(with = "serde_bytes")]
    pub buffer: Vec<u8>,
    pub headers: Option<BTreeMap<String, String>>,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for GetFeedPipeThrough {
    type Error = anyhow::Error;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match Scoped::<RpcProxy>::from_request(req).await {
            Outcome::Success(auth) => {
                let requester: Option<String> = match auth.did_opt().await {
                    Ok(requester) => requester,
                    Err(api_error) => {
                        req.local_cache(|| Some(api_error));
                        return Outcome::Error((Status::Forbidden, anyhow!("InsufficientScope")));
                    }
                };
                if let Some(limit) = req.query_value::<Option<u8>>("limit") {
                    match limit {
                        Ok(limit) => match limit {
                            Some(limit) if limit > 100 => {
                                req.local_cache(|| {
                                    Some(ApiError::InvalidRequest("`limit` is invalid".to_string()))
                                });
                                return Outcome::Error((
                                    Status::BadRequest,
                                    anyhow!("`limit` is invalid"),
                                ));
                            }
                            _ => (),
                        },
                        _ => {
                            req.local_cache(|| {
                                Some(ApiError::InvalidRequest("`limit` is invalid".to_string()))
                            });
                            return Outcome::Error((
                                Status::BadRequest,
                                anyhow!("`limit` is invalid"),
                            ));
                        }
                    }
                }
                match req.query_value::<String>("feed") {
                    Some(Ok(feed)) => {
                        let headers = req.headers().clone().into_iter().fold(
                            BTreeMap::new(),
                            |mut acc: BTreeMap<String, String>, cur| {
                                let _ = acc.insert(cur.name().to_string(), cur.value().to_string());
                                acc
                            },
                        );
                        let id_resolver = req.guard::<&State<SharedIdResolver>>().await.unwrap();
                        let cfg = req.guard::<&State<ServerConfig>>().await.unwrap();
                        let actor_store = req.guard::<&State<ActorStore>>().await.unwrap();
                        // The generator's DID comes from its record, fetched
                        // from the app view the request names with the
                        // requester's own service auth, exactly as the
                        // reference does; the app view's own generator lookup
                        // answers differently for generators it has not
                        // hydrated.
                        let feed_uri = match AtUri::new(feed.clone(), None) {
                            Ok(uri) => uri,
                            Err(error) => {
                                req.local_cache(|| {
                                    Some(ApiError::InvalidRequest(format!(
                                        "`feed` is invalid: {error}"
                                    )))
                                });
                                return Outcome::Error((Status::BadRequest, anyhow!(error)));
                            }
                        };
                        let lookup = ProxyRequest {
                            headers: headers.clone(),
                            query: Some(
                                url::form_urlencoded::Serializer::new(String::new())
                                    .append_pair("repo", feed_uri.get_hostname())
                                    .append_pair("collection", &feed_uri.get_collection())
                                    .append_pair("rkey", &feed_uri.get_rkey())
                                    .finish(),
                            ),
                            path: format!("/xrpc/{}", Ids::ComAtprotoRepoGetRecord.as_str()),
                            method: req.method(),
                            id_resolver,
                            cfg,
                            actor_store,
                        };
                        let generator_did = match pipethrough(
                            &lookup,
                            requester.clone(),
                            OverrideOpts {
                                aud: None,
                                lxm: None,
                            },
                        )
                        .await
                        .map_err(|error| error.to_string())
                        .and_then(|res| {
                            serde_json::from_slice::<serde_json::Value>(&res.buffer)
                                .map_err(|error| error.to_string())
                        })
                        .and_then(|record| {
                            record["value"]["did"]
                                .as_str()
                                .map(str::to_owned)
                                .ok_or_else(|| "could not resolve feed did".to_owned())
                        }) {
                            Ok(did) => did,
                            Err(error) => {
                                req.local_cache(|| Some(ApiError::InvalidRequest(error.clone())));
                                return Outcome::Error((Status::BadRequest, anyhow!(error)));
                            }
                        };
                        let proxy_req = ProxyRequest {
                            headers,
                            query: req.uri().query().map(|query| query.to_string()),
                            path: req.uri().path().to_string(),
                            method: req.method(),
                            id_resolver,
                            cfg,
                            actor_store,
                        };
                        match pipethrough(
                            &proxy_req,
                            requester,
                            OverrideOpts {
                                aud: Some(generator_did),
                                lxm: Some(Ids::AppBskyFeedGetFeedSkeleton.as_str().to_string()),
                            },
                        )
                        .await
                        {
                            Ok(res) => Outcome::Success(Self {
                                encoding: res.encoding,
                                buffer: res.buffer,
                                headers: res.headers,
                            }),
                            Err(error) => {
                                req.local_cache(|| {
                                    Some(ApiError::InvalidRequest(error.to_string()))
                                });
                                Outcome::Error((Status::BadRequest, error))
                            }
                        }
                    }
                    _ => {
                        req.local_cache(|| {
                            Some(ApiError::InvalidRequest("`feed` is invalid".to_string()))
                        });
                        Outcome::Error((Status::BadRequest, anyhow!("`feed` is invalid")))
                    }
                }
            }
            Outcome::Error(err) => {
                req.local_cache(|| Some(ApiError::InvalidRequest(err.1.to_string())));
                Outcome::Error((Status::BadRequest, anyhow::Error::new(err.1)))
            }
            _ => panic!("Unexpected outcome during Pipethrough"),
        }
    }
}

/// Get a hydrated feed from an actor's selected feed generator. Implemented by App View.
#[tracing::instrument(skip_all)]
#[allow(unused_variables)]
#[rocket::get("/xrpc/app.bsky.feed.getFeed?<feed>&<limit>&<cursor>")]
pub async fn get_feed(
    feed: String,
    limit: Option<u8>,
    cursor: Option<String>,
    res: GetFeedPipeThrough,
) -> Result<ReadAfterWriteResponse<AuthorFeed>, ApiError> {
    let res = HandlerPipeThrough {
        encoding: res.encoding,
        buffer: res.buffer,
        headers: res.headers,
    };
    Ok(ReadAfterWriteResponse::HandlerPipeThrough(res))
}
