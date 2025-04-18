use crate::oauth_types::{OAuthAuthorizationRequestParameters, OAuthTokenType};
use rocket::http::{ContentType, Status};
use rocket::serde::json::Json;
use rocket::{response, Request};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OAuthError {
    AccessDeniedError(OAuthAuthorizationRequestParameters, String, Option<String>),
    AccountSelectionRequiredError(OAuthAuthorizationRequestParameters, Option<String>),
    ConsentRequiredError(OAuthAuthorizationRequestParameters, Option<String>),
    /**
     * @see
     * {@link https://datatracker.ietf.org/doc/html/rfc9396#section-14.6 | RFC 9396 - OAuth Dynamic Client Registration Metadata Registration Error}
     *
     * The AS MUST refuse to process any unknown authorization details type or
     * authorization details not conforming to the respective type definition. The
     * AS MUST abort processing and respond with an error
     * invalid_authorization_details to the client if any of the following are true
     * of the objects in the authorization_details structure:
     *  - contains an unknown authorization details type value,
     *  - is an object of known type but containing unknown fields,
     *  - contains fields of the wrong type for the authorization details type,
     *  - contains fields with invalid values for the authorization details type, or
     *  - is missing required fields for the authorization details type.
     */
    InvalidAuthorizationDetailsError(OAuthAuthorizationRequestParameters, String),
    /**
     * @see
     * {@link https://datatracker.ietf.org/doc/html/rfc6749#section-5.2 | RFC6749 - Issuing an Access Token }
     *
     * Client authentication failed (e.g., unknown client, no client authentication
     * included, or unsupported authentication method). The authorization server MAY
     * return an HTTP 401 (Unauthorized) status code to indicate which HTTP
     * authentication schemes are supported.  If the client attempted to
     * authenticate via the "Authorization" request header field, the authorization
     * server MUST respond with an HTTP 401 (Unauthorized) status code and include
     * the "WWW-Authenticate" response header field matching the authentication
     * scheme used by the client.
     */
    InvalidClientError(String),
    /**
     * @see {@link https://datatracker.ietf.org/doc/html/rfc7591#section-3.2.2 | RFC7591 - Client Registration Error Response}
     *
     * The value of one of the client metadata fields is invalid and the server has
     * rejected this request.  Note that an authorization server MAY choose to
     * substitute a valid value for any requested parameter of a client's metadata.
     */
    InvalidClientIdError(String),
    /**
     * @see {@link https://datatracker.ietf.org/doc/html/rfc7591#section-3.2.2 | RFC7591 - Client Registration Error Response}
     *
     * The value of one of the client metadata fields is invalid and the server has
     * rejected this request.  Note that an authorization server MAY choose to
     * substitute a valid value for any requested parameter of a client's metadata.
     */
    InvalidClientMetadataError(String),
    /**
     * @see
     * {@link https://datatracker.ietf.org/doc/html/rfc6750#section-3.1 | RFC6750 - The WWW-Authenticate Response Header Field}
     *
     * @see
     * {@link https://datatracker.ietf.org/doc/html/rfc9449#name-the-dpop-authentication-sch | RFC9449 - The DPoP Authentication Scheme}
     */
    InvalidDpopKeyBindingError,
    InvalidDpopProofError(String),
    /**
     * @see
     * {@link https://datatracker.ietf.org/doc/html/rfc6749#section-5.2 | RFC6749 - Issuing an Access Token }
     *
     * The provided authorization grant (e.g., authorization code, resource owner
     * credentials) or refresh token is invalid, expired, revoked, does not match
     * the redirection URI used in the authorization request, or was issued to
     * another client.
     */
    InvalidGrantError(String),
    InvalidParametersError(OAuthAuthorizationRequestParameters, String),
    /**
     * @see {@link https://datatracker.ietf.org/doc/html/rfc7591#section-3.2.2 | RFC7591}
     *
     * The value of one or more redirection URIs is invalid.
     */
    InvalidRedirectUriError(String),
    /**
     * @see
     * {@link https://datatracker.ietf.org/doc/html/rfc6749#section-5.2 | RFC6749 - Issuing an Access Token }
     *
     * The request is missing a required parameter, includes an unsupported
     * parameter value (other than grant type), repeats a parameter, includes
     * multiple credentials, utilizes more than one mechanism for authenticating the
     * client, or is otherwise malformed.
     *
     * @see
     * {@link https://datatracker.ietf.org/doc/html/rfc6749#section-4.1.2.1 | RFC6749 - Authorization Code Grant, Authorization Request}
     *
     * The request is missing a required parameter, includes an invalid parameter
     * value, includes a parameter more than once, or is otherwise malformed.
     *
     * @see
     * {@link https://datatracker.ietf.org/doc/html/rfc6750#section-3.1 | RFC6750 - The WWW-Authenticate Response Header Field }
     *
     * The request is missing a required parameter, includes an unsupported
     * parameter or parameter value, repeats the same parameter, uses more than one
     * method for including an access token, or is otherwise malformed. The resource
     * server SHOULD respond with the HTTP 400 (Bad Request) status code.
     */
    InvalidRequestError(String),
    /**
     * @see {@link https://datatracker.ietf.org/doc/html/draft-ietf-oauth-v2-1-11#section-4.1.2.1}
     */
    InvalidScopeError(OAuthAuthorizationRequestParameters, String),
    /**
     * @see
     * {@link https://datatracker.ietf.org/doc/html/rfc6750#section-3.1 | RFC6750 - The WWW-Authenticate Response Header Field }
     *
     * The access token provided is expired, revoked, malformed, or invalid for
     * other reasons.  The resource SHOULD respond with the HTTP 401 (Unauthorized)
     * status code.  The client MAY request a new access token and retry the
     * protected resource request.
     */
    InvalidTokenError(OAuthTokenType, String),
    LoginRequiredError(OAuthAuthorizationRequestParameters, Option<String>),
    /**
     * @see
     * {@link https://datatracker.ietf.org/doc/html/rfc6749#section-5.2 | RFC6749 - Issuing an Access Token }
     *
     * The authenticated client is not authorized to use this authorization grant
     * type.
     *
     * @see
     * {@link https://datatracker.ietf.org/doc/html/rfc6749#section-4.1.2.1 | RFC6749 - Authorization Code Grant, Authorization Request}
     *
     * The client is not authorized to request an authorization code using this
     * method.
     */
    UnauthorizedClientError(String),
    /**
     * @see
     * {@link https://datatracker.ietf.org/doc/html/rfc9449#section-8 | RFC9449 - Section 8. Authorization Server-Provided Nonce}
     */
    UseDpopNonceError(Option<String>),
    RuntimeError(String),
    InvalidClientAuthMethod(String),
    JwtVerifyError(String),
    WwwAuthenticateError,
}

#[derive(Serialize)]
pub struct ErrorBody {
    error: String,
    message: String,
}

impl<'r, 'o: 'r> ::rocket::response::Responder<'r, 'o> for OAuthError {
    fn respond_to(self, __req: &'r Request<'_>) -> response::Result<'o> {
        match self {
            OAuthError::AccessDeniedError(_, _, _) => {
                unimplemented!()
            }
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
            OAuthError::UseDpopNonceError(message) => {
                let body = Json(ErrorBody {
                    error: "use_dpop_nonce".to_string(),
                    message: message
                        .unwrap_or("Authorization server requires nonce in DPoP proof".to_string()),
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
            OAuthError::AccountSelectionRequiredError(_, _) => {
                unimplemented!()
            }
            OAuthError::ConsentRequiredError(_, _) => {
                unimplemented!()
            }
            OAuthError::InvalidAuthorizationDetailsError(_, _) => {
                unimplemented!()
            }
            OAuthError::InvalidClientIdError(_) => {
                unimplemented!()
            }
            OAuthError::LoginRequiredError(_, _) => {
                unimplemented!()
            }
            OAuthError::WwwAuthenticateError => {
                unimplemented!()
            }
        }
    }
}
