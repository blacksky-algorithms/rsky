use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use crate::auth_verifier::scope::{RpcProxy, Scoped};
use crate::config::{ServerConfig, ServiceConfig};
use crate::xrpc_server::types::{HandlerPipeThrough, InvalidRequestError, XRPCError};
use crate::{context, SharedIdResolver, APP_USER_AGENT};
use anyhow::{bail, Result};
use lazy_static::lazy_static;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, USER_AGENT};
use reqwest::{RequestBuilder, Response};
use rocket::data::ToByteUnit;
use rocket::http::{Method, Status};
use rocket::request::{FromRequest, Outcome, Request};
use rocket::{Data, State};
use rsky_common::{get_service_endpoint, GetServiceEndpointOpts};
use rsky_repo::types::Ids;
use serde::de::DeserializeOwned;
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::LazyLock;
use std::time::{Duration, Instant};
use url::Url;

pub struct OverrideOpts {
    pub aud: Option<String>,
    pub lxm: Option<String>,
}

pub struct UrlAndAud {
    pub url: Url,
    pub aud: String,
    pub lxm: String,
}

pub struct ProxyHeader {
    pub did: String,
    pub service_url: String,
}

pub struct ProxyRequest<'r> {
    pub headers: BTreeMap<String, String>,
    pub query: Option<String>,
    pub path: String,
    pub method: Method,
    pub id_resolver: &'r State<SharedIdResolver>,
    pub cfg: &'r State<ServerConfig>,
    pub actor_store: &'r State<ActorStore>,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for HandlerPipeThrough {
    type Error = anyhow::Error;

    #[tracing::instrument(skip_all)]
    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match Scoped::<RpcProxy>::from_request(req).await {
            Outcome::Success(auth) => {
                let requester: Option<String> = match auth.did_opt().await {
                    Ok(requester) => requester,
                    Err(api_error) => {
                        req.local_cache(|| Some(api_error));
                        return Outcome::Error((
                            Status::Forbidden,
                            anyhow::anyhow!("InsufficientScope"),
                        ));
                    }
                };
                let headers = req.headers().clone().into_iter().fold(
                    BTreeMap::new(),
                    |mut acc: BTreeMap<String, String>, cur| {
                        let _ = acc.insert(cur.name().to_string(), cur.value().to_string());
                        acc
                    },
                );
                let proxy_req = ProxyRequest {
                    headers,
                    query: req.uri().query().map(|query| query.to_string()),
                    path: req.uri().path().to_string(),
                    method: req.method(),
                    id_resolver: req.guard::<&State<SharedIdResolver>>().await.unwrap(),
                    cfg: req.guard::<&State<ServerConfig>>().await.unwrap(),
                    actor_store: req.guard::<&State<ActorStore>>().await.unwrap(),
                };
                match pipethrough(
                    &proxy_req,
                    requester,
                    OverrideOpts {
                        aud: None,
                        lxm: None,
                    },
                )
                .await
                {
                    Ok(res) => Outcome::Success(res),
                    Err(error) => {
                        if let Some(InvalidRequestError::XRPCError(XRPCError::FailedResponse {
                            status,
                            error,
                            message,
                            headers,
                        })) = error.downcast_ref()
                        {
                            tracing::error!("@LOG: XRPC ERROR Status:{status}; Message: {message:?}; Error: {error:?}; Headers: {headers:?}");
                        }
                        let api_error = pipethrough_error(&error);
                        req.local_cache(|| Some(api_error));
                        Outcome::Error((Status::BadRequest, error))
                    }
                }
            }
            Outcome::Error(err) => {
                req.local_cache(|| Some(ApiError::RuntimeError));
                Outcome::Error((Status::BadRequest, anyhow::Error::new(err.1)))
            }
            _ => panic!("Unexpected outcome during Pipethrough"),
        }
    }
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for ProxyRequest<'r> {
    type Error = anyhow::Error;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let headers = req.headers().clone().into_iter().fold(
            BTreeMap::new(),
            |mut acc: BTreeMap<String, String>, cur| {
                let _ = acc.insert(cur.name().to_string(), cur.value().to_string());
                acc
            },
        );
        Outcome::Success(Self {
            headers,
            query: req.uri().query().map(|query| query.to_string()),
            path: req.uri().path().to_string(),
            method: req.method(),
            id_resolver: req.guard::<&State<SharedIdResolver>>().await.unwrap(),
            cfg: req.guard::<&State<ServerConfig>>().await.unwrap(),
            actor_store: req.guard::<&State<ActorStore>>().await.unwrap(),
        })
    }
}

pub async fn pipethrough(
    req: &ProxyRequest<'_>,
    requester: Option<String>,
    override_opts: OverrideOpts,
) -> Result<HandlerPipeThrough> {
    let UrlAndAud {
        url,
        aud,
        lxm: nsid,
    } = format_url_and_aud(req, override_opts.aud).await?;
    let lxm = override_opts.lxm.unwrap_or(nsid);
    let headers = format_headers(req, aud, lxm, requester).await?;
    let req_init = format_req_init(req, url, headers, None)?;
    let res = make_request(req_init).await?;
    parse_proxy_res(res).await
}

pub async fn pipethrough_procedure<T: serde::Serialize>(
    req: &ProxyRequest<'_>,
    requester: Option<String>,
    body: Option<T>,
) -> Result<HandlerPipeThrough> {
    let UrlAndAud {
        url,
        aud,
        lxm: nsid,
    } = format_url_and_aud(req, None).await?;
    let headers = format_headers(req, aud, nsid, requester).await?;
    let encoded_body: Option<Vec<u8>> = match body {
        None => None,
        Some(body) => Some(serde_json::to_string(&body)?.into_bytes()),
    };
    let req_init = format_req_init(req, url, headers, encoded_body)?;
    let res = make_request(req_init).await?;
    parse_proxy_res(res).await
}

#[tracing::instrument(skip_all)]
pub async fn pipethrough_procedure_post(
    req: &ProxyRequest<'_>,
    requester: Option<String>,
    body: Option<Data<'_>>,
) -> Result<HandlerPipeThrough, ApiError> {
    let UrlAndAud {
        url,
        aud,
        lxm: nsid,
    } = format_url_and_aud(req, None)
        .await
        .map_err(|error| pipethrough_error(&error))?;
    let headers = format_headers(req, aud, nsid, requester)
        .await
        .map_err(|error| pipethrough_error(&error))?;
    let encoded_body: Option<JsonValue>;
    match body {
        None => encoded_body = None,
        Some(body) => {
            let res = match body.open(50.megabytes()).into_string().await {
                Ok(res1) => {
                    tracing::info!(res1.value);
                    res1.value
                }
                Err(error) => {
                    tracing::error!("{error}");
                    return Err(ApiError::RuntimeError);
                }
            };
            match serde_json::from_str(res.as_str()) {
                Ok(res) => {
                    encoded_body = Some(res);
                }
                Err(error) => {
                    tracing::error!("{error}");
                    return Err(ApiError::RuntimeError);
                }
            }
        }
    };
    let req_init = format_req_init_with_value(req, url, headers, encoded_body)?;
    let res = make_request(req_init)
        .await
        .map_err(|error| pipethrough_error(&error))?;
    Ok(parse_proxy_res(res).await?)
}

/// Enforce an OAuth session's `rpc:` scope before a call is proxied to
/// another service, at the seam every pipethrough request passes through
/// (`bsky_api_get_forwarder` and friends all resolve to
/// [`HandlerPipeThrough`]). Gating and `transition:generic` handling are
/// [`crate::apis::scoped_session`]'s.
pub async fn assert_rpc_scope(
    granted_scopes: &Option<Vec<String>>,
    req: &ProxyRequest<'_>,
) -> Result<(), ApiError> {
    let Some(scopes) = crate::apis::scoped_session(granted_scopes.as_ref(), true) else {
        return Ok(());
    };
    let lxm = parse_req_nsid(req);
    // An `rpc:` grant is bound to an audience, so a destination we cannot
    // resolve is a destination we cannot show the call is scoped for.
    let aud = match format_url_and_aud(req, None).await {
        Ok(UrlAndAud { aud, .. }) => aud,
        Err(error) => return Err(pipethrough_error(&error)),
    };
    if scopes.allows_rpc(&lxm, &aud) {
        Ok(())
    } else {
        Err(ApiError::InsufficientScope(format!(
            "Token scope does not permit calling {lxm} on {aud}"
        )))
    }
}

// Request setup/formatting
// -------------------

const REQ_HEADERS_TO_FORWARD: [&str; 4] = [
    "accept-language",
    "content-type",
    "atproto-accept-labelers",
    "x-bsky-topics",
];

#[tracing::instrument(skip_all)]
pub async fn format_url_and_aud(
    req: &ProxyRequest<'_>,
    aud_override: Option<String>,
) -> Result<UrlAndAud> {
    let proxy_to = parse_proxy_header(req).await?;
    let nsid = parse_req_nsid(req);
    let default_proxy = default_service(req, &nsid).await;
    let service_url = match proxy_to {
        Some(ref proxy_to) => {
            tracing::info!(
                "@LOG: format_url_and_aud() proxy_to: {:?}",
                proxy_to.service_url
            );
            Some(proxy_to.service_url.clone())
        }
        None => default_proxy
            .as_ref()
            .map(|default_proxy| default_proxy.url.clone()),
    };
    let aud = match aud_override {
        Some(_) => aud_override,
        None => match proxy_to {
            Some(proxy_to) => Some(proxy_to.did),
            None => match default_proxy {
                Some(default_proxy) => Some(default_proxy.did),
                None => None,
            },
        },
    };
    match (service_url, aud) {
        (Some(service_url), Some(aud)) => {
            let mut url = Url::parse(format!("{0}{1}", service_url, req.path).as_str())?;
            if let Some(ref params) = req.query {
                url.set_query(Some(params.as_str()));
            }
            if let Err(refused) = crate::outbound::client().check(&url) {
                tracing::warn!(%refused, "proxy target refused");
                bail!(InvalidRequestError::InvalidServiceUrl(url.to_string()));
            }
            Ok(UrlAndAud {
                url,
                aud,
                lxm: nsid,
            })
        }
        _ => bail!(InvalidRequestError::NoServiceConfigured(req.path.clone())),
    }
}

pub async fn format_headers(
    req: &ProxyRequest<'_>,
    aud: String,
    lxm: String,
    requester: Option<String>,
) -> Result<HeaderMap> {
    let mut headers: HeaderMap = match requester {
        Some(requester) => {
            context::service_auth_headers(req.actor_store, &requester, &aud, &lxm).await?
        }
        None => HeaderMap::new(),
    };
    // forward select headers to upstream services
    for header in REQ_HEADERS_TO_FORWARD {
        let val = req.headers.get(header);
        if let Some(val) = val {
            headers.insert(header, HeaderValue::from_str(val)?);
        }
    }
    Ok(headers)
}

/// A request on the shared outbound transport. Building a client per
/// request loaded the certificate store and opened a new TLS connection
/// every time, which is where the proxy path spent its time under load.
fn proxy_request(method: Method, url: Url, headers: HeaderMap) -> Result<RequestBuilder> {
    let transport = crate::outbound::client().transport();
    let request = match method {
        Method::Get => transport.get(url),
        Method::Head => transport.head(url),
        Method::Post => transport.post(url),
        _ => bail!(InvalidRequestError::MethodNotFound),
    };
    Ok(request.header(USER_AGENT, APP_USER_AGENT).headers(headers))
}

pub fn format_req_init(
    req: &ProxyRequest,
    url: Url,
    headers: HeaderMap,
    body: Option<Vec<u8>>,
) -> Result<RequestBuilder> {
    let request = proxy_request(req.method, url, headers)?;
    Ok(match (req.method, body) {
        (Method::Post, Some(body)) => request.body(body),
        _ => request,
    })
}

pub fn format_req_init_with_value(
    req: &ProxyRequest,
    url: Url,
    headers: HeaderMap,
    body: Option<JsonValue>,
) -> Result<RequestBuilder> {
    let request = proxy_request(req.method, url, headers)?;
    match (req.method, body) {
        (Method::Post, Some(body)) => Ok(request.json(&body)),
        _ => Ok(request),
    }
}

/// Service endpoints already resolved from `atproto-proxy` headers. The
/// app names the same service on every proxied request, and resolving it
/// each time serialised the whole proxy path behind one DID lookup.
static PROXY_TARGETS: LazyLock<std::sync::RwLock<HashMap<String, (String, Instant)>>> =
    LazyLock::new(|| std::sync::RwLock::new(HashMap::new()));
const PROXY_TARGET_TTL: Duration = Duration::from_secs(300);
const PROXY_TARGET_CAPACITY: usize = 4096;

fn cached_proxy_target(header: &str) -> Option<String> {
    let targets = PROXY_TARGETS
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    targets
        .get(header)
        .filter(|(_, resolved_at)| resolved_at.elapsed() < PROXY_TARGET_TTL)
        .map(|(service_url, _)| service_url.clone())
}

fn remember_proxy_target(header: &str, service_url: &str) {
    let mut targets = PROXY_TARGETS
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if targets.len() >= PROXY_TARGET_CAPACITY {
        targets.retain(|_, (_, resolved_at)| resolved_at.elapsed() < PROXY_TARGET_TTL);
        if targets.len() >= PROXY_TARGET_CAPACITY {
            targets.clear();
        }
    }
    targets.insert(header.to_owned(), (service_url.to_owned(), Instant::now()));
}

pub async fn parse_proxy_header(req: &ProxyRequest<'_>) -> Result<Option<ProxyHeader>> {
    let headers = &req.headers;
    let proxy_to: Option<&String> = headers.get("atproto-proxy");
    match proxy_to {
        None => Ok(None),
        Some(proxy_to) => {
            let parts: Vec<&str> = proxy_to.split("#").collect::<Vec<&str>>();
            match (parts.first(), parts.get(1), parts.get(2)) {
                (Some(did), Some(service_id), None) => {
                    let did = did.to_string();
                    if let Some(service_url) = cached_proxy_target(proxy_to) {
                        return Ok(Some(ProxyHeader { did, service_url }));
                    }
                    let id_resolver = req.id_resolver;
                    let lock = id_resolver.id_resolver.read().await;
                    match lock.did.resolve(did.clone(), None).await? {
                        None => bail!(InvalidRequestError::CannotResolveProxyDid),
                        Some(did_doc) => {
                            match get_service_endpoint(
                                did_doc,
                                GetServiceEndpointOpts {
                                    id: format!("#{service_id}"),
                                    r#type: None,
                                },
                            ) {
                                None => bail!(InvalidRequestError::CannotResolveServiceUrl),
                                Some(service_url) => {
                                    remember_proxy_target(proxy_to, &service_url);
                                    Ok(Some(ProxyHeader { did, service_url }))
                                }
                            }
                        }
                    }
                }
                (_, None, _) => bail!(InvalidRequestError::NoServiceId),
                _ => bail!("error parsing atproto-proxy header"),
            }
        }
    }
}

pub fn parse_req_nsid(req: &ProxyRequest) -> String {
    let nsid = req.path.as_str().replace("/xrpc/", "");
    match nsid.ends_with("/") {
        false => nsid,
        true => nsid
            .trim_end_matches(|c| c == nsid.chars().last().unwrap())
            .to_string(),
    }
}

// Sending request
// -------------------
#[tracing::instrument(skip_all)]
pub async fn make_request(req_init: RequestBuilder) -> Result<Response> {
    let res = req_init.send().await;
    match res {
        Err(e) => {
            tracing::error!("@LOG WARN: pipethrough network error {}", e.to_string());
            bail!(InvalidRequestError::XRPCError(XRPCError::UpstreamFailure))
        }
        Ok(res) => match res.error_for_status_ref() {
            Ok(_) => Ok(res),
            Err(_) => {
                let status = res.status().to_string();
                let headers = res.headers().clone();
                let error_body = res.json::<JsonValue>().await?;
                bail!(InvalidRequestError::XRPCError(XRPCError::FailedResponse {
                    status,
                    headers,
                    error: error_body["error"]
                        .as_str()
                        .map(|error_body_error| error_body_error.to_string()),
                    message: error_body["message"]
                        .as_str()
                        .map(|error_body_message| error_body_message.to_string())
                }))
            }
        },
    }
}

// Response parsing/forwarding
// -------------------

const RES_HEADERS_TO_FORWARD: [&str; 5] = [
    "content-type",
    "content-language",
    "atproto-repo-rev",
    "atproto-content-labelers",
    "retry-after",
];

/// Maps a pipethrough failure to an ApiError, preserving the upstream status
/// code and error shape when the upstream responded with an XRPC error.
pub fn pipethrough_error(error: &anyhow::Error) -> ApiError {
    match error.downcast_ref::<InvalidRequestError>() {
        Some(InvalidRequestError::XRPCError(XRPCError::FailedResponse {
            status,
            error,
            message,
            ..
        })) => {
            let code = status
                .split_whitespace()
                .next()
                .and_then(|code| code.parse::<u16>().ok())
                .unwrap_or(502);
            ApiError::UpstreamResponse(
                code,
                error
                    .clone()
                    .unwrap_or_else(|| "UpstreamFailure".to_string()),
                message.clone().unwrap_or_default(),
            )
        }
        Some(InvalidRequestError::XRPCError(XRPCError::UpstreamFailure)) => {
            ApiError::UpstreamResponse(
                502,
                "UpstreamFailure".to_string(),
                "Upstream service unreachable".to_string(),
            )
        }
        Some(err) => ApiError::InvalidRequest(err.to_string()),
        None => ApiError::InvalidRequest(error.to_string()),
    }
}

pub async fn parse_proxy_res(res: Response) -> Result<HandlerPipeThrough> {
    let encoding = match res.headers().get(CONTENT_TYPE) {
        Some(content_type) => content_type.to_str()?,
        None => "application/json",
    };
    // Release borrow
    let encoding = encoding.to_string();
    let res_headers = RES_HEADERS_TO_FORWARD.into_iter().fold(
        BTreeMap::new(),
        |mut acc: BTreeMap<String, String>, cur| {
            let _ = match res.headers().get(cur) {
                Some(res_header_val) => acc.insert(
                    cur.to_string(),
                    res_header_val.clone().to_str().unwrap().to_string(),
                ),
                None => None,
            };
            acc
        },
    );
    let buffer = read_array_buffer_res(res).await?;
    Ok(HandlerPipeThrough {
        encoding,
        buffer,
        headers: Some(res_headers),
    })
}

// Utils
// -------------------

lazy_static! {
    pub static ref PRIVILEGED_METHODS: HashSet<&'static str> = {
        let mut s = HashSet::new();
        s.insert(Ids::ChatBskyActorDeleteAccount.as_str());
        s.insert(Ids::ChatBskyActorExportAccountData.as_str());
        s.insert(Ids::ChatBskyConvoDeleteMessageForSelf.as_str());
        s.insert(Ids::ChatBskyConvoGetConvo.as_str());
        s.insert(Ids::ChatBskyConvoGetConvoForMembers.as_str());
        s.insert(Ids::ChatBskyConvoGetLog.as_str());
        s.insert(Ids::ChatBskyConvoGetMessages.as_str());
        s.insert(Ids::ChatBskyConvoLeaveConvo.as_str());
        s.insert(Ids::ChatBskyConvoListConvos.as_str());
        s.insert(Ids::ChatBskyConvoMuteConvo.as_str());
        s.insert(Ids::ChatBskyConvoSendMessage.as_str());
        s.insert(Ids::ChatBskyConvoSendMessageBatch.as_str());
        s.insert(Ids::ChatBskyConvoUnmuteConvo.as_str());
        s.insert(Ids::ChatBskyConvoUpdateRead.as_str());
        s.insert(Ids::ComAtprotoServerCreateAccount.as_str());
        s
    };

    // These endpoints are related to account management and must be used directly,
    // not proxied or service-authed. Service auth may be utilized between PDS and
    // entryway for these methods.
    pub static ref PROTECTED_METHODS: HashSet<&'static str> = {
        let mut s = HashSet::new();
        s.insert(Ids::ComAtprotoAdminSendEmail.as_str());
        s.insert(Ids::ComAtprotoIdentityRequestPlcOperationSignature.as_str());
        s.insert(Ids::ComAtprotoIdentitySignPlcOperation.as_str());
        s.insert(Ids::ComAtprotoIdentityUpdateHandle.as_str());
        s.insert(Ids::ComAtprotoServerActivateAccount.as_str());
        s.insert(Ids::ComAtprotoServerConfirmEmail.as_str());
        s.insert(Ids::ComAtprotoServerCreateAppPassword.as_str());
        s.insert(Ids::ComAtprotoServerDeactivateAccount.as_str());
        s.insert(Ids::ComAtprotoServerGetAccountInviteCodes.as_str());
        s.insert(Ids::ComAtprotoServerListAppPasswords.as_str());
        s.insert(Ids::ComAtprotoServerRequestAccountDelete.as_str());
        s.insert(Ids::ComAtprotoServerRequestEmailConfirmation.as_str());
        s.insert(Ids::ComAtprotoServerRequestEmailUpdate.as_str());
        s.insert(Ids::ComAtprotoServerRevokeAppPassword.as_str());
        s.insert(Ids::ComAtprotoServerUpdateEmail.as_str());
        s
    };

}

/// The service a method reaches without an `atproto-proxy` header, as the
/// reference PDS decides it: every `tools.ozone.*` method goes to the
/// moderation service, reports to the report service, everything else to
/// the app view.
pub async fn default_service(req: &ProxyRequest<'_>, nsid: &str) -> Option<ServiceConfig> {
    default_service_for(req.cfg, nsid)
}

pub fn default_service_for(cfg: &ServerConfig, nsid: &str) -> Option<ServiceConfig> {
    if nsid.starts_with("tools.ozone.") {
        cfg.mod_service.clone()
    } else if Ids::from_str(nsid)
        .is_ok_and(|id| matches!(id, Ids::ComAtprotoModerationCreateReport))
    {
        cfg.report_service.clone()
    } else {
        cfg.bsky_app_view.clone()
    }
}

pub fn parse_res<T: DeserializeOwned>(_nsid: String, res: HandlerPipeThrough) -> Result<T> {
    let buffer = res.buffer;
    let record = serde_json::from_slice::<T>(buffer.as_slice())?;
    Ok(record)
}

#[tracing::instrument(skip_all)]
pub async fn read_array_buffer_res(res: Response) -> Result<Vec<u8>> {
    match res.bytes().await {
        Ok(bytes) => Ok(bytes.to_vec()),
        Err(err) => {
            tracing::error!("@LOG WARN: pipethrough network error {}", err.to_string());
            bail!("UpstreamFailure")
        }
    }
}

#[cfg(test)]
mod default_service_tests {
    use super::default_service_for;
    use crate::config::{env_to_cfg, ServiceConfig};

    fn service(name: &str) -> Option<ServiceConfig> {
        Some(ServiceConfig {
            url: format!("https://{name}.example.com"),
            did: format!("did:web:{name}.example.com"),
            cdn_url_pattern: None,
        })
    }

    #[test]
    fn methods_reach_the_reference_default_service() {
        let mut cfg = env_to_cfg();
        cfg.mod_service = service("mod");
        cfg.report_service = service("report");
        cfg.bsky_app_view = service("appview");
        let did_for = |nsid: &str| default_service_for(&cfg, nsid).unwrap().did;
        assert_eq!(
            did_for("tools.ozone.moderation.queryStatuses"),
            "did:web:mod.example.com"
        );
        assert_eq!(
            did_for("tools.ozone.some.futureMethod"),
            "did:web:mod.example.com"
        );
        assert_eq!(
            did_for("com.atproto.moderation.createReport"),
            "did:web:report.example.com"
        );
        assert_eq!(
            did_for("app.bsky.feed.getTimeline"),
            "did:web:appview.example.com"
        );
        assert_eq!(did_for("xyz.unknown.method"), "did:web:appview.example.com");
        cfg.bsky_app_view = None;
        assert!(default_service_for(&cfg, "app.bsky.feed.getTimeline").is_none());
    }
}

#[cfg(test)]
mod proxy_target_tests {
    use super::{
        cached_proxy_target, remember_proxy_target, PROXY_TARGETS, PROXY_TARGET_CAPACITY,
        PROXY_TARGET_TTL,
    };

    #[test]
    fn proxy_targets_are_remembered_until_they_expire() {
        let header = "did:web:cache.test#bsky_appview";
        assert_eq!(cached_proxy_target(header), None);
        remember_proxy_target(header, "https://cache.test");
        assert_eq!(
            cached_proxy_target(header).as_deref(),
            Some("https://cache.test")
        );
        remember_proxy_target(header, "https://cache.test/again");
        assert_eq!(
            cached_proxy_target(header).as_deref(),
            Some("https://cache.test/again")
        );
        {
            let mut targets = PROXY_TARGETS.write().unwrap();
            let expired = std::time::Instant::now() - PROXY_TARGET_TTL * 2;
            targets.insert(
                header.to_owned(),
                ("https://cache.test".to_owned(), expired),
            );
            for i in 0..PROXY_TARGET_CAPACITY {
                targets.insert(
                    format!("did:web:full{i}#svc"),
                    ("https://full.test".to_owned(), expired),
                );
            }
        }
        assert_eq!(cached_proxy_target(header), None);
        remember_proxy_target(header, "https://cache.test/fresh");
        assert_eq!(
            cached_proxy_target(header).as_deref(),
            Some("https://cache.test/fresh")
        );
        assert!(PROXY_TARGETS.read().unwrap().len() <= 2);
    }
}
