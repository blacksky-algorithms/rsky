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

/// Enforce a granular OAuth session's `repo:` scope on a record write.
///
/// A session that carries `repo:` grants but no `transition:generic` is
/// confined to the collections and actions those grants name (proposal 0016
/// §Scopes). Legacy transition sessions and app passwords carry no `repo:`
/// grant and are unaffected. Without this a token scoped to one collection
/// can write any collection.
pub fn assert_repo_scope(
    credentials: &Option<Credentials>,
    collection: &str,
    action: crate::oauth_scope::RepoAction,
) -> Result<(), ApiError> {
    let Some(granted) = credentials.as_ref().and_then(|c| c.granted_scopes.as_ref()) else {
        return Ok(());
    };
    let scopes = crate::oauth_scope::GrantedScopes::parse(granted);
    if !scopes.is_granular_repo_session() {
        return Ok(());
    }
    if scopes.allows_repo(collection, action) {
        Ok(())
    } else {
        Err(ApiError::InsufficientScope(format!(
            "Token scope does not permit {action:?} on {collection}"
        )))
    }
}

/// Enforce a granular OAuth session's `blob:` scope on a blob upload.
///
/// Same shape as [`assert_repo_scope`]: a session carrying `blob:` grants but
/// no `transition:generic` is confined to the accepted mime patterns those
/// grants name. Legacy transition sessions and app passwords carry no
/// `blob:` grant and are unaffected.
pub fn assert_blob_scope(credentials: &Option<Credentials>, mime: &str) -> Result<(), ApiError> {
    let Some(granted) = credentials.as_ref().and_then(|c| c.granted_scopes.as_ref()) else {
        return Ok(());
    };
    let scopes = crate::oauth_scope::GrantedScopes::parse(granted);
    if !scopes.is_granular_blob_session() {
        return Ok(());
    }
    if scopes.allows_blob(mime) {
        Ok(())
    } else {
        Err(ApiError::InsufficientScope(format!(
            "Token scope does not permit uploading blobs of type {mime}"
        )))
    }
}

/// Enforce a granular OAuth session's `identity:` scope on an identity write
/// (e.g. `com.atproto.identity.updateHandle`).
///
/// Same shape as [`assert_repo_scope`]: a session carrying `identity:`
/// grants but no `transition:generic` is confined to the attributes those
/// grants name.
pub fn assert_identity_scope(
    credentials: &Option<Credentials>,
    attr: &str,
) -> Result<(), ApiError> {
    let Some(granted) = credentials.as_ref().and_then(|c| c.granted_scopes.as_ref()) else {
        return Ok(());
    };
    let scopes = crate::oauth_scope::GrantedScopes::parse(granted);
    if !scopes.is_granular_identity_session() {
        return Ok(());
    }
    if scopes.allows_identity(attr) {
        Ok(())
    } else {
        Err(ApiError::InsufficientScope(format!(
            "Token scope does not permit changing identity attribute {attr}"
        )))
    }
}

/// Enforce a granular OAuth session's `account:` scope on an account-level
/// mutation (email, deactivation/activation, PLC rotation).
///
/// Same shape as [`assert_repo_scope`]: a session carrying `account:` grants
/// but no `transition:generic` is confined to the attribute/action pairs
/// those grants name.
pub fn assert_account_scope(
    credentials: &Option<Credentials>,
    attr: &str,
    action: crate::oauth_scope::AccountAction,
) -> Result<(), ApiError> {
    let Some(granted) = credentials.as_ref().and_then(|c| c.granted_scopes.as_ref()) else {
        return Ok(());
    };
    let scopes = crate::oauth_scope::GrantedScopes::parse(granted);
    if !scopes.is_granular_account_session() {
        return Ok(());
    }
    if scopes.allows_account(attr, action) {
        Ok(())
    } else {
        Err(ApiError::InsufficientScope(format!(
            "Token scope does not permit {action:?} on account attribute {attr}"
        )))
    }
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

    #[test]
    fn blob_scope_allows_and_denies_by_mime() {
        let allowed = creds(&["atproto", "blob:image/*"]);
        assert!(assert_blob_scope(&allowed, "image/png").is_ok());
        assert!(matches!(
            assert_blob_scope(&allowed, "video/mp4"),
            Err(ApiError::InsufficientScope(_))
        ));

        // No `blob:` grant at all: enforcement is opt-in per resource, so
        // this passes through unrestricted (same as `assert_repo_scope`).
        let no_blob_grant = creds(&["atproto", "repo:app.bsky.feed.post"]);
        assert!(assert_blob_scope(&no_blob_grant, "video/mp4").is_ok());

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
        assert!(assert_identity_scope(&no_identity_grant, "handle").is_ok());
    }

    #[test]
    fn account_scope_allows_and_denies_by_attribute_and_action() {
        let manage = creds(&["atproto", "account:email?action=manage"]);
        assert!(
            assert_account_scope(&manage, "email", crate::oauth_scope::AccountAction::Manage)
                .is_ok()
        );

        let read_only = creds(&["atproto", "account:email"]);
        assert!(matches!(
            assert_account_scope(
                &read_only,
                "email",
                crate::oauth_scope::AccountAction::Manage
            ),
            Err(ApiError::InsufficientScope(_))
        ));

        let wrong_attr = creds(&["atproto", "account:status?action=manage"]);
        assert!(matches!(
            assert_account_scope(
                &wrong_attr,
                "email",
                crate::oauth_scope::AccountAction::Manage
            ),
            Err(ApiError::InsufficientScope(_))
        ));

        let no_account_grant = creds(&["atproto", "repo:app.bsky.feed.post"]);
        assert!(assert_account_scope(
            &no_account_grant,
            "email",
            crate::oauth_scope::AccountAction::Manage
        )
        .is_ok());
    }
}
