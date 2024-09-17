use crate::auth_verifier::{AccessOutput, AccessStandard};
use crate::common::{get_service_endpoint, GetServiceEndpointOpts};
use crate::config::{ServerConfig, ServiceConfig};
use crate::repo::types::Ids;
use crate::xrpc_server::types::{HandlerPipeThrough, InvalidRequestError, XRPCError};
use crate::{context, SharedIdResolver, APP_USER_AGENT};
use anyhow::{bail, Result};
use lazy_static::lazy_static;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use reqwest::{Client, RequestBuilder, Response};
use rocket::http::{Method, Status};
use rocket::request::{FromRequest, Outcome, Request};
use rocket::State;
use serde::de::DeserializeOwned;
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, HashSet};
use std::str::FromStr;
use std::time::Duration;
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
                let headers = req.headers().clone().into_iter().fold(
                    BTreeMap::new(),
                    |mut acc: BTreeMap<String, String>, cur| {
                        let _ = acc.insert(cur.name().to_string(), cur.value().to_string());
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
                    id_resolver: req.guard::<&State<SharedIdResolver>>().await.unwrap(),
                    cfg: req.guard::<&State<ServerConfig>>().await.unwrap(),
                };
                match pipethrough(
                    &req,
                    requester,
                    OverrideOpts {
                        aud: None,
                        lxm: None,
                    },
                )
                .await
                {
                    Ok(res) => Outcome::Success(res),
                    Err(error) => match error.downcast_ref() {
                        Some(InvalidRequestError::XRPCError(xrpc)) => {
                            if let XRPCError::FailedResponse {
                                status,
                                error,
                                message,
                                headers,
                            } = xrpc
                            {
                                eprintln!("@LOG: XRPC ERROR Status:{status}; Message: {message:?}; Error: {error:?}; Headers: {headers:?}");
                            }
                            Outcome::Error((Status::BadRequest, error))
                        }
                        _ => Outcome::Error((Status::BadRequest, error)),
                    },
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
            query: match req.uri().query() {
                None => None,
                Some(query) => Some(query.to_string()),
            },
            path: req.uri().path().to_string(),
            method: req.method(),
            id_resolver: req.guard::<&State<SharedIdResolver>>().await.unwrap(),
            cfg: req.guard::<&State<ServerConfig>>().await.unwrap(),
        })
    }
}

pub async fn pipethrough<'r>(
    req: &'r ProxyRequest<'_>,
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

pub async fn pipethrough_procedure<'r, T: serde::Serialize>(
    req: &'r ProxyRequest<'_>,
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

// Request setup/formatting
// -------------------

const REQ_HEADERS_TO_FORWARD: [&'static str; 4] = [
    "accept-language",
    "content-type",
    "atproto-accept-labelers",
    "x-bsky-topics",
];

pub async fn format_url_and_aud<'r>(
    req: &'r ProxyRequest<'_>,
    aud_override: Option<String>,
) -> Result<UrlAndAud> {
    let proxy_to = parse_proxy_header(req).await?;
    let nsid = parse_req_nsid(req);
    let default_proxy = default_service(req, &nsid).await;
    let service_url = match proxy_to {
        Some(ref proxy_to) => {
            println!(
                "@LOG: format_url_and_aud() proxy_to: {:?}",
                proxy_to.service_url
            );
            Some(proxy_to.service_url.clone())
        }
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
            let mut url = Url::parse(format!("{0}{1}", service_url, req.path).as_str())?;
            if let Some(ref params) = req.query {
                url.set_query(Some(params.as_str()));
            }
            if !req.cfg.service.dev_mode && !is_safe_url(url.clone()) {
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

pub async fn format_headers<'r>(
    req: &'r ProxyRequest<'_>,
    aud: String,
    lxm: String,
    requester: Option<String>,
) -> Result<HeaderMap> {
    let mut headers: HeaderMap = match requester {
        Some(requester) => context::service_auth_headers(&requester, &aud, &lxm).await?,
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

pub fn format_req_init(
    req: &ProxyRequest,
    url: Url,
    headers: HeaderMap,
    body: Option<Vec<u8>>,
) -> Result<RequestBuilder> {
    match req.method {
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

pub async fn parse_proxy_header<'r>(req: &'r ProxyRequest<'_>) -> Result<Option<ProxyHeader>> {
    let headers = &req.headers;
    let proxy_to: Option<&String> = headers.get("atproto-proxy");
    match proxy_to {
        None => Ok(None),
        Some(proxy_to) => {
            let parts: Vec<&str> = proxy_to.split("#").collect::<Vec<&str>>();
            match (parts.get(0), parts.get(1), parts.get(2)) {
                (Some(did), Some(service_id), None) => {
                    let did = did.to_string();
                    let id_resolver = req.id_resolver;
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

pub async fn make_request(req_init: RequestBuilder) -> Result<Response> {
    let res = req_init.send().await;
    match res {
        Err(e) => {
            println!("@LOG WARN: pipethrough network error {}", e.to_string());
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
                    error: match error_body["error"].as_str() {
                        None => None,
                        Some(error_body_error) => Some(error_body_error.to_string()),
                    },
                    message: match error_body["message"].as_str() {
                        None => None,
                        Some(error_body_message) => Some(error_body_message.to_string()),
                    }
                }))
            }
        },
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

pub async fn default_service<'r>(
    req: &'r ProxyRequest<'_>,
    nsid: &String,
) -> Option<ServiceConfig> {
    let cfg = req.cfg;
    match Ids::from_str(nsid) {
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
