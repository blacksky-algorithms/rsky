use crate::auth_verifier::{AccessStandard, AuthError, AuthScope, Credentials};
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
    /// A refresh token that was revoked or already rotated past its grace period.
    RefreshTokenRevoked,
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
            ApiError::InvalidToken(message) => json_error(400, "InvalidToken", message, __req),
            ApiError::AuthMissing => json_error(
                401,
                "AuthMissing",
                "Authentication Required".to_string(),
                __req,
            ),
            ApiError::Forbidden(message) => json_error(403, "Forbidden", message, __req),
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
        use crate::apis::com::atproto::repo::RepoUnavailable;
        match value.downcast_ref::<RepoUnavailable>() {
            Some(RepoUnavailable::NotFound(_)) => ApiError::RepoNotFound(value.to_string()),
            Some(RepoUnavailable::Takendown(_)) => ApiError::RepoTakendown(value.to_string()),
            Some(RepoUnavailable::Deactivated(_)) => ApiError::RepoDeactivated(value.to_string()),
            None => ApiError::RuntimeError,
        }
    }
}

#[cfg(test)]
mod tests {
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
