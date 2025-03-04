use crate::oauth_types::OAuthTokenType;
use rocket::http::{ContentType, Status};
use rocket::serde::json::Json;
use rocket::{response, Request};
use serde::Serialize;

#[derive(Debug)]
pub enum OAuthError {
    InvalidGrantError(String),
    InvalidRequestError(String),
    InvalidClientMetadataError(String),
    InvalidRedirectUriError(String),
    InvalidParametersError(String),
    UnauthorizedClientError(String),
    InvalidTokenError(OAuthTokenType, String),
    InvalidDpopKeyBindingError,
    InvalidDpopProofError(String),
    RuntimeError(String),
    AccessDeniedError(String),
    InvalidClientAuthMethod(String),
    AccountSelectionRequiredError,
    LoginRequiredError,
    ConsentRequiredError,
}

#[derive(Serialize)]
pub struct ErrorBody {
    error: String,
    message: String,
}

impl<'r, 'o: 'r> ::rocket::response::Responder<'r, 'o> for OAuthError {
    fn respond_to(self, __req: &'r Request<'_>) -> response::Result<'o> {
        match self {
            _ => {
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
        }
    }
}
