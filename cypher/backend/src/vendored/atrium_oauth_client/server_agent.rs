use crate::vendored::atrium_oauth_client::constants::FALLBACK_ALG;
use crate::vendored::atrium_oauth_client::http_client::dpop::DpopClient;
use crate::vendored::atrium_oauth_client::jose::jwt::{RegisteredClaims, RegisteredClaimsAud};
use crate::vendored::atrium_oauth_client::keyset::Keyset;
use crate::vendored::atrium_oauth_client::resolver::OAuthResolver;
use crate::vendored::atrium_oauth_client::types::{
    OAuthAuthorizationServerMetadata, OAuthClientMetadata, OAuthTokenResponse,
    PushedAuthorizationRequestParameters, RefreshRequestParameters, TokenGrantType,
    TokenRequestParameters, TokenSet,
};
use crate::vendored::atrium_oauth_client::utils::{compare_algos, generate_nonce};
use atrium_api::types::string::Datetime;
use atrium_identity::{did::DidResolver, handle::HandleResolver};
use atrium_xrpc::HttpClient;
use atrium_xrpc::http::{Method, Request, StatusCode};
use chrono::{TimeDelta, Utc};
use jose_jwk::Key;
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;
use thiserror::Error;

// https://datatracker.ietf.org/doc/html/rfc7523#section-2.2
const CLIENT_ASSERTION_TYPE_JWT_BEARER: &str =
    "urn:ietf:params:oauth:client-assertion-type:jwt-bearer";

#[derive(Error, Debug)]
pub enum Error {
    #[error("no {0} endpoint available")]
    NoEndpoint(String),
    #[error("token response verification failed")]
    Token(String),
    #[error("unsupported authentication method")]
    UnsupportedAuthMethod,
    #[error(transparent)]
    DpopClient(#[from] crate::vendored::atrium_oauth_client::http_client::dpop::Error),
    #[error(transparent)]
    Http(#[from] atrium_xrpc::http::Error),
    #[error("http client error: {0}")]
    HttpClient(Box<dyn std::error::Error + Send + Sync + 'static>),
    #[error("http status: {0}")]
    HttpStatus(StatusCode),
    #[error("http status: {0}, body: {1:?}")]
    HttpStatusWithBody(StatusCode, Value),
    #[error(transparent)]
    Identity(#[from] atrium_identity::Error),
    #[error(transparent)]
    Keyset(#[from] crate::vendored::atrium_oauth_client::keyset::Error),
    #[error(transparent)]
    SerdeHtmlForm(#[from] serde_html_form::ser::Error),
    #[error(transparent)]
    SerdeJson(#[from] serde_json::Error),
}

pub type Result<T> = core::result::Result<T, Error>;

#[allow(dead_code)]
pub enum OAuthRequest {
    Token(TokenRequestParameters),
    Refresh(RefreshRequestParameters),
    Revocation,
    Introspection,
    PushedAuthorizationRequest(PushedAuthorizationRequestParameters),
}

impl OAuthRequest {
    fn name(&self) -> String {
        String::from(match self {
            Self::Token(_) => "token",
            Self::Refresh(_) => "refresh",
            Self::Revocation => "revocation",
            Self::Introspection => "introspection",
            Self::PushedAuthorizationRequest(_) => "pushed_authorization_request",
        })
    }
    fn expected_status(&self) -> StatusCode {
        match self {
            Self::Token(_) | Self::Refresh(_) => StatusCode::OK,
            Self::PushedAuthorizationRequest(_) => StatusCode::CREATED,
            _ => unimplemented!(),
        }
    }
}

#[derive(Debug, Serialize)]
struct RequestPayload<T>
where
    T: Serialize,
{
    client_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_assertion_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_assertion: Option<String>,
    #[serde(flatten)]
    parameters: T,
}

pub struct OAuthServerAgent<T, D, H>
where
    T: HttpClient + Send + Sync + 'static,
{
    server_metadata: OAuthAuthorizationServerMetadata,
    client_metadata: OAuthClientMetadata,
    dpop_client: DpopClient<T>,
    resolver: Arc<OAuthResolver<T, D, H>>,
    keyset: Option<Keyset>,
}

impl<T, D, H> OAuthServerAgent<T, D, H>
where
    T: HttpClient + Send + Sync + 'static,
    D: DidResolver + Send + Sync + 'static,
    H: HandleResolver + Send + Sync + 'static,
{
    pub fn new(
        dpop_key: Key,
        server_metadata: OAuthAuthorizationServerMetadata,
        client_metadata: OAuthClientMetadata,
        resolver: Arc<OAuthResolver<T, D, H>>,
        http_client: Arc<T>,
        keyset: Option<Keyset>,
    ) -> Result<Self> {
        let dpop_client = DpopClient::new(
            dpop_key,
            http_client,
            true,
            &server_metadata.token_endpoint_auth_signing_alg_values_supported,
        )?;
        Ok(Self {
            server_metadata,
            client_metadata,
            dpop_client,
            resolver,
            keyset,
        })
    }
    /**
     * VERY IMPORTANT ! Always call this to process token responses.
     *
     * Whenever an OAuth token response is received, we **MUST** verify that the
     * "sub" is a DID, whose issuer authority is indeed the server we just
     * obtained credentials from. This check is a critical step to actually be
     * able to use the "sub" (DID) as being the actual user's identifier.
     */
    async fn verify_token_response(&self, token_response: OAuthTokenResponse) -> Result<TokenSet> {
        // ATPROTO requires that the "sub" is always present in the token response.
        let Some(sub) = &token_response.sub else {
            return Err(Error::Token("missing `sub` in token response".into()));
        };
        let (metadata, identity) = self.resolver.resolve_from_identity(sub).await?;
        if metadata.issuer != self.server_metadata.issuer {
            return Err(Error::Token("issuer mismatch".into()));
        }
        let expires_at = token_response.expires_in.and_then(|expires_in| {
            Datetime::now()
                .as_ref()
                .checked_add_signed(TimeDelta::seconds(expires_in))
                .map(Datetime::new)
        });
        Ok(TokenSet {
            sub: sub.clone(),
            aud: identity.pds,
            iss: metadata.issuer,
            scope: token_response.scope,
            access_token: token_response.access_token,
            refresh_token: token_response.refresh_token,
            token_type: token_response.token_type,
            expires_at,
        })
    }
    pub async fn exchange_code(&self, code: &str, verifier: &str) -> Result<TokenSet> {
        self.verify_token_response(
            self.request(OAuthRequest::Token(TokenRequestParameters {
                grant_type: TokenGrantType::AuthorizationCode,
                code: code.into(),
                redirect_uri: self.client_metadata.redirect_uris[0].clone(), // ?
                code_verifier: verifier.into(),
            }))
            .await?,
        )
        .await
    }
    pub async fn request<O>(&self, request: OAuthRequest) -> Result<O>
    where
        O: serde::de::DeserializeOwned,
    {
        let Some(url) = self.endpoint(&request) else {
            return Err(Error::NoEndpoint(request.name()));
        };
        let body = match &request {
            OAuthRequest::Token(params) => self.build_body(params)?,
            OAuthRequest::Refresh(params) => self.build_body(params)?,
            OAuthRequest::PushedAuthorizationRequest(params) => self.build_body(params)?,
            _ => unimplemented!(),
        };
        let req = Request::builder()
            .uri(url)
            .method(Method::POST)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body.into_bytes())?;
        let res = self
            .dpop_client
            .send_http(req)
            .await
            .map_err(Error::HttpClient)?;
        if res.status() == request.expected_status() {
            Ok(serde_json::from_slice(res.body())?)
        } else if res.status().is_client_error() {
            Err(Error::HttpStatusWithBody(
                res.status(),
                serde_json::from_slice(res.body())?,
            ))
        } else {
            Err(Error::HttpStatus(res.status()))
        }
    }
    fn build_body<S>(&self, parameters: S) -> Result<String>
    where
        S: Serialize,
    {
        let (client_assertion_type, client_assertion) = self.build_auth()?;
        Ok(serde_html_form::to_string(RequestPayload {
            client_id: self.client_metadata.client_id.clone(),
            client_assertion_type,
            client_assertion,
            parameters,
        })?)
    }
    fn build_auth(&self) -> Result<(Option<String>, Option<String>)> {
        let method_supported = &self.server_metadata.token_endpoint_auth_methods_supported;
        let method = &self.client_metadata.token_endpoint_auth_method;
        match method.as_deref() {
            Some("private_key_jwt")
                if method_supported
                    .as_ref()
                    .map_or(false, |v| v.contains(&String::from("private_key_jwt"))) =>
            {
                if let Some(keyset) = &self.keyset {
                    let mut algs = self
                        .server_metadata
                        .token_endpoint_auth_signing_alg_values_supported
                        .clone()
                        .unwrap_or(vec![FALLBACK_ALG.into()]);
                    algs.sort_by(compare_algos);
                    let iat = Utc::now().timestamp();
                    return Ok((
                        Some(String::from(CLIENT_ASSERTION_TYPE_JWT_BEARER)),
                        Some(
                            keyset.create_jwt(
                                &algs,
                                // https://datatracker.ietf.org/doc/html/rfc7523#section-3
                                RegisteredClaims {
                                    iss: Some(self.client_metadata.client_id.clone()),
                                    sub: Some(self.client_metadata.client_id.clone()),
                                    aud: Some(RegisteredClaimsAud::Single(
                                        self.server_metadata.issuer.clone(),
                                    )),
                                    exp: Some(iat + 60),
                                    // "iat" is required and **MUST** be less than one minute
                                    // https://datatracker.ietf.org/doc/html/rfc9101
                                    iat: Some(iat),
                                    // atproto oauth-provider requires "jti" to be present
                                    jti: Some(generate_nonce()),
                                    ..Default::default()
                                }
                                .into(),
                            )?,
                        ),
                    ));
                }
            }
            Some("none")
                if method_supported
                    .as_ref()
                    .map_or(false, |v| v.contains(&String::from("none"))) =>
            {
                return Ok((None, None));
            }
            _ => {}
        }
        Err(Error::UnsupportedAuthMethod)
    }
    fn endpoint(&self, request: &OAuthRequest) -> Option<&String> {
        match request {
            OAuthRequest::Token(_) | OAuthRequest::Refresh(_) => {
                Some(&self.server_metadata.token_endpoint)
            }
            OAuthRequest::Revocation => self.server_metadata.revocation_endpoint.as_ref(),
            OAuthRequest::Introspection => self.server_metadata.introspection_endpoint.as_ref(),
            OAuthRequest::PushedAuthorizationRequest(_) => self
                .server_metadata
                .pushed_authorization_request_endpoint
                .as_ref(),
        }
    }
}
