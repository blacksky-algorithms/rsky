use crate::auth_verifier::{AccessOutput, AccessStandard};
use crate::common::{get_service_endpoint, GetServiceEndpointOpts};
use crate::config::{ServerConfig, ServiceConfig};
use crate::repo::types::Ids;
use crate::xrpc_server::types::{HandlerPipeThrough, InvalidRequestError, XRPCError};
use crate::{context, SharedIdResolver, APP_USER_AGENT};
use anyhow::{bail, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use reqwest::{Client, RequestBuilder, Response};
use rocket::http::{Method, Status};
use rocket::request::{FromRequest, Outcome, Request};
use rocket::State;
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;
use std::str::FromStr;
use std::time::Duration;
use url::Url;

pub struct UrlAndAud {
    pub url: Url,
    pub aud: String,
}

pub struct ProxyHeader {
    pub did: String,
    pub service_url: String,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for HandlerPipeThrough {
    type Error = anyhow::Error;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match AccessStandard::from_request(req).await {
            Outcome::Success(output) => {
                let AccessOutput { credentials, .. } = output.access;
                let requester: Option<String> = match credentials {
                    None => None,
                    Some(credentials) => credentials.did,
                };
                match pipethrough(req, requester, None).await {
                    Ok(res) => Outcome::Success(res),
                    Err(error) => Outcome::Error((Status::BadRequest, error)),
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

pub async fn pipethrough<'r>(
    req: &'r Request<'_>,
    requester: Option<String>,
    aud_override: Option<String>,
) -> Result<HandlerPipeThrough> {
    let UrlAndAud { url, aud } = format_url_and_aud(req, aud_override).await?;
    let headers = format_headers(req, aud, requester).await?;
    let req_init = format_req_init(req, url, headers, None)?;
    let res = make_request(req_init).await?;
    parse_proxy_res(res).await
}

// Request setup/formatting
// -------------------

const REQ_HEADERS_TO_FORWARD: [&'static str; 4] = [
    "accept-language",
    "content-type",
    "atproto-accept-labelers",
    "x-bsky-topics",
];

pub async fn format_url_and_aud<'r>(
    req: &'r Request<'_>,
    aud_override: Option<String>,
) -> Result<UrlAndAud> {
    let proxy_to = parse_proxy_header(req).await?;
    let default_proxy = default_service(req).await;
    let service_url = match proxy_to {
        Some(ref proxy_to) => Some(proxy_to.service_url.clone()),
        None => match default_proxy {
            Some(ref default_proxy) => Some(default_proxy.url.clone()),
            None => None,
        },
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
            let mut url =
                Url::parse(format!("https://{0}{1}", req.uri().path(), service_url).as_str())?;
            if let Some(params) = req.uri().query() {
                url.set_query(Some(params.as_str()));
            }
            let cfg = req.guard::<&State<ServerConfig>>().await.unwrap();
            if !cfg.service.dev_mode && !is_safe_url(url.clone()) {
                bail!(InvalidRequestError::InvalidServiceUrl(url.to_string()));
            }
            Ok(UrlAndAud { url, aud })
        }
        _ => bail!(InvalidRequestError::NoServiceConfigured(
            req.uri().path().to_string()
        )),
    }
}

pub async fn format_headers<'r>(
    req: &'r Request<'_>,
    aud: String,
    requester: Option<String>,
) -> Result<HeaderMap> {
    let mut headers: HeaderMap = match requester {
        Some(requester) => context::service_auth_headers(&requester, &aud).await?,
        None => HeaderMap::new(),
    };
    // forward select headers to upstream services
    for header in REQ_HEADERS_TO_FORWARD {
        let val = req.headers().get(header);
        if let Some(val) = val.last() {
            headers.insert(header, HeaderValue::from_str(val)?);
        }
    }
    assert!(headers.contains_key(AUTHORIZATION));
    Ok(headers)
}

pub fn format_req_init(
    req: &Request,
    url: Url,
    headers: HeaderMap,
    body: Option<Vec<u8>>,
) -> Result<RequestBuilder> {
    match req.method() {
        Method::Get => {
            let client = Client::builder()
                .user_agent(APP_USER_AGENT)
                .http2_keep_alive_while_idle(true)
                .http2_keep_alive_timeout(Duration::from_secs(5))
                .default_headers(headers)
                .build()?;
            Ok(client.get(url))
        }
        Method::Head => {
            let client = Client::builder()
                .user_agent(APP_USER_AGENT)
                .http2_keep_alive_while_idle(true)
                .http2_keep_alive_timeout(Duration::from_secs(5))
                .default_headers(headers)
                .build()?;
            Ok(client.head(url))
        }
        Method::Post => {
            let client = Client::builder()
                .user_agent(APP_USER_AGENT)
                .http2_keep_alive_while_idle(true)
                .http2_keep_alive_timeout(Duration::from_secs(5))
                .default_headers(headers)
                .build()?;
            Ok(client.post(url).json(&body))
        }
        _ => bail!(InvalidRequestError::MethodNotFound),
    }
}

pub async fn parse_proxy_header<'r>(req: &'r Request<'_>) -> Result<Option<ProxyHeader>> {
    let headers = req.headers();
    let proxy_to: Option<&str> = headers.get("atproto-proxy").last();
    match proxy_to {
        None => Ok(None),
        Some(proxy_to) => {
            let parts: Vec<&str> = proxy_to.split("#").collect::<Vec<&str>>();
            match (parts.get(0), parts.get(1), parts.get(2)) {
                (Some(did), Some(service_id), None) => {
                    let did = did.to_string();
                    let id_resolver = req.guard::<&State<SharedIdResolver>>().await.unwrap();
                    let mut lock = id_resolver.id_resolver.write().await;
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
                                Some(service_url) => Ok(Some(ProxyHeader { did, service_url })),
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

// Sending request
// -------------------

pub async fn make_request(req_init: RequestBuilder) -> Result<Response> {
    let res = req_init.send().await;
    match res {
        Err(e) => {
            println!("@LOG WARN: pipethrough network error {}", e.to_string());
            bail!(InvalidRequestError::XRPCError(XRPCError::UpstreamFailure))
        }
        Ok(res) => {
            match res.error_for_status() {
                Ok(res) => Ok(res),
                Err(err) => {
                    // @TODO add additional error logging
                    bail!(InvalidRequestError::XRPCError(XRPCError::FailedResponse(
                        err.to_string()
                    )))
                }
            }
        }
    }
}

// Response parsing/forwarding
// -------------------

const RES_HEADERS_TO_FORWARD: [&'static str; 4] = [
    "content-type",
    "content-language",
    "atproto-repo-rev",
    "atproto-content-labelers",
];

pub async fn parse_proxy_res(res: Response) -> Result<HandlerPipeThrough> {
    let encoding = match res.headers().get(CONTENT_TYPE) {
        Some(content_type) => content_type.to_str()?,
        None => "application/json",
    };
    // Release borrow
    let encoding = encoding.to_string();
    let res_headers = RES_HEADERS_TO_FORWARD.clone().into_iter().fold(
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

pub async fn default_service<'r>(req: &'r Request<'_>) -> Option<ServiceConfig> {
    let cfg = req.guard::<&State<ServerConfig>>().await.unwrap();
    let nsid = req.uri().path().as_str().replace("/xrpc/", "");
    match Ids::from_str(&nsid) {
        Ok(Ids::ToolsOzoneTeamAddMember) => cfg.mod_service.clone(),
        Ok(Ids::ToolsOzoneTeamDeleteMember) => cfg.mod_service.clone(),
        Ok(Ids::ToolsOzoneTeamUpdateMember) => cfg.mod_service.clone(),
        Ok(Ids::ToolsOzoneTeamListMembers) => cfg.mod_service.clone(),
        Ok(Ids::ToolsOzoneCommunicationCreateTemplate) => cfg.mod_service.clone(),
        Ok(Ids::ToolsOzoneCommunicationDeleteTemplate) => cfg.mod_service.clone(),
        Ok(Ids::ToolsOzoneCommunicationUpdateTemplate) => cfg.mod_service.clone(),
        Ok(Ids::ToolsOzoneCommunicationListTemplates) => cfg.mod_service.clone(),
        Ok(Ids::ToolsOzoneModerationEmitEvent) => cfg.mod_service.clone(),
        Ok(Ids::ToolsOzoneModerationGetEvent) => cfg.mod_service.clone(),
        Ok(Ids::ToolsOzoneModerationGetRecord) => cfg.mod_service.clone(),
        Ok(Ids::ToolsOzoneModerationGetRepo) => cfg.mod_service.clone(),
        Ok(Ids::ToolsOzoneModerationQueryEvents) => cfg.mod_service.clone(),
        Ok(Ids::ToolsOzoneModerationQueryStatuses) => cfg.mod_service.clone(),
        Ok(Ids::ToolsOzoneModerationSearchRepos) => cfg.mod_service.clone(),
        Ok(Ids::ComAtprotoModerationCreateReport) => cfg.report_service.clone(),
        _ => cfg.bsky_app_view.clone(),
    }
}

pub fn parse_res<T: DeserializeOwned>(_nsid: String, res: HandlerPipeThrough) -> Result<T> {
    let buffer = res.buffer;
    let record = serde_json::from_slice::<T>(buffer.as_slice())?;
    Ok(record)
}

pub async fn read_array_buffer_res(res: Response) -> Result<Vec<u8>> {
    match res.bytes().await {
        Ok(bytes) => Ok(bytes.to_vec()),
        Err(err) => {
            println!("@LOG WARN: pipethrough network error {}", err.to_string());
            bail!("UpstreamFailure")
        }
    }
}

pub fn is_safe_url(url: Url) -> bool {
    if url.scheme() != "https" {
        return false;
    }
    return match url.host_str() {
        None => false,
        Some(hostname) if hostname == "localhost" => false,
        Some(hostname) => {
            if std::net::IpAddr::from_str(hostname).is_ok() {
                return false;
            }
            true
        }
    };
}
