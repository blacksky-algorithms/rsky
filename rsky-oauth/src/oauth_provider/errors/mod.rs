use crate::oauth_types::{OAuthAuthorizationRequestParameters, OAuthTokenType};
use rocket::http::{ContentType, Status};
use rocket::serde::json::Json;
use rocket::{response, Request};
use serde::Serialize;

#[derive(Debug, Clone)]
pub enum OAuthError {
    InvalidGrantError(String),
    InvalidRequestError(String),
    InvalidClientError(String),
    InvalidClientMetadataError(String),
    InvalidRedirectUriError(String),
    InvalidParametersError(OAuthAuthorizationRequestParameters, String),
    UnauthorizedClientError(String),
    InvalidTokenError(OAuthTokenType, String),
    InvalidDpopKeyBindingError,
    InvalidDpopProofError(String),
    RuntimeError(String),
    AccessDeniedError(OAuthAuthorizationRequestParameters, String),
    InvalidClientAuthMethod(String),
    AccountSelectionRequiredError,
    LoginRequiredError,
    ConsentRequiredError(OAuthAuthorizationRequestParameters, String),
    InvalidAuthorizationDetailsError(String),
    InvalidScopeError(OAuthAuthorizationRequestParameters, String),
    JwtVerifyError(String),
}

#[derive(Serialize)]
pub struct ErrorBody {
    error: String,
    message: String,
}

impl<'r, 'o: 'r> ::rocket::response::Responder<'r, 'o> for OAuthError {
    fn respond_to(self, __req: &'r Request<'_>) -> response::Result<'o> {
        match self {
            OAuthError::InvalidGrantError(error) => {
                let body = Json(ErrorBody {
                    error: "InvalidGrantError".to_string(),
                    message: error,
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 401u16 });
                Ok(res)
            }
            OAuthError::InvalidRequestError(error) => {
                let body = Json(ErrorBody {
                    error: "InvalidRequestError".to_string(),
                    message: error,
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
            OAuthError::InvalidClientError(error) => {
                let body = Json(ErrorBody {
                    error: "InvalidClientError".to_string(),
                    message: error,
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 401u16 });
                Ok(res)
            }
            OAuthError::InvalidClientMetadataError(error) => {
                let body = Json(ErrorBody {
                    error: "InvalidClientMetadataError".to_string(),
                    message: error,
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 401u16 });
                Ok(res)
            }
            OAuthError::InvalidRedirectUriError(error) => {
                let body = Json(ErrorBody {
                    error: "InvalidRedirectUriError".to_string(),
                    message: error,
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 401u16 });
                Ok(res)
            }
            OAuthError::InvalidParametersError(parameters, error) => {
                let body = Json(ErrorBody {
                    error: "InvalidParametersError".to_string(),
                    message: error,
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 401u16 });
                Ok(res)
            }
            OAuthError::UnauthorizedClientError(error) => {
                let body = Json(ErrorBody {
                    error: "UnauthorizedClientError".to_string(),
                    message: error,
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 401u16 });
                Ok(res)
            }
            OAuthError::InvalidTokenError(token_type, message) => {
                let body = Json(ErrorBody {
                    error: "InvalidTokenError".to_string(),
                    message: message,
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 401u16 });
                Ok(res)
            }
            OAuthError::InvalidDpopKeyBindingError => {
                let body = Json(ErrorBody {
                    error: "InvalidDpopKeyBindingError".to_string(),
                    message: "Invalid Dpop Key Binding Error".to_string(),
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 401u16 });
                Ok(res)
            }
            OAuthError::InvalidDpopProofError(error) => {
                let body = Json(ErrorBody {
                    error: "InvalidDpopProofError".to_string(),
                    message: error,
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 401u16 });
                Ok(res)
            }
            OAuthError::RuntimeError(error) => {
                let body = Json(ErrorBody {
                    error: "RuntimeError".to_string(),
                    message: error,
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 401u16 });
                Ok(res)
            }
            OAuthError::AccessDeniedError(parameters, error) => {
                let body = Json(ErrorBody {
                    error: "AccessDeniedError".to_string(),
                    message: error,
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 401u16 });
                Ok(res)
            }
            OAuthError::InvalidClientAuthMethod(error) => {
                let body = Json(ErrorBody {
                    error: "InvalidClientAuthMethod".to_string(),
                    message: error,
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 401u16 });
                Ok(res)
            }
            OAuthError::AccountSelectionRequiredError => {
                let body = Json(ErrorBody {
                    error: "AccountSelectionRequiredError".to_string(),
                    message: "AccountSelectionRequiredError".to_string(),
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 401u16 });
                Ok(res)
            }
            OAuthError::LoginRequiredError => {
                let body = Json(ErrorBody {
                    error: "LoginRequiredError".to_string(),
                    message: "LoginRequiredError".to_string(),
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 401u16 });
                Ok(res)
            }
            OAuthError::ConsentRequiredError(parameters, error) => {
                let body = Json(ErrorBody {
                    error: "ConsentRequiredError".to_string(),
                    message: error,
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 401u16 });
                Ok(res)
            }
            OAuthError::InvalidAuthorizationDetailsError(error) => {
                let body = Json(ErrorBody {
                    error: "InvalidAuthorizationDetailsError".to_string(),
                    message: error,
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 401u16 });
                Ok(res)
            }
            OAuthError::InvalidScopeError(parameters, error) => {
                let body = Json(ErrorBody {
                    error: "InvalidScopeError".to_string(),
                    message: error,
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 401u16 });
                Ok(res)
            }
            OAuthError::JwtVerifyError(error) => {
                let body = Json(ErrorBody {
                    error: "JwtVerifyError".to_string(),
                    message: error,
                });
                let mut res =
                    <Json<ErrorBody> as ::rocket::response::Responder>::respond_to(body, __req)?;
                res.set_header(ContentType(rocket::http::MediaType::const_new(
                    "application",
                    "json",
                    &[],
                )));
                res.set_status(Status { code: 401u16 });
                Ok(res)
            }
        }
    }
}
