use crate::auth_verifier::{AccessStandard, AuthError, AuthScope, Credentials};
use crate::handle;
use crate::handle::errors::ErrorKind;
use crate::pipethrough::{
    assert_rpc_scope, pipethrough_error, pipethrough_procedure, pipethrough_procedure_post,
    ProxyRequest, PRIVILEGED_METHODS,
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
        // This is how we make sure we allowlist lexicons and what gets proxied
        if param.starts_with("app.bsky.")
            || param.starts_with("chat.bsky")
            || param.starts_with("community.blacksky.")
        {
            Ok(Nsid(param.to_string()))
        } else {
            Err(param)
        }
    }
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

// Lower ranks have higher presidence
#[tracing::instrument(skip_all)]
#[allow(unused_variables)]
#[rocket::get("/xrpc/<nsid>?<query..>", rank = 2)]
pub async fn bsky_api_get_forwarder(
    nsid: Nsid,
    query: Option<&str>,
    auth: AccessStandard,
    req: ProxyRequest<'_>,
) -> Result<ProxyResponder, ApiError> {
    assert_valid_token_method(&nsid.0, &auth.access.credentials)?;
    let granted_scopes = auth
        .access
        .credentials
        .as_ref()
        .and_then(|c| c.granted_scopes.clone());
    assert_rpc_scope(&granted_scopes, &req).await?;
    let requester: Option<String> = match auth.access.credentials {
        None => None,
        Some(credentials) => credentials.did,
    };
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
    auth: AccessStandard,
    req: ProxyRequest<'_>,
) -> Result<ProxyResponder, ApiError> {
    assert_valid_token_method(&nsid.0, &auth.access.credentials)?;
    let granted_scopes = auth
        .access
        .credentials
        .as_ref()
        .and_then(|c| c.granted_scopes.clone());
    assert_rpc_scope(&granted_scopes, &req).await?;
    let requester: Option<String> = match auth.access.credentials {
        None => None,
        Some(credentials) => credentials.did,
    };

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
    InvalidToken,
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
    /// Error passed through from an upstream service: status code, error, message
    UpstreamResponse(u16, String, String),
}

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
            ApiError::InvalidLogin => {
                let body = Json(ErrorBody {
                    error: "InvalidLogin".to_string(),
                    message: "Invalid identifier or password".to_string(),
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(::rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 400u16 });
                Ok(res)
            }
            ApiError::AccountTakendown => {
                let body = Json(ErrorBody {
                    error: "AccountTakendown".to_string(),
                    message: "Account has been taken down".to_string(),
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(::rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 400u16 });
                Ok(res)
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
                let body = Json(ErrorBody {
                    error: "ExpiredToken".to_string(),
                    message: "Token is expired".to_string(),
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
            ApiError::InvalidToken => {
                let body = Json(ErrorBody {
                    error: "InvalidToken".to_string(),
                    message: "Token is invalid".to_string(),
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
                    message: "Invalid email".to_string(),
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
                let body = Json(ErrorBody {
                    error: "AuthRequiredError".to_string(),
                    message,
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(::rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 401u16 });
                Ok(res)
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

impl From<Error> for ApiError {
    fn from(_value: Error) -> Self {
        ApiError::RuntimeError
    }
}

/// Renders an [`AuthError`] as its wire-facing [`ApiError`].
///
/// This is the single place auth guards translate a verification failure into
/// the rendered error body. Previously every guard hardcoded `InvalidRequest`,
/// which made an expired token indistinguishable from a malformed one; routing
/// through here surfaces `ExpiredToken` so clients know to refresh, while every
/// other case keeps its historical `InvalidRequest` rendering unchanged.
impl From<&AuthError> for ApiError {
    fn from(error: &AuthError) -> Self {
        match error {
            AuthError::ExpiredToken => ApiError::ExpiredToken,
            // A missing or revoked credential, or one from an untrusted
            // issuer or for the wrong audience, is an authentication failure
            // and surfaces as 401. A malformed token (`BadJwt`) stays a 400
            // client error so the two remain distinguishable.
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

    /// App passwords and legacy access tokens carry no `granted_scopes`;
    /// nothing here applies to them.
    #[test]
    fn a_session_without_granted_scopes_is_unaffected() {
        assert_eq!(denials(&None), [false; 5]);
    }
}
