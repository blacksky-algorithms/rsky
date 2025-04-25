mod oauth_authorize;
mod oauth_authorize_accept;
mod oauth_authorize_reject;
mod oauth_authorize_sign_in;
mod oauth_introspect;
mod oauth_jwks;
mod oauth_par;
mod oauth_revoke;
mod oauth_token;
mod oauth_well_known;

use rocket::http::{ContentType, Header, Status};
use rocket::request::FromRequest;
use rocket::response::{content, Responder};
use rocket::{response, routes, Request, Response, Route};
use rsky_oauth::oauth_provider::device::device_id::DeviceId;
use rsky_oauth::oauth_provider::lib::http::request::setup_csrf_token;
use rsky_oauth::oauth_provider::output::build_authorize_data::AuthorizationResultAuthorize;
use rsky_oauth::oauth_provider::output::send_authorize_redirect::AuthorizationResultRedirect;
use rsky_oauth::oauth_provider::request::request_uri::RequestUri;
use rsky_oauth::oauth_types::{
    OAuthAuthorizationRequestQuery, OAuthClientCredentials, OAuthClientId,
    OAuthTokenIdentification, Prompt, ResponseMode, WebUri,
};
use serde_json::json;
use std::hash::Hash;
use std::io::Cursor;
use url::{form_urlencoded, Url};

pub struct AcceptQuery {
    pub csrf_token: String,
    pub request_uri: RequestUri,
    pub client_id: OAuthClientId,
    pub account_sub: String,
}

pub struct OAuthAcceptRequestBody {
    pub oauth_client_credentials: OAuthClientCredentials,
    pub oauth_token_identification: OAuthTokenIdentification,
}

pub struct OAuthRejectRequestBody {
    pub oauth_client_credentials: OAuthClientCredentials,
    pub oauth_token_identification: OAuthTokenIdentification,
}

pub struct OAuthSigninRequestBody {
    pub device_id: DeviceId,
    pub credentials: OAuthClientCredentials,
    pub authorization_request: OAuthAuthorizationRequestQuery,
}

pub struct Dpop(String);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for Dpop {
    type Error = ();

    async fn from_request(req: &'r Request<'_>) -> rocket::request::Outcome<Self, Self::Error> {
        match req.headers().get_one("dpop") {
            None => rocket::request::Outcome::Error((Status::new(400), ())),
            Some(res) => rocket::request::Outcome::Success(Dpop(res.to_string())),
        }
    }
}

pub fn get_routes() -> Vec<Route> {
    routes![
        oauth_well_known::oauth_well_known,
        oauth_jwks::oauth_jwks,
        oauth_par::oauth_par,
        oauth_par::oauth_par_options,
        oauth_token::oauth_token,
        oauth_token::oauth_token_options,
        oauth_revoke::post_oauth_revoke,
        oauth_revoke::oauth_revoke_options,
        oauth_introspect::oauth_introspect,
        oauth_authorize::oauth_authorize,
        oauth_authorize_sign_in::oauth_authorize_sign_in,
        oauth_authorize_sign_in::oauth_signin_options,
        oauth_authorize_accept::oauth_authorize_accept,
        oauth_authorize_reject::oauth_authorize_reject,
        oauth_revoke::get_oauth_revoke,
        oauth_well_known::oauth_well_known_resources
    ]
}

pub fn csrf_cookie(uri: &RequestUri) -> String {
    "csrf-".to_string() + uri.to_string().as_str()
}

#[derive(Serialize, Deserialize)]
pub enum OAuthAuthorizeResponse {
    Redirect(AuthorizationResultRedirect),
    Page(AuthorizationResultAuthorize),
}

impl<'r> Responder<'r, 'static> for OAuthAuthorizeResponse {
    fn respond_to(self, request: &'r Request<'_>) -> response::Result<'static> {
        let mut response = Response::build();
        match self {
            OAuthAuthorizeResponse::Redirect(redirect) => {
                let uri = match redirect.parameters.redirect_uri {
                    None => {
                        response.status(Status::BadRequest);
                        let message = json!({
                            "error": "invalid_request",
                            "error_description": "No redirect_uri"
                        })
                        .to_string();
                        response.sized_body(message.len(), Cursor::new(message));
                        return Ok(response.finalize());
                    }
                    Some(uri) => uri,
                };
                let mode = redirect
                    .parameters
                    .response_mode
                    .unwrap_or(ResponseMode::Query);
                response.header(Header::new("Cache-Control", "no-store"));

                let issuer = redirect.issuer.into_inner();
                let state = redirect.parameters.state;
                let redirect_response = redirect.redirect.response;
                let session_state = redirect.redirect.session_state;
                let code = redirect.redirect.code;
                let id_token = redirect.redirect.id_token;
                let access_token = redirect.redirect.access_token;
                let expires_in = redirect.redirect.expires_in;
                let token_type = redirect.redirect.token_type;
                let error = redirect.redirect.error;
                let error_description = redirect.redirect.error_description;
                let error_uri = redirect.redirect.error_uri;

                match mode {
                    ResponseMode::Query => {
                        let mut url = Url::parse(uri.as_str()).unwrap();
                        let mut query_str = "".to_string();
                        query_str += format!("issuer={issuer}&").as_str();
                        if let Some(state) = state {
                            query_str += format!("state={state}&").as_str();
                        }
                        if let Some(redirect_response) = redirect_response {
                            query_str += format!("redirect_response={redirect_response}&").as_str();
                        }
                        if let Some(session_state) = session_state {
                            query_str += format!("session_state={session_state}&").as_str();
                        }
                        if let Some(code) = code {
                            query_str += format!("code={code}&").as_str();
                        }
                        if let Some(id_token) = id_token {
                            query_str += format!("id_token={id_token}&").as_str();
                        }

                        if let Some(access_token) = access_token {
                            query_str += format!("access_token={access_token}&").as_str();
                        }
                        if let Some(expires_in) = expires_in {
                            query_str += format!("expires_in={expires_in}&").as_str();
                        }
                        if let Some(token_type) = token_type {
                            query_str += format!("token_type={token_type}&").as_str();
                        }
                        if let Some(error) = error {
                            query_str += format!("error={error}&").as_str();
                        }
                        if let Some(error_description) = error_description {
                            query_str += format!("error_description={error_description}&").as_str();
                        }
                        if let Some(error_uri) = error_uri {
                            query_str += format!("error_uri={error_uri}&").as_str();
                        }
                        query_str.pop();
                        url.set_fragment(Some(query_str.as_str()));
                        response.status(Status::SeeOther);
                        response.header(Header::new("Location", url.as_str().to_string()));
                        return Ok(response.finalize());
                    }
                    ResponseMode::Fragment => {
                        let mut url = Url::parse(uri.as_str()).unwrap();
                        let mut fragment_str = "".to_string();
                        let issuer: String =
                            form_urlencoded::byte_serialize(issuer.as_bytes()).collect();
                        fragment_str += format!("iss={issuer}&").as_str();
                        if let Some(state) = state {
                            fragment_str += format!("state={state}&").as_str();
                        }
                        if let Some(redirect_response) = redirect_response {
                            fragment_str +=
                                format!("redirect_response={redirect_response}&").as_str();
                        }
                        if let Some(session_state) = session_state {
                            fragment_str += format!("session_state={session_state}&").as_str();
                        }
                        if let Some(code) = code {
                            fragment_str += format!("code={code}&").as_str();
                        }
                        if let Some(id_token) = id_token {
                            fragment_str += format!("id_token={id_token}&").as_str();
                        }

                        if let Some(access_token) = access_token {
                            fragment_str += format!("access_token={access_token}&").as_str();
                        }
                        if let Some(expires_in) = expires_in {
                            fragment_str += format!("expires_in={expires_in}&").as_str();
                        }
                        if let Some(token_type) = token_type {
                            fragment_str += format!("token_type={token_type}&").as_str();
                        }
                        if let Some(error) = error {
                            fragment_str += format!("error={error}&").as_str();
                        }
                        if let Some(error_description) = error_description {
                            fragment_str +=
                                format!("error_description={error_description}&").as_str();
                        }
                        if let Some(error_uri) = error_uri {
                            fragment_str += format!("error_uri={error_uri}&").as_str();
                        }
                        fragment_str.pop();
                        url.set_fragment(Some(fragment_str.as_str()));
                        response.status(Status::SeeOther);
                        response.header(Header::new("Location", url.as_str().to_string()));
                        return Ok(response.finalize());
                    }
                    ResponseMode::FormPost => unimplemented!(),
                }
            }
            OAuthAuthorizeResponse::Page(page) => {
                setup_csrf_token(request, csrf_cookie(&page.authorize.uri));
                response.header(Header::new(
                    "Permissions-Policy",
                    "otp-credentials=*, document-domain=()",
                ));
                response.header(Header::new(
                    "Cross-Origin-Embedder-Policy",
                    "credentialless",
                ));
                response.header(Header::new("Cross-Origin-Resource-Policy", "same-origin"));
                response.header(Header::new("Cross-Origin-Opener-Policy", "same-origin"));
                response.header(Header::new("Referrer-Policy", "same-origin"));
                response.header(Header::new("X-Frame-Options", "DENY"));
                response.header(Header::new("X-Content-Type-Options", "nosniff"));
                response.header(Header::new("X-XSS-Protection", "0"));
                response.header(Header::new("Strict-Transport-Security", "max-age=63072000"));
                response.header(Header::new("Content-Security-Policy", "same-origin"));

                // Build Document
                let issuer = page.issuer.to_string();
                let client_id = page.client.id.as_str();
                let request_uri = page.authorize.uri.into_inner();
                let csrf_cookie = "csrf-".to_string() + request_uri.as_str();
                let login_hint = page.parameters.login_hint.unwrap_or("".to_string());
                let client_name = page
                    .client
                    .metadata
                    .client_name
                    .clone()
                    .unwrap_or(client_id.to_string());
                let client_uri = match page.client.metadata.client_uri {
                    None => "".to_string(),
                    Some(client_uri) => client_uri.to_string(),
                };
                let new_session_requires_consent = match page.parameters.prompt {
                    None => "false",
                    Some(prompt) => {
                        if prompt == Prompt::Consent {
                            "true"
                        } else {
                            "false"
                        }
                    }
                };

                //Write HTML
                let html = format!(
                    "<!doctype html>
                        <html lang=\"en\">
                            <head>
                                <meta charset=\"UTF-8\" />
                                <title>Authorize</title>
                                <meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\"/>
                                <link rel=\"stylesheet\" href=\"/@atproto/oauth-provider/~assets/main.css\" /><style></style>
                            </head>
                            <body>
                                <div id=\"root\"></div>
                                <script>window[\"__authorizeData\"]={{\"clientId\":\"{client_id}\",\"clientMetadata\":{{\"redirect_uris\":[\"{issuer}/\"],\"response_types\":[\"code\"],\"grant_types\":[\"authorization_code\",\"refresh_token\"],\"scope\":\"atproto transition:generic\",\"token_endpoint_auth_method\":\"none\",\"application_type\":\"web\",\"client_id\":\"{client_id}\",\"client_name\":\"{client_name}\",\"client_uri\":\"{client_uri}\",\"dpop_bound_access_tokens\":true}},\"clientTrusted\":false,\"requestUri\":\"{request_uri}\",\"csrfCookie\":\"{csrf_cookie}\",\"loginHint\":\"{login_hint}\",\"newSessionsRequireConsent\":{new_session_requires_consent},\"scopeDetails\":[{{\"scope\":\"atproto\"}},{{\"scope\":\"transition:generic\"}}],\"sessions\":[]}};document.currentScript.remove();</script><script>window[\"__customizationData\"]={{\"name\":\"Personal PDS\",\"links\":[]}};document.currentScript.remove();</script><script type=\"module\" src=\"/@atproto/oauth-provider/~assets/main.js\"></script>
                            </body>
                        </html>",
                );

                response.header(ContentType::HTML);
                response.sized_body(html.len(), Cursor::new(html));
                response.ok()
            }
        }
    }
}
