use crate::auth_verifier::scope::{RpcProxy, Scoped};
use crate::auth_verifier::{AuthError, AuthScope, Credentials};
use crate::handle;
use crate::handle::errors::ErrorKind;
use crate::pipethrough::{
    pipethrough_error, pipethrough_procedure, pipethrough_procedure_post, ProxyRequest,
    PRIVILEGED_METHODS,
};
use anyhow::{Error, Result};
use rocket::http::{ContentType, Header, Status};
use rocket::request::FromParam;
use rocket::serde::json::Json;
use rocket::{response, Data, Request, Responder};

#[derive(Responder)]
#[response(status = 200)]
pub struct ProxyResponder(Vec<u8>, Header<'static>, Header<'static>);

#[allow(dead_code)]
pub struct Nsid(String);

impl<'a> FromParam<'a> for Nsid {
    type Error = &'a str;

    fn from_param(param: &'a str) -> Result<Self, Self::Error> {
        // any well-formed method name reaches the proxy, as on the reference
        // PDS; which service answers is decided from the header or the
        // method, not from an allowlist here
        if is_nsid(param) {
            Ok(Nsid(param.to_string()))
        } else {
            Err(param)
        }
    }
}

/// A namespaced identifier: at least three dot-separated segments of
/// letters, digits, and hyphens.
pub fn is_nsid(value: &str) -> bool {
    let segments: Vec<&str> = value.split('.').collect();
    segments.len() >= 3
        && segments.iter().all(|segment| {
            !segment.is_empty()
                && segment
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
}

/// Privileged methods (e.g. chat.bsky.*) must not be reachable with
/// unprivileged app-password credentials.
pub fn assert_valid_token_method(
    nsid: &str,
    credentials: &Option<Credentials>,
) -> Result<(), ApiError> {
    if PRIVILEGED_METHODS.contains(nsid) {
        let privileged = matches!(
            credentials.as_ref().and_then(|c| c.scope.as_ref()),
            Some(AuthScope::Access) | Some(AuthScope::AppPassPrivileged)
        );
        if !privileged {
            return Err(ApiError::BadRequest(
                "InvalidToken".to_string(),
                "Bad token method".to_string(),
            ));
        }
    }
    Ok(())
}

/// The granted scopes an enforcement check must consult, or `None` when the
/// session is not subject to granular scope enforcement at all.
///
/// The gate is the *auth source*, not the scope content: a session carrying
/// `granted_scopes` is an OAuth session and every resource check applies to
/// it, so a session that names no grant for a resource is denied rather than
/// left unrestricted. App passwords and legacy access tokens carry no
/// `granted_scopes` and are unaffected. Gating on "does this session hold a
/// grant of this kind" instead would make absence of a grant mean unlimited
/// access, and would let a permission set that expands to nothing disengage
/// enforcement entirely.
///
/// `transition_exempt` names the resources a legacy `transition:generic`
/// session may reach without a modern grant, mirroring upstream's
/// `ScopePermissionsTransition` (`repo`, `blob`, `rpc` -- but not `identity`,
/// which that class deliberately leaves to the base implementation).
pub(crate) fn scoped_session(
    granted: Option<&Vec<String>>,
    transition_exempt: bool,
) -> Option<crate::oauth_scope::GrantedScopes> {
    let scopes = crate::oauth_scope::GrantedScopes::parse(granted?);
    (!(transition_exempt && scopes.has_transition("generic"))).then_some(scopes)
}

/// Run one resource check behind [`scoped_session`], rendering a refusal as
/// [`ApiError::InsufficientScope`].
fn assert_scope(
    credentials: &Option<Credentials>,
    transition_exempt: bool,
    allows: impl FnOnce(&crate::oauth_scope::GrantedScopes) -> bool,
    denial: impl FnOnce() -> String,
) -> Result<(), ApiError> {
    match scoped_session(
        credentials.as_ref().and_then(|c| c.granted_scopes.as_ref()),
        transition_exempt,
    ) {
        Some(scopes) if !allows(&scopes) => Err(ApiError::InsufficientScope(denial())),
        _ => Ok(()),
    }
}

/// Enforce an OAuth session's `repo:` scope on a record write: the session is
/// confined to the collections and actions its grants name (proposal 0016
/// §Scopes).
pub fn assert_repo_scope(
    credentials: &Option<Credentials>,
    collection: &str,
    action: crate::oauth_scope::RepoAction,
) -> Result<(), ApiError> {
    assert_scope(
        credentials,
        true,
        |scopes| scopes.allows_repo(collection, action),
        || format!("Token scope does not permit {action:?} on {collection}"),
    )
}

/// Enforce an OAuth session's `blob:` scope on a blob upload: the session is
/// confined to the mime patterns its grants accept.
pub fn assert_blob_scope(credentials: &Option<Credentials>, mime: &str) -> Result<(), ApiError> {
    assert_scope(
        credentials,
        true,
        |scopes| scopes.allows_blob(mime),
        || format!("Token scope does not permit uploading blobs of type {mime}"),
    )
}

/// Enforce an OAuth session's `identity:` scope on an identity write (e.g.
/// `com.atproto.identity.updateHandle`).
///
/// Unlike the other resources this takes no `transition:generic` exemption:
/// upstream's `ScopePermissionsTransition` overrides `allowsRepo`,
/// `allowsBlob` and `allowsRpc` but leaves `allowsIdentity` alone, so a
/// legacy transition session must still hold an explicit `identity:` grant.
pub fn assert_identity_scope(
    credentials: &Option<Credentials>,
    attr: &str,
) -> Result<(), ApiError> {
    assert_scope(
        credentials,
        false,
        |scopes| scopes.allows_identity(attr),
        || format!("Token scope does not permit changing identity attribute {attr}"),
    )
}

/// Enforce an OAuth session's `rpc:` scope on an outbound call to a known
/// audience, for callers that resolve the audience themselves rather than
/// through the `atproto-proxy` header.
pub fn assert_rpc_target(
    credentials: &Option<Credentials>,
    lxm: &str,
    aud: &str,
) -> Result<(), ApiError> {
    assert_scope(
        credentials,
        true,
        |scopes| scopes.allows_rpc(lxm, aud),
        || format!("Token scope does not permit calling {lxm} on {aud}"),
    )
}

/// Whether a session may see the account's email address on
/// `com.atproto.server.getSession`.
///
/// Unlike the `assert_*` helpers this degrades the response rather than
/// refusing the request: the endpoint answers for any session and only the
/// address is withheld. The OAuth spec confers it through `transition:email`
/// ("gets included in response to com.atproto.server.getSession") or an
/// explicit `account:email` grant. `transition:generic` is not one of them.
#[must_use]
pub fn allows_email_read(credentials: &Option<Credentials>) -> bool {
    match scoped_session(
        credentials.as_ref().and_then(|c| c.granted_scopes.as_ref()),
        false,
    ) {
        None => true,
        Some(scopes) => {
            scopes.has_transition("email")
                || scopes.allows_account("email", crate::oauth_scope::AccountAction::Read)
        }
    }
}

/// Enforce an OAuth session's `account:` scope on an account-level mutation
/// (email, deactivation/activation, PLC rotation).
///
/// Like `identity:`, this takes no `transition:generic` exemption. The OAuth
/// spec grants that scope "no account management actions: change handle,
/// change email, delete or deactivate account, migrate account", which is
/// exactly the surface these guards cover -- every one of them asks for
/// `Manage`. `transition:email` is unaffected: it confers a `read` on the
/// address, which no caller here requests.
pub fn assert_account_scope(
    credentials: &Option<Credentials>,
    attr: &str,
    action: crate::oauth_scope::AccountAction,
) -> Result<(), ApiError> {
    assert_scope(
        credentials,
        false,
        |scopes| scopes.allows_account(attr, action),
        || format!("Token scope does not permit {action:?} on account attribute {attr}"),
    )
}

/// Subscriptions have no HTTP route in the reference, so a plain request for
/// one falls through to its catch-all like any unknown method.
const SUBSCRIPTIONS: [&str; 1] = ["/xrpc/com.atproto.sync.subscribeRepos"];

/// Passes only requests for methods this server does not serve over HTTP.
/// When it does, its own route has already turned the request down over the
/// query parameters, so the request is a client error and is not proxied on:
/// the reference validates parameters before anything else, authentication
/// included.
pub struct NotServedLocally;

#[rocket::async_trait]
impl<'r> rocket::request::FromRequest<'r> for NotServedLocally {
    type Error = ApiError;

    async fn from_request(req: &'r Request<'_>) -> rocket::request::Outcome<Self, Self::Error> {
        let path = req.uri().path();
        let mut declared = req
            .rocket()
            .routes()
            .filter(|route| route.method == req.method() && route.uri.path() == path.as_str())
            .flat_map(|route| {
                route
                    .uri
                    .as_str()
                    .split_once('?')
                    .map_or("", |(_, q)| q)
                    .split('&')
            })
            .filter_map(|segment| segment.strip_prefix('<')?.strip_suffix('>'))
            .filter(|name| !name.ends_with(".."))
            .peekable();
        if SUBSCRIPTIONS.contains(&path.as_str()) || declared.peek().is_none() {
            return rocket::request::Outcome::Success(Self);
        }
        let present: Vec<&str> = req
            .uri()
            .query()
            .map(|query| query.segments().map(|(name, _)| name).collect())
            .unwrap_or_default();
        let message = match declared.find(|name| !present.contains(name)) {
            Some(name) => format!("Params must have the property \"{name}\""),
            None => "Invalid request parameters".to_string(),
        };
        let error = ApiError::InvalidRequest(message);
        req.local_cache(|| Some(error.clone()));
        rocket::request::Outcome::Error((Status::BadRequest, error))
    }
}

// Lower ranks have higher presidence
#[tracing::instrument(skip_all)]
#[allow(unused_variables)]
#[rocket::get("/xrpc/<nsid>?<query..>", rank = 2)]
pub async fn bsky_api_get_forwarder(
    nsid: Nsid,
    query: Option<&str>,
    _local: NotServedLocally,
    auth: Scoped<RpcProxy>,
    req: ProxyRequest<'_>,
) -> Result<ProxyResponder, ApiError> {
    assert_valid_token_method(&nsid.0, auth.credentials().await?)?;
    let requester: Option<String> = auth.did_opt().await?;
    match pipethrough_procedure::<()>(&req, requester, None).await {
        Ok(res) => {
            let headers = res.headers.expect("Upstream responded without headers.");
            let content_length = match headers.get("content-length") {
                None => Header::new("content-length", res.buffer.len().to_string()),
                Some(val) => Header::new("content-length", val.to_string()),
            };
            let content_type = match headers.get("content-type") {
                None => Header::new("content-type", "octet-stream".to_string()),
                Some(val) => Header::new("Content-Type", val.to_string()),
            };
            Ok(ProxyResponder(res.buffer, content_length, content_type))
        }
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(pipethrough_error(&error))
        }
    }
}

#[rocket::post("/xrpc/<nsid>", data = "<body>", rank = 2)]
pub async fn bsky_api_post_forwarder(
    body: Data<'_>,
    nsid: Nsid,
    auth: Scoped<RpcProxy>,
    req: ProxyRequest<'_>,
) -> Result<ProxyResponder, ApiError> {
    assert_valid_token_method(&nsid.0, auth.credentials().await?)?;
    let requester: Option<String> = auth.did_opt().await?;

    let res = pipethrough_procedure_post(&req, requester, Some(body)).await?;
    let headers = res.headers.expect("Upstream responded without headers.");
    let content_length = match headers.get("content-length") {
        None => Header::new("content-length", res.buffer.len().to_string()),
        Some(val) => Header::new("content-length", val.to_string()),
    };
    let content_type = match headers.get("content-type") {
        None => Header::new("content-type", "application/octet-stream".to_string()),
        Some(val) => Header::new("Content-Type", val.to_string()),
    };
    Ok(ProxyResponder(res.buffer, content_length, content_type))
}

#[derive(Clone, Debug)]
pub enum ApiError {
    RuntimeError,
    InvalidLogin,
    AccountTakendown,
    InvalidRequest(String),
    ExpiredToken,
    /// A refresh token that was revoked or already rotated past its grace period.
    RefreshTokenRevoked,
    /// An email token past its validity window.
    ExpiredEmailToken,
    InvalidToken(String),
    /// No credentials were presented.
    AuthMissing,
    /// The credentials are valid but not accepted by this method.
    Forbidden(String),
    /// A scope-limited token that does not cover the requested write.
    InsufficientScope(String),
    RecordNotFound,
    /// RecordNotFound carrying the requested at-uri in the message.
    RecordNotFoundUri(String),
    InvalidHandle,
    InvalidEmail,
    InvalidPassword,
    InvalidInviteCode,
    HandleNotAvailable,
    EmailNotAvailable,
    UnsupportedDomain,
    UnresolvableDid,
    IncompatibleDidDoc,
    WellKnownNotFound,
    AccountNotFound,
    BlobNotFound,
    BadRequest(String, String),
    AuthRequiredError(String),
    /// The repository does not exist on this server.
    RepoNotFound(String),
    /// The repository exists but has been taken down.
    RepoTakendown(String),
    /// The repository exists but its account is deactivated.
    RepoDeactivated(String),
    /// Error passed through from an upstream service: status code, error, message
    UpstreamResponse(u16, String, String),
    /// This server does not admit writes for the actor right now.
    NotAdmitted(String),
    /// This server serves reads only.
    ReadOnly,
    /// No route answers the path.
    NotFound,
    /// A body larger than the server accepts.
    PayloadTooLarge,
    /// Every slot for this kind of work is taken.
    Overloaded(String),
    /// A fixed-window limit is exhausted; carries the window's status.
    RateLimitExceeded(crate::rate_limits::LimitStatus),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ApiError {}

#[derive(Serialize)]
pub struct ErrorBody {
    error: String,
    message: String,
}

impl<'r, 'o: 'r> ::rocket::response::Responder<'r, 'o> for ApiError {
    fn respond_to(self, __req: &'r Request<'_>) -> response::Result<'o> {
        match self {
            ApiError::RuntimeError => {
                let body = Json(ErrorBody {
                    error: "InternalServerError".to_string(),
                    message: "Something went wrong".to_string(),
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 500u16 });
                Ok(res)
            }
            ApiError::InvalidLogin => json_error(
                401,
                "AuthenticationRequired",
                "Invalid identifier or password".to_string(),
                __req,
            ),
            ApiError::AccountTakendown => json_error(
                401,
                "AccountTakedown",
                "Account has been taken down".to_string(),
                __req,
            ),
            ApiError::RepoNotFound(message) => json_error(400, "RepoNotFound", message, __req),
            ApiError::RepoTakendown(message) => json_error(400, "RepoTakendown", message, __req),
            ApiError::RepoDeactivated(message) => {
                json_error(400, "RepoDeactivated", message, __req)
            }
            ApiError::InvalidRequest(message) => {
                let body = Json(ErrorBody {
                    error: "InvalidRequest".to_string(),
                    message,
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 400u16 });
                Ok(res)
            }
            ApiError::ExpiredToken => {
                json_error(400, "ExpiredToken", "Token has expired".to_string(), __req)
            }
            ApiError::RefreshTokenRevoked => json_error(
                400,
                "ExpiredToken",
                "Token has been revoked".to_string(),
                __req,
            ),
            ApiError::ExpiredEmailToken => {
                json_error(400, "ExpiredToken", "Token is expired".to_string(), __req)
            }
            ApiError::InvalidToken(message) => json_error(400, "InvalidToken", message, __req),
            ApiError::AuthMissing => json_error(
                401,
                "AuthMissing",
                "Authentication Required".to_string(),
                __req,
            ),
            ApiError::Forbidden(message) => json_error(403, "Forbidden", message, __req),
            ApiError::NotAdmitted(message) => {
                let mut res = json_error(503, "NotAdmitted", message, __req)?;
                res.set_header(Header::new("Retry-After", "1"));
                Ok(res)
            }
            ApiError::NotFound => json_error(404, "NotFound", "Not Found".to_string(), __req),
            ApiError::PayloadTooLarge => json_error(
                413,
                "PayloadTooLarge",
                "request entity too large".to_string(),
                __req,
            ),
            ApiError::RateLimitExceeded(status) => {
                let mut res = json_error(
                    429,
                    "RateLimitExceeded",
                    "Rate Limit Exceeded".to_string(),
                    __req,
                )?;
                for header in status.headers() {
                    res.set_header(header);
                }
                Ok(res)
            }
            ApiError::Overloaded(message) => {
                let mut res = json_error(503, "ServiceUnavailable", message, __req)?;
                res.set_header(Header::new("Retry-After", "5"));
                Ok(res)
            }
            ApiError::ReadOnly => json_error(
                503,
                "ReadOnly",
                "this server is serving reads only".to_string(),
                __req,
            ),
            ApiError::InsufficientScope(message) => {
                let body = Json(ErrorBody {
                    error: "InsufficientScope".to_string(),
                    message: message.clone(),
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 403u16 });
                Ok(res)
            }
            ApiError::RecordNotFoundUri(message) => {
                let body = Json(ErrorBody {
                    error: "RecordNotFound".to_string(),
                    message: message.clone(),
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 400u16 });
                Ok(res)
            }
            ApiError::InvalidHandle => {
                let body = Json(ErrorBody {
                    error: "InvalidHandle".to_string(),
                    message: "Handle is invalid".to_string(),
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 400u16 });
                Ok(res)
            }
            ApiError::InvalidEmail => {
                let body = Json(ErrorBody {
                    error: "InvalidEmail".to_string(),
                    message: "invalid email".to_string(),
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 400u16 });
                Ok(res)
            }
            ApiError::InvalidPassword => {
                let body = Json(ErrorBody {
                    error: "InvalidPassword".to_string(),
                    message: "Invalid Password".to_string(),
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 400u16 });
                Ok(res)
            }
            ApiError::InvalidInviteCode => {
                let body = Json(ErrorBody {
                    error: "InvalidInviteCode".to_string(),
                    message: "Invalid invite code".to_string(),
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 400u16 });
                Ok(res)
            }
            ApiError::HandleNotAvailable => {
                let body = Json(ErrorBody {
                    error: "HandleNotAvailable".to_string(),
                    message: "Handle not available".to_string(),
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 400u16 });
                Ok(res)
            }
            ApiError::EmailNotAvailable => {
                let body = Json(ErrorBody {
                    error: "EmailNotAvailable".to_string(),
                    message: "Email not available".to_string(),
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 400u16 });
                Ok(res)
            }
            ApiError::UnsupportedDomain => {
                let body = Json(ErrorBody {
                    error: "UnsupportedDomain".to_string(),
                    message: "Unsupported domain".to_string(),
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 400u16 });
                Ok(res)
            }
            ApiError::UnresolvableDid => {
                let body = Json(ErrorBody {
                    error: "UnresolvableDid".to_string(),
                    message: "Unresolved Did".to_string(),
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 400u16 });
                Ok(res)
            }
            ApiError::IncompatibleDidDoc => {
                let body = Json(ErrorBody {
                    error: "IncompatibleDidDoc".to_string(),
                    message: "IncompatibleDidDoc".to_string(),
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 400u16 });
                Ok(res)
            }
            ApiError::AccountNotFound => {
                let body = Json(ErrorBody {
                    error: "AccountNotFound".to_string(),
                    message: "Account could not be found".to_string(),
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 400u16 });
                Ok(res)
            }
            ApiError::BlobNotFound => {
                let body = Json(ErrorBody {
                    error: "BlobNotFound".to_string(),
                    message: "Blob could not be found".to_string(),
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 400u16 });
                Ok(res)
            }
            ApiError::WellKnownNotFound => {
                let body = Json(ErrorBody {
                    error: "WellKnownNotFound".to_string(),
                    message: "User not found".to_string(),
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(::rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 404u16 });
                Ok(res)
            }
            ApiError::BadRequest(error, message) => {
                let body = Json(ErrorBody { error, message });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 400u16 });
                Ok(res)
            }
            ApiError::AuthRequiredError(message) => {
                json_error(401, "AuthenticationRequired", message, __req)
            }
            ApiError::UpstreamResponse(status, error, message) => {
                let body = Json(ErrorBody { error, message });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: status });
                Ok(res)
            }
            ApiError::RecordNotFound => {
                let body = Json(ErrorBody {
                    error: "RecordNotFound".to_string(),
                    message: "Record could not be found".to_string(),
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                // XRPC maps a named error like RecordNotFound to 400, not 404;
                // 404 is reserved for an unknown route.
                res.set_status(Status { code: 400u16 });
                Ok(res)
            }
        }
    }
}

fn json_error<'r, 'o: 'r>(
    status: u16,
    error: &str,
    message: String,
    req: &'r Request<'_>,
) -> response::Result<'o> {
    let body = Json(ErrorBody {
        error: error.to_string(),
        message,
    });
    let mut res = <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, req)?;
    res.set_header(ContentType(rocket::http::MediaType::const_new(
        "application",
        "json",
        &[],
    )));
    res.set_status(Status { code: status });
    Ok(res)
}

impl From<Error> for ApiError {
    fn from(value: Error) -> Self {
        use crate::account_manager::helpers::account::AccountHelperError;
        use crate::account_manager::helpers::email_token::EmailTokenError;
        use crate::apis::com::atproto::repo::RepoUnavailable;
        use crate::lifecycle::AccountDeleting;
        if let Some(unavailable) = value.downcast_ref::<RepoUnavailable>() {
            return match unavailable {
                RepoUnavailable::NotFound(_) => ApiError::RepoNotFound(value.to_string()),
                RepoUnavailable::Takendown(_) => ApiError::RepoTakendown(value.to_string()),
                RepoUnavailable::Deactivated(_) => ApiError::RepoDeactivated(value.to_string()),
            };
        }
        if let Some(token) = value.downcast_ref::<EmailTokenError>() {
            return match token {
                EmailTokenError::Invalid => ApiError::InvalidToken("Token is invalid".to_string()),
                EmailTokenError::Expired => ApiError::ExpiredEmailToken,
            };
        }
        if value.downcast_ref::<AccountDeleting>().is_some() {
            return ApiError::InvalidRequest(value.to_string());
        }
        if let Some(refused) = value.downcast_ref::<crate::admission::NotAdmitted>() {
            return ApiError::NotAdmitted(refused.to_string());
        }
        if value
            .downcast_ref::<crate::actor_store::ReadOnlyMode>()
            .is_some()
        {
            return ApiError::ReadOnly;
        }
        if let Some(limit) = value.downcast_ref::<crate::actor_store::WriteLimitError>() {
            return ApiError::InvalidRequest(limit.to_string());
        }
        if let Some(mismatch) = value.downcast_ref::<crate::actor_store::blob::BlobMismatch>() {
            return ApiError::InvalidRequest(mismatch.to_string());
        }
        if let Some(AccountHelperError::UserAlreadyExistsError) = value.downcast_ref() {
            return ApiError::InvalidRequest(
                "This email address is already in use, please use a different email.".to_string(),
            );
        }
        tracing::error!(error = ?value, "request failed with an internal error");
        ApiError::RuntimeError
    }
}

#[cfg(test)]
mod error_tests {
    use super::ApiError;
    use crate::apis::com::atproto::repo::RepoUnavailable;

    #[test]
    fn repo_unavailability_keeps_its_reference_error_name() {
        let did = "did:plc:x".to_string();
        for (error, expected) in [
            (
                RepoUnavailable::NotFound(did.clone()),
                ApiError::RepoNotFound("Could not find repo for DID: did:plc:x".to_string()),
            ),
            (
                RepoUnavailable::Takendown(did.clone()),
                ApiError::RepoTakendown("Repo has been takendown: did:plc:x".to_string()),
            ),
            (
                RepoUnavailable::Deactivated(did),
                ApiError::RepoDeactivated("Repo has been deactivated: did:plc:x".to_string()),
            ),
        ] {
            let converted: ApiError = anyhow::Error::from(error).into();
            assert_eq!(format!("{converted:?}"), format!("{expected:?}"));
        }
        let other: ApiError = anyhow::anyhow!("disk on fire").into();
        assert!(matches!(other, ApiError::RuntimeError));
        let read_only: ApiError = anyhow::Error::from(crate::actor_store::ReadOnlyMode).into();
        assert!(matches!(read_only, ApiError::ReadOnly));
    }

    #[test]
    fn any_well_formed_method_reaches_the_proxy() {
        use super::{is_nsid, Nsid};
        use rocket::request::FromParam;
        for method in [
            "app.bsky.feed.getTimeline",
            "chat.bsky.convo.listConvos",
            "tools.ozone.moderation.queryStatuses",
            "com.atproto.moderation.createReport",
            "community.blacksky.pds.getConvergence",
            "xyz.some-vendor.thing",
        ] {
            assert!(is_nsid(method), "{method}");
            assert_eq!(Nsid::from_param(method).unwrap().0, method);
        }
        for junk in [
            "",
            "app.bsky",
            "app..bsky",
            "a.b.c/d",
            "app.bsky.feed.get timeline",
        ] {
            assert!(!is_nsid(junk), "{junk:?}");
            assert!(Nsid::from_param(junk).is_err(), "{junk:?}");
        }
    }
}

/// Renders an [`AuthError`] as its wire-facing [`ApiError`].
///
/// This is the single place auth guards translate a verification failure into
/// the rendered error body, using the reference PDS's names: an expired
/// session is `ExpiredToken` so clients know to refresh, a token that fails
/// verification or scope is `InvalidToken`, and no credentials is `AuthMissing`.
impl From<&AuthError> for ApiError {
    fn from(error: &AuthError) -> Self {
        let rendered = error.to_string();
        crate::metrics::METRICS.auth_failure(rendered.split(':').next().unwrap_or("unknown"));
        match error {
            AuthError::ExpiredToken => ApiError::ExpiredToken,
            AuthError::AuthMissing => ApiError::AuthMissing,
            AuthError::OAuth(code, description) => {
                ApiError::UpstreamResponse(401, code.clone(), description.clone())
            }
            AuthError::Forbidden(message) => ApiError::Forbidden(message.clone()),
            AuthError::BadJwt(message) => ApiError::InvalidToken(message.clone()),
            // A revoked credential, or one from an untrusted issuer or for
            // the wrong audience, is an authentication failure and surfaces
            // as 401.
            AuthError::AuthRequired(_)
            | AuthError::BadJwtAudience(_)
            | AuthError::UntrustedIss(_) => ApiError::AuthRequiredError(error.to_string()),
            other => ApiError::InvalidRequest(other.to_string()),
        }
    }
}

impl From<handle::errors::Error> for ApiError {
    fn from(value: handle::errors::Error) -> Self {
        match value.kind {
            ErrorKind::InvalidHandle => ApiError::InvalidHandle,
            ErrorKind::HandleNotAvailable => ApiError::HandleNotAvailable,
            ErrorKind::UnsupportedDomain => ApiError::UnsupportedDomain,
            ErrorKind::InternalError => ApiError::RuntimeError,
        }
    }
}

pub mod app;
pub mod com;
pub mod community;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oauth_scope::{AccountAction, RepoAction};

    const POST: &str = "app.bsky.feed.post";

    fn creds(granted: &[&str]) -> Option<Credentials> {
        Some(Credentials {
            r#type: "oauth".to_string(),
            did: Some("did:plc:test".to_string()),
            scope: Some(AuthScope::AppPass),
            granted_scopes: Some(granted.iter().map(|s| s.to_string()).collect()),
            audience: None,
            token_id: None,
            aud: None,
            iss: None,
            is_privileged: None,
        })
    }

    /// The four synchronous resource checks, plus the gate + `allows_rpc`
    /// pair that `pipethrough::assert_rpc_scope` performs once it has
    /// resolved an audience (that helper needs a live `ProxyRequest`, so its
    /// scope decision is exercised here rather than through the route).
    fn denials(credentials: &Option<Credentials>) -> [bool; 5] {
        let rpc_denied = match scoped_session(
            credentials.as_ref().and_then(|c| c.granted_scopes.as_ref()),
            true,
        ) {
            Some(scopes) => !scopes.allows_rpc("app.bsky.feed.getTimeline", "did:web:api.example"),
            None => false,
        };
        [
            assert_repo_scope(credentials, POST, RepoAction::Create).is_err(),
            assert_blob_scope(credentials, "image/png").is_err(),
            rpc_denied,
            assert_identity_scope(credentials, "handle").is_err(),
            assert_account_scope(credentials, "email", AccountAction::Manage).is_err(),
        ]
    }

    #[test]
    fn blob_scope_allows_and_denies_by_mime() {
        let allowed = creds(&["atproto", "blob:image/*"]);
        assert!(assert_blob_scope(&allowed, "image/png").is_ok());
        assert!(matches!(
            assert_blob_scope(&allowed, "video/mp4"),
            Err(ApiError::InsufficientScope(_))
        ));

        // No `blob:` grant at all. Enforcement is gated on the auth source,
        // not on whether the session happens to hold a grant of this kind, so
        // absence of the grant is a denial.
        let no_blob_grant = creds(&["atproto", "repo:app.bsky.feed.post"]);
        assert!(matches!(
            assert_blob_scope(&no_blob_grant, "video/mp4"),
            Err(ApiError::InsufficientScope(_))
        ));

        // Legacy app-password sessions carry no `granted_scopes` at all.
        assert!(assert_blob_scope(&None, "video/mp4").is_ok());
    }

    #[test]
    fn identity_scope_allows_and_denies_by_attribute() {
        let allowed = creds(&["atproto", "identity:handle"]);
        assert!(assert_identity_scope(&allowed, "handle").is_ok());

        let wrong_attr = creds(&["atproto", "identity:invalid"]);
        assert!(matches!(
            assert_identity_scope(&wrong_attr, "handle"),
            Err(ApiError::InsufficientScope(_))
        ));

        let no_identity_grant = creds(&["atproto", "repo:app.bsky.feed.post"]);
        assert!(matches!(
            assert_identity_scope(&no_identity_grant, "handle"),
            Err(ApiError::InsufficientScope(_))
        ));
    }

    #[test]
    fn account_scope_allows_and_denies_by_attribute_and_action() {
        let manage = creds(&["atproto", "account:email?action=manage"]);
        assert!(assert_account_scope(&manage, "email", AccountAction::Manage).is_ok());

        let read_only = creds(&["atproto", "account:email"]);
        assert!(matches!(
            assert_account_scope(&read_only, "email", AccountAction::Manage),
            Err(ApiError::InsufficientScope(_))
        ));

        let wrong_attr = creds(&["atproto", "account:status?action=manage"]);
        assert!(matches!(
            assert_account_scope(&wrong_attr, "email", AccountAction::Manage),
            Err(ApiError::InsufficientScope(_))
        ));

        let no_account_grant = creds(&["atproto", "repo:app.bsky.feed.post"]);
        assert!(matches!(
            assert_account_scope(&no_account_grant, "email", AccountAction::Manage),
            Err(ApiError::InsufficientScope(_))
        ));
    }

    /// The original defect: a granular session holding one `repo:` grant and
    /// nothing else got *unrestricted* access to every other resource,
    /// because each check gated on whether the session held a grant of that
    /// same kind.
    #[test]
    fn a_repo_only_grant_confers_nothing_on_other_resources() {
        let repo_only = creds(&["atproto", "repo:app.bsky.feed.post"]);
        assert!(assert_repo_scope(&repo_only, POST, RepoAction::Create).is_ok());
        assert_eq!(denials(&repo_only), [false, true, true, true, true]);
    }

    /// A session granted the base scope and nothing else can do nothing.
    #[test]
    fn an_empty_scope_set_is_denied_every_resource() {
        assert_eq!(denials(&creds(&["atproto"])), [true; 5]);
    }

    /// `include:` scopes reach these helpers already expanded
    /// (`permission_set::expand_includes`). One that resolved to nothing --
    /// an unreachable authority, or a set naming no permissions -- must leave
    /// the session with no grants, not with every grant.
    #[test]
    fn an_include_that_resolved_to_nothing_is_denied_every_resource() {
        assert_eq!(
            denials(&creds(&["atproto", "include:app.example.nothing"])),
            [true; 5]
        );
    }

    /// An unparseable scope string is inert: it can never be the reason a
    /// session gets access it was not granted.
    #[test]
    fn an_unparseable_scope_widens_nothing() {
        assert_eq!(
            denials(&creds(&["atproto", "not-a-scope-this-server-knows"])),
            [true; 5]
        );
    }

    /// Backward compatibility for legacy `transition:generic` sessions,
    /// following upstream's `ScopePermissionsTransition`: it overrides
    /// `allowsRepo`, `allowsBlob` and `allowsRpc`, but not `allowsIdentity`,
    /// so a transition session still needs an explicit `identity:` grant to
    /// change its handle.
    #[test]
    fn a_transition_generic_session_keeps_its_legacy_reach() {
        // Repo writes, blob uploads and service proxying, per the OAuth spec's
        // definition of the scope -- and no account management: not the handle,
        // not the email, not deactivation, not migration.
        let transition = creds(&["atproto", "transition:generic"]);
        assert_eq!(denials(&transition), [false, false, false, true, true]);

        // Explicit grants alongside it still work.
        let with_identity = creds(&["atproto", "transition:generic", "identity:handle"]);
        assert!(assert_identity_scope(&with_identity, "handle").is_ok());
        let with_account = creds(&[
            "atproto",
            "transition:generic",
            "account:email?action=manage",
        ]);
        assert!(assert_account_scope(&with_account, "email", AccountAction::Manage).is_ok());
    }

    /// `registerPush` and `unregisterPush` mint service auth and call the
    /// notification service, so they need the `rpc:` grant that governs
    /// reaching outward -- against the audience the request body names.
    #[test]
    fn an_rpc_target_is_checked_against_the_audience_the_body_names() {
        let lxm = "app.bsky.notification.registerPush";
        let aud = "did:web:notif.example.com";

        // The matching grant permits it; a wildcard audience does too.
        let exact = creds(&["atproto", &format!("rpc:{lxm}?aud={aud}")]);
        assert!(assert_rpc_target(&exact, lxm, aud).is_ok());
        let wildcard = creds(&["atproto", &format!("rpc:{lxm}?aud=*")]);
        assert!(assert_rpc_target(&wildcard, lxm, aud).is_ok());

        // A grant for a different audience does not.
        let other_aud = creds(&[
            "atproto",
            &format!("rpc:{lxm}?aud=did:web:someone-else.example.com"),
        ]);
        assert!(assert_rpc_target(&other_aud, lxm, aud).is_err());

        // Nor does a grant for a different method, nor no rpc grant at all.
        let other_lxm = creds(&[
            "atproto",
            &format!("rpc:app.bsky.feed.getTimeline?aud={aud}"),
        ]);
        assert!(assert_rpc_target(&other_lxm, lxm, aud).is_err());
        assert!(
            assert_rpc_target(&creds(&["atproto", "repo:app.bsky.feed.post"]), lxm, aud).is_err()
        );

        // Legacy sessions are unaffected, as everywhere else.
        assert!(assert_rpc_target(&creds(&["atproto", "transition:generic"]), lxm, aud).is_ok());
        assert!(assert_rpc_target(&None, lxm, aud).is_ok());
    }

    /// The OAuth spec confers the address through `transition:email` or an
    /// explicit `account:email` grant, and through nothing else.
    #[test]
    fn email_is_visible_only_to_a_session_granted_it() {
        // No granted scopes at all: app password / legacy token, unaffected.
        assert!(allows_email_read(&None));

        // The two grants the spec names.
        assert!(allows_email_read(&creds(&["atproto", "transition:email"])));
        assert!(allows_email_read(&creds(&["atproto", "account:email"])));
        assert!(allows_email_read(&creds(&[
            "atproto",
            "account:email?action=read"
        ])));
        // `manage` subsumes `read`.
        assert!(allows_email_read(&creds(&[
            "atproto",
            "account:email?action=manage"
        ])));

        // Everything else is withheld, transition:generic included.
        assert!(!allows_email_read(&creds(&["atproto"])));
        assert!(!allows_email_read(&creds(&[
            "atproto",
            "transition:generic"
        ])));
        assert!(!allows_email_read(&creds(&[
            "atproto",
            "repo:app.bsky.feed.post"
        ])));
        assert!(!allows_email_read(&creds(&["atproto", "account:status"])));
        assert!(!allows_email_read(&creds(&[
            "atproto",
            "include:app.example.set"
        ])));
    }

    /// App passwords and legacy access tokens carry no `granted_scopes`;
    /// nothing here applies to them.
    #[test]
    fn a_session_without_granted_scopes_is_unaffected() {
        assert_eq!(denials(&None), [false; 5]);
    }

    #[rocket::get("/xrpc/com.example.thing?<count>")]
    fn thing(count: u8) -> String {
        count.to_string()
    }

    #[rocket::get("/xrpc/<_nsid>?<_query..>", rank = 2)]
    fn proxied(_nsid: &str, _query: Option<&str>, _local: NotServedLocally) -> &'static str {
        "proxied"
    }

    async fn answer(client: &rocket::local::asynchronous::Client, path: &str) -> (Status, String) {
        let response = client.get(path).dispatch().await;
        let status = response.status();
        (status, response.into_string().await.unwrap_or_default())
    }

    #[tokio::test]
    async fn only_methods_served_elsewhere_reach_the_proxy() {
        let rocket = rocket::build()
            .mount("/", rocket::routes![thing, proxied])
            .register("/", rocket::catchers![crate::default_catcher]);
        let client = rocket::local::asynchronous::Client::untracked(rocket)
            .await
            .unwrap();

        let (status, text) = answer(&client, "/xrpc/com.example.other?x=1").await;
        assert_eq!((status, text.as_str()), (Status::Ok, "proxied"));
        let (status, text) = answer(&client, "/xrpc/com.example.thing?count=7").await;
        assert_eq!((status, text.as_str()), (Status::Ok, "7"));

        let (status, text) = answer(&client, "/xrpc/com.example.thing").await;
        assert_eq!(status, Status::BadRequest);
        assert!(
            text.contains("Params must have the property \\\"count\\\""),
            "{text}"
        );
        let (status, text) = answer(&client, "/xrpc/com.example.thing?count=many").await;
        assert_eq!(status, Status::BadRequest);
        assert!(text.contains("Invalid request parameters"), "{text}");
    }
}
