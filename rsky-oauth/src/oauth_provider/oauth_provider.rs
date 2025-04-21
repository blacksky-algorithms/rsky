use crate::jwk::Keyset;
use crate::oauth_provider::access_token::access_token_type::AccessTokenType;
use crate::oauth_provider::account::account::Account;
use crate::oauth_provider::account::account_manager::AccountManager;
use crate::oauth_provider::account::account_store::{
    AccountStore, DeviceAccountInfo, SignInCredentials,
};
use crate::oauth_provider::client::client::Client;
use crate::oauth_provider::client::client_auth::ClientAuth;
use crate::oauth_provider::client::client_manager::{ClientManager, LoopbackMetadataGetter};
use crate::oauth_provider::client::client_store::ClientStore;
use crate::oauth_provider::constants::{AUTHENTICATION_MAX_AGE, TOKEN_MAX_AGE};
use crate::oauth_provider::device::device_id::DeviceId;
use crate::oauth_provider::device::device_store::DeviceStore;
use crate::oauth_provider::dpop::dpop_manager::DpopManagerOptions;
use crate::oauth_provider::dpop::dpop_nonce::DpopNonceInput;
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_provider::metadata::build_metadata::{build_metadata, CustomMetadata};
use crate::oauth_provider::now_as_secs;
use crate::oauth_provider::oauth_hooks::OAuthHooks;
use crate::oauth_provider::oauth_verifier::{OAuthVerifier, OAuthVerifierOptions};
use crate::oauth_provider::oidc::sub::Sub;
use crate::oauth_provider::output::build_authorize_data::{
    AuthorizationResultAuthorize, Authorize, Session,
};
use crate::oauth_provider::output::customization::Customization;
use crate::oauth_provider::output::send_authorize_redirect::{
    AuthorizationResponseParameters, AuthorizationResult, AuthorizationResultRedirect,
};
use crate::oauth_provider::replay::replay_store::ReplayStore;
use crate::oauth_provider::request::code::Code;
use crate::oauth_provider::request::request_info::RequestInfo;
use crate::oauth_provider::request::request_manager::RequestManager;
use crate::oauth_provider::request::request_store::RequestStore;
use crate::oauth_provider::request::request_store_memory::RequestStoreMemory;
use crate::oauth_provider::request::request_uri::RequestUri;
use crate::oauth_provider::token::token_id::TokenId;
use crate::oauth_provider::token::token_manager::{CreateTokenInput, TokenManager};
use crate::oauth_provider::token::token_store::TokenStore;
use crate::oauth_provider::token::verify_token_claims::{
    VerifyTokenClaimsOptions, VerifyTokenClaimsResult,
};
use crate::oauth_types::{
    ActiveTokenInfo, ApplicationType, OAuthAccessToken, OAuthAuthorizationCodeGrantTokenRequest,
    OAuthAuthorizationRequestJar, OAuthAuthorizationRequestPar,
    OAuthAuthorizationRequestParameters, OAuthAuthorizationRequestQuery,
    OAuthAuthorizationServerMetadata, OAuthClientCredentials, OAuthClientId, OAuthClientMetadata,
    OAuthIntrospectionResponse, OAuthIssuerIdentifier, OAuthParResponse,
    OAuthRefreshTokenGrantTokenRequest, OAuthResponseType, OAuthTokenIdentification,
    OAuthTokenRequest, OAuthTokenResponse, OAuthTokenType, Prompt,
    CLIENT_ASSERTION_TYPE_JWT_BEARER,
};
use crate::simple_store_memory::SimpleStoreMemory;
use jsonwebtoken::jwk::{Jwk, JwkSet};
use rocket::form::validate::Contains;
use rocket::http::Status;
use rocket::response::Responder;
use rocket::{response, Request, Response};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OAuthProviderSession {
    pub account: Account,
    pub info: DeviceAccountInfo,
    pub selected: bool,
    pub login_required: bool,
    pub consent_required: bool,
    pub matches_hint: bool,
}

pub struct AcceptRequestResponse {
    pub issuer: OAuthIssuerIdentifier,
    pub parameters: OAuthAuthorizationRequestParameters,
    pub redirect_code: Code,
}

pub struct RejectRequestResponse {
    pub issuer: OAuthIssuerIdentifier,
    pub parameters: OAuthAuthorizationRequestParameters,
    pub error: String,
    pub error_string: String,
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Eq)]
pub struct SignInResponse {
    pub account: Account,
    pub consent_required: bool,
}

impl<'r> Responder<'r, 'static> for SignInResponse {
    fn respond_to(self, request: &'r Request<'_>) -> response::Result<'static> {
        let mut response = Response::build();

        response.raw_header("Access-Control-Allow-Origin", "*");
        response.raw_header("Access-Control-Allow-Headers", "*");

        // https://www.rfc-editor.org/rfc/rfc6749.html#section-5.1
        response.raw_header("Cache-Control", "no-store");
        response.raw_header("Pragma", "no-cache");

        // https://datatracker.ietf.org/doc/html/rfc9449#section-8.2
        //TODO DPOP
        response.raw_header("DPoP-Nonce", "TODO");
        response.raw_header_adjoin("Access-Control-Expose-Headers", "DPoP-Nonce");

        match request.headers().get_one("accept") {
            None => {
                let mut response = Response::build();
                response.status(Status { code: 406u16 });
                return response.ok();
            }
            Some(accept_header) => {
                if accept_header != "application/json" {
                    let mut response = Response::build();
                    response.status(Status { code: 406u16 });
                    return response.ok();
                }
            }
        }

        let y = match serde_json::to_string(&self) {
            Ok(y) => y,
            Err(e) => {
                let mut response = Response::build();
                response.status(Status { code: 500u16 });
                return response.ok();
            }
        };
        response.sized_body(y.len(), Cursor::new(y));
        response.ok()
    }
}

pub struct DecodeJarResponse {
    pub payload: OAuthAuthorizationRequestParameters,
    pub kid: Option<String>,
    pub alg: Option<String>,
    pub jkt: Option<String>,
}

pub struct OAuthProviderOptions {
    /**
     * Maximum age a device/account session can be before requiring
     * re-authentication.
     */
    pub authentication_max_age: Option<u64>,

    /**
     * Maximum age access & id tokens can be before requiring a refresh.
     */
    pub token_max_age: Option<u64>,

    /**
     * Additional metadata to be included in the discovery document.
     */
    pub metadata: Option<CustomMetadata>,

    /**
     * UI customizations
     */
    pub customization: Option<Customization>,

    /**
     * A custom fetch function that can be used to fetch the client metadata from
     * the internet. By default, the fetch function is a safeFetchWrap() function
     * that protects against SSRF attacks, large responses & known bad domains. If
     * you want to disable all protections, you can provide `globalThis.fetch` as
     * fetch function.
     */
    pub safe_fetch: bool,

    /**
     * A redis instance to use for replay protection. If not provided, replay
     * protection will use memory storage.
     */
    pub redis: Option<String>,

    pub account_store: Arc<RwLock<dyn AccountStore>>,
    pub device_store: Arc<RwLock<dyn DeviceStore>>,
    pub client_store: Option<Arc<RwLock<dyn ClientStore>>>,
    pub replay_store: Option<Arc<RwLock<dyn ReplayStore>>>,
    pub request_store: Option<Arc<RwLock<dyn RequestStore>>>,
    pub token_store: Arc<RwLock<dyn TokenStore>>,

    /**
     * In order to speed up the client fetching process, you can provide a cache
     * to store HTTP responses.
     *
     * @note the cached entries should automatically expire after a certain time (typically 10 minutes)
     */
    pub client_jwks_cache: Arc<RwLock<SimpleStoreMemory<String, JwkSet>>>,

    /**
     * In order to speed up the client fetching process, you can provide a cache
     * to store HTTP responses.
     *
     * @note the cached entries should automatically expire after a certain time (typically 10 minutes)
     */
    pub client_metadata_cache: Arc<RwLock<SimpleStoreMemory<String, OAuthClientMetadata>>>,

    /**
     * In order to enable loopback clients, you can provide a function that
     * returns the client metadata for a given loopback URL. This is useful for
     * development and testing purposes. This function is not called for internet
     * clients.
     *
     * @default is as specified by ATPROTO
     */
    pub loopback_metadata: Option<LoopbackMetadataGetter>,
    pub dpop_secret: Option<DpopNonceInput>,
    pub dpop_step: Option<u64>,
    pub issuer: OAuthIssuerIdentifier,
    pub keyset: Option<Arc<RwLock<Keyset>>>,
    pub access_token_type: Option<AccessTokenType>,
    pub oauth_hooks: Arc<OAuthHooks>,
}

pub struct OAuthProvider {
    pub metadata: OAuthAuthorizationServerMetadata,
    customization: Option<Customization>,

    authentication_max_age: u64,

    client_manager: ClientManager,
    request_manager: RequestManager,
    token_manager: TokenManager,
    pub oauth_verifier: OAuthVerifier,
    device_store: Arc<RwLock<dyn DeviceStore>>,

    account_manager: AccountManager,
}

pub struct OAuthProviderCreatorParams {
    /**
     * Maximum age a device/account session can be before requiring
     * re-authentication.
     */
    pub authentication_max_age: Option<u64>,

    /**
     * Maximum age access & id tokens can be before requiring a refresh.
     */
    pub token_max_age: Option<u64>,

    /**
     * Additional metadata to be included in the discovery document.
     */
    pub metadata: Option<CustomMetadata>,

    /**
     * UI customizations
     */
    pub customization: Option<Customization>,

    /**
     * A custom fetch function that can be used to fetch the client metadata from
     * the internet. By default, the fetch function is a safeFetchWrap() function
     * that protects against SSRF attacks, large responses & known bad domains. If
     * you want to disable all protections, you can provide `globalThis.fetch` as
     * fetch function.
     */
    pub safe_fetch: bool,

    /**
     * A redis instance to use for replay protection. If not provided, replay
     * protection will use memory storage.
     */
    pub redis: Option<String>,

    /**
     * In order to speed up the client fetching process, you can provide a cache
     * to store HTTP responses.
     *
     * @note the cached entries should automatically expire after a certain time (typically 10 minutes)
     */
    pub client_jwks_cache: Option<Arc<RwLock<SimpleStoreMemory<String, JwkSet>>>>,

    /**
     * In order to speed up the client fetching process, you can provide a cache
     * to store HTTP responses.
     *
     * @note the cached entries should automatically expire after a certain time (typically 10 minutes)
     */
    pub client_metadata_cache: Option<Arc<RwLock<SimpleStoreMemory<String, OAuthClientMetadata>>>>,

    /**
     * In order to enable loopback clients, you can provide a function that
     * returns the client metadata for a given loopback URL. This is useful for
     * development and testing purposes. This function is not called for internet
     * clients.
     *
     * @default is as specified by ATPROTO
     */
    pub loopback_metadata: Option<LoopbackMetadataGetter>,
    pub dpop_secret: Option<DpopNonceInput>,
    pub dpop_step: Option<u64>,
    pub issuer: OAuthIssuerIdentifier,
    pub keyset: Option<Arc<RwLock<Keyset>>>,
    pub access_token_type: Option<AccessTokenType>,
    pub oauth_hooks: Arc<OAuthHooks>,
}

pub type OAuthProviderCreator = Box<
    dyn Fn(
            Arc<RwLock<dyn AccountStore>>,
            Option<Arc<RwLock<dyn RequestStore>>>,
            Arc<RwLock<dyn DeviceStore>>,
            Arc<RwLock<dyn TokenStore>>,
            Option<Arc<RwLock<dyn ClientStore>>>,
            Option<Arc<RwLock<dyn ReplayStore>>>,
        ) -> OAuthProvider
        + Send
        + Sync,
>;

pub struct OAuthProviderCreatorOptions {
    pub metadata: Option<OAuthAuthorizationServerMetadata>,
    pub authentication_max_age: Option<u64>,
}

impl OAuthProvider {
    pub fn creator(options: OAuthProviderCreatorParams) -> OAuthProviderCreator {
        let client_jwks_cache = options
            .client_jwks_cache
            .unwrap_or(Arc::new(RwLock::new(SimpleStoreMemory::default())));
        let client_metadata_cache = options
            .client_metadata_cache
            .unwrap_or(Arc::new(RwLock::new(SimpleStoreMemory::default())));
        // let loopback_metadata = Arc::new(options.loopback_metadata);
        Box::new(
            move |account_store: Arc<RwLock<dyn AccountStore>>,
                  request_store: Option<Arc<RwLock<dyn RequestStore>>>,
                  device_store: Arc<RwLock<dyn DeviceStore>>,
                  token_store: Arc<RwLock<dyn TokenStore>>,
                  client_store: Option<Arc<RwLock<dyn ClientStore>>>,
                  replay_store: Option<Arc<RwLock<dyn ReplayStore>>>|
                  -> OAuthProvider {
                let options = OAuthProviderOptions {
                    authentication_max_age: options.authentication_max_age,
                    token_max_age: options.token_max_age,
                    metadata: options.metadata.clone(),
                    customization: options.customization.clone(),
                    safe_fetch: false,
                    redis: options.redis.clone(),
                    account_store: account_store.clone(),
                    device_store: device_store.clone(),
                    client_store: client_store.clone(),
                    replay_store: replay_store.clone(),
                    request_store: request_store.clone(),
                    token_store: token_store.clone(),
                    client_jwks_cache: client_jwks_cache.clone(),
                    client_metadata_cache: client_metadata_cache.clone(),
                    loopback_metadata: None,
                    dpop_secret: options.dpop_secret.clone(),
                    dpop_step: options.dpop_step,
                    issuer: options.issuer.clone(),
                    keyset: options.keyset.clone(),
                    access_token_type: options.access_token_type.clone(),
                    oauth_hooks: options.oauth_hooks.clone(),
                };
                OAuthProvider::new(options).unwrap()
            },
        )
    }

    pub fn new(options: OAuthProviderOptions) -> Result<Self, OAuthError> {
        let oauth_hooks = options.oauth_hooks;
        let token_max_age = TOKEN_MAX_AGE;

        //safefetch wrap
        let redis = options.redis;

        // Requires stores
        let account_store = options.account_store;
        let device_store = options.device_store;
        let token_store = options.token_store;

        // These are optional
        let client_store = options.client_store;
        let replay_store = options.replay_store;
        let request_store = options.request_store;

        let client_jwks_cache = options.client_jwks_cache;
        let client_metadata_cache = options.client_metadata_cache;

        //loopback metadata different

        let dpop_options = match options.dpop_secret {
            None => None,
            Some(dpop_secret) => Some(DpopManagerOptions {
                dpop_secret: Some(dpop_secret),
                dpop_step: options.dpop_step,
            }),
        };
        let verifier_opts = OAuthVerifierOptions {
            issuer: options.issuer.clone(),
            keyset: options.keyset.unwrap(),
            access_token_type: options.access_token_type,
            redis: None,
            replay_store,
            dpop_options: None,
        };
        let oauth_verifier = OAuthVerifier::new(verifier_opts);

        let request_store = match request_store {
            None => match redis {
                None => Arc::new(RwLock::new(RequestStoreMemory::new())),
                Some(redis) => {
                    unimplemented!()
                }
            },
            Some(request_store) => request_store,
        };
        let metadata = build_metadata(options.issuer.clone(), options.metadata);
        let customization = options.customization;
        let authentication_max_age = options
            .authentication_max_age
            .unwrap_or_else(|| AUTHENTICATION_MAX_AGE as u64);

        let account_manager = AccountManager::new(account_store);
        let client_manager = ClientManager::new(
            metadata.clone(),
            oauth_verifier.keyset.clone(),
            oauth_hooks.clone(),
            client_store.unwrap(),
            options.loopback_metadata,
            client_jwks_cache,
            client_metadata_cache,
        );
        let request_manager = RequestManager::new(
            request_store,
            oauth_verifier.signer.clone(),
            metadata.clone(),
            token_max_age,
            oauth_hooks.clone(),
        );
        let token_manager = TokenManager::new(
            token_store,
            oauth_verifier.signer.clone(),
            oauth_verifier.access_token_type.clone(),
            Some(token_max_age),
            oauth_hooks,
        );

        Ok(OAuthProvider {
            oauth_verifier,
            metadata,
            customization,
            authentication_max_age,
            account_manager,
            client_manager,
            request_manager,
            token_manager,
            device_store,
        })
    }

    pub async fn get_jwks(&self) -> Vec<Jwk> {
        let keyset = self.oauth_verifier.keyset.read().await;
        keyset.public_jwks()
    }

    fn login_required(&self, info: &DeviceAccountInfo) -> bool {
        /* in seconds */
        let now = now_as_secs();
        let auth_age = now - info.authenticated_at.timestamp();
        auth_age as u64 >= self.authentication_max_age
    }

    async fn authenticate_client(
        &mut self,
        credentials: OAuthClientCredentials,
    ) -> Result<(Client, ClientAuth), OAuthError> {
        let client_id = credentials.client_id();
        let client = self.client_manager.get_client(client_id).await?;
        let (client_auth, nonce) = client
            .verify_credentials(credentials, &self.oauth_verifier.issuer)
            .await?;

        if client.metadata.application_type == ApplicationType::Native
            && client_auth.method == "none"
        {
            // https://datatracker.ietf.org/doc/html/rfc8252#section-8.4
            //
            // > Except when using a mechanism like Dynamic Client Registration
            // > [RFC7591] to provision per-instance secrets, native apps are
            // > classified as public clients, as defined by Section 2.1 of OAuth 2.0
            // > [RFC6749]; they MUST be registered with the authorization server as
            // > such. Authorization servers MUST record the client type in the client
            // > registration details in order to identify and process requests
            // > accordingly.
            return Err(OAuthError::InvalidGrantError(
                "Native clients must authenticate using \"none\" method".to_string(),
            ));
        }

        if let Some(nonce) = nonce {
            let unique = self
                .oauth_verifier
                .replay_manager
                .unique_auth(nonce, &client.id)
                .await;
            if !unique {
                return Err(OAuthError::InvalidGrantError("jti reused".to_string()));
            }
        }

        Ok((client, client_auth))
    }

    async fn decode_jar(
        &mut self,
        client: &Client,
        input: OAuthAuthorizationRequestJar,
    ) -> Result<DecodeJarResponse, OAuthError> {
        let result = client.decode_request_object(input.jwt()).await?;
        let claims = result.claims.clone();
        let payload = OAuthAuthorizationRequestParameters {
            client_id: result.claims.client_id.clone().unwrap(),
            state: None,
            redirect_uri: None,
            scope: None,
            response_type: OAuthResponseType::Code,
            code_challenge: None,
            code_challenge_method: None,
            dpop_jkt: None,
            response_mode: None,
            nonce: None,
            max_age: None,
            claims: None,
            login_hint: None,
            ui_locales: None,
            id_token_hint: None,
            display: None,
            prompt: None,
            authorization_details: None,
        };
        let jti: String = match claims.clone().jti {
            None => {
                return Err(OAuthError::InvalidParametersError(
                    payload,
                    "Request object must contain a jti claim".to_string(),
                ));
            }
            Some(jti) => jti,
        };

        if !self
            .oauth_verifier
            .replay_manager
            .unique_jar(jti, &client.id)
            .await
        {
            return Err(OAuthError::InvalidParametersError(
                payload,
                "Request object jti is not unique".to_string(),
            ));
        }

        let kid = result.header.kid;
        let alg = result.header.alg;
        unimplemented!()
        // let jkt = result.cl
        // if let Some(protected_header) = result.1 {
        //     let kid = protected_header.0;
        //     let alg = protected_header.1;
        //     let jkt = protected_header.2;
        //     Ok(DecodeJarResponse {
        //         payload,
        //         kid: Some(kid),
        //         alg: Some(alg),
        //         jkt: Some(jkt),
        //     })
        // } else {
        //     Ok(DecodeJarResponse {
        //         payload,
        //         kid: None,
        //         alg: None,
        //         jkt: None,
        //     })
        // }
    }

    /**
     * @see {@link https://datatracker.ietf.org/doc/html/rfc9126 }
     */
    pub async fn pushed_authorization_request(
        &mut self,
        credentials: OAuthClientCredentials,
        authorization_request: OAuthAuthorizationRequestPar,
        dpop_jkt: Option<String>,
    ) -> Result<OAuthParResponse, OAuthError> {
        let (client, client_auth) = self.authenticate_client(credentials).await?;
        let parameters = match authorization_request {
            OAuthAuthorizationRequestPar::Parameters(request) => request,
            OAuthAuthorizationRequestPar::Jar(request) => {
                match self.decode_jar(&client, request).await {
                    Ok(res) => res.payload,
                    Err(e) => return Err(OAuthError::InvalidRequestError("test".to_string())),
                }
            }
        };

        let res = self
            .request_manager
            .create_authorization_request(client, client_auth, parameters, None, dpop_jkt)
            .await?;

        let expires_in = res.expires_at.timestamp() - now_as_secs();
        let response = OAuthParResponse::new(res.uri.into_inner(), expires_in).unwrap();
        Ok(response)
    }

    async fn process_authorization_request(
        &mut self,
        client: Client,
        device_id: DeviceId,
        query: OAuthAuthorizationRequestQuery,
    ) -> Result<RequestInfo, OAuthError> {
        match query.clone() {
            OAuthAuthorizationRequestQuery::Parameters(query) => {
                let auth = ClientAuth {
                    method: "none".to_string(),
                    alg: "".to_string(),
                    kid: "".to_string(),
                    jkt: "".to_string(),
                };
                self.request_manager
                    .create_authorization_request(client, auth, query, Some(device_id), None)
                    .await
            }
            OAuthAuthorizationRequestQuery::Jar(request_query) => {
                let request_object = self.decode_jar(&client, request_query).await?;
                if request_object.kid.is_some() {
                    // Allow using signed JAR during "/authorize" as client authentication.
                    // This allows clients to skip PAR to initiate trusted sessions.
                    let client_auth = ClientAuth {
                        method: CLIENT_ASSERTION_TYPE_JWT_BEARER.to_string(),
                        alg: request_object.alg.unwrap(),
                        kid: request_object.kid.unwrap(),
                        jkt: request_object.jkt.unwrap(),
                    };

                    self.request_manager
                        .create_authorization_request(
                            client,
                            client_auth,
                            request_object.payload,
                            Some(device_id),
                            None,
                        )
                        .await
                } else {
                    let client_auth = ClientAuth {
                        method: "none".to_string(),
                        alg: "".to_string(),
                        kid: "".to_string(),
                        jkt: "".to_string(),
                    };
                    self.request_manager
                        .create_authorization_request(
                            client,
                            client_auth,
                            request_object.payload,
                            Some(device_id),
                            None,
                        )
                        .await
                }
            }
            OAuthAuthorizationRequestQuery::Uri(query) => {
                let request_uri = match RequestUri::new(query.request_uri().clone().into_inner()) {
                    Ok(request_uri) => request_uri,
                    Err(e) => {
                        return Err(OAuthError::InvalidRequestError(
                            "Invalid request uri".to_string(),
                        ))
                    }
                };
                self.request_manager
                    .get(request_uri, client.id, device_id)
                    .await
            }
        }
    }

    async fn delete_request(&mut self, uri: RequestUri) -> Result<(), OAuthError> {
        self.request_manager.delete(&uri).await;
        Ok(())
    }

    /**
     * @see {@link https://datatracker.ietf.org/doc/html/draft-ietf-oauth-v2-1-11#section-4.1.1}
     */
    pub async fn authorize(
        &mut self,
        device_id: &DeviceId,
        credentials: &OAuthClientCredentials,
        query: &OAuthAuthorizationRequestQuery,
    ) -> Result<AuthorizationResult, OAuthError> {
        let issuer = self.oauth_verifier.issuer.clone();

        // If there is a chance to redirect the user to the client, let's do it
        let access_denied_redirect =
            if let OAuthAuthorizationRequestQuery::Parameters(params) = query {
                if params.redirect_uri.is_some() {
                    // https://datatracker.ietf.org/doc/html/draft-ietf-oauth-v2-1-11#section-4.1.2.1
                    Some(OAuthError::AccessDeniedError(
                        params.clone(),
                        "invalid_request".to_string(),
                        None,
                    ))
                } else {
                    None
                }
            } else {
                None
            };

        let client = match self
            .client_manager
            .get_client(credentials.client_id())
            .await
        {
            Ok(client) => client,
            Err(e) => {
                return match access_denied_redirect {
                    None => Err(e),
                    Some(access_denied_redirect) => Err(access_denied_redirect),
                }
            }
        };

        let request_info = match self
            .process_authorization_request(client.clone(), device_id.clone(), query.clone())
            .await
        {
            Ok(request_info) => request_info,
            Err(e) => {
                return match access_denied_redirect {
                    None => Err(e),
                    Some(access_denied_redirect) => Err(access_denied_redirect),
                }
            }
        };
        let parameters = request_info.parameters;
        let uri = request_info.uri;

        let sessions = self.get_sessions(&client, device_id, &parameters).await?;

        if let Some(prompt) = parameters.prompt {
            if prompt == Prompt::None {
                let sso_sessions: Vec<OAuthProviderSession> =
                    sessions.into_iter().filter(|s| s.matches_hint).collect();
                if sso_sessions.is_empty() {
                    return Err(OAuthError::LoginRequiredError(parameters, None));
                }

                if sso_sessions.len() > 1 {
                    return Err(OAuthError::AccountSelectionRequiredError(parameters, None));
                }

                let sso_session = sso_sessions.first().unwrap();
                if sso_session.login_required {
                    return Err(OAuthError::LoginRequiredError(parameters, None));
                }
                if sso_session.consent_required {
                    return Err(OAuthError::ConsentRequiredError(parameters, None));
                }

                let code = self
                    .request_manager
                    .set_authorized(&uri, device_id, &sso_session.account)
                    .await?;

                return Ok(AuthorizationResult::Redirect(AuthorizationResultRedirect {
                    issuer,
                    parameters,
                    redirect: AuthorizationResponseParameters {
                        code: Some(code),
                        id_token: None,
                        access_token: None,
                        token_type: None,
                        expires_in: None,
                        response: None,
                        session_state: None,
                        error: None,
                        error_description: None,
                        error_uri: None,
                    },
                }));
            }
        } else {
            // Automatic SSO when a did was provided
            if let Some(login_hint) = parameters.login_hint.clone() {
                let sso_sessions: Vec<OAuthProviderSession> = sessions
                    .clone()
                    .into_iter()
                    .filter(|session| session.matches_hint)
                    .collect();
                if sso_sessions.len() == 1 {
                    let sso_session = sso_sessions.first().unwrap();
                    if !sso_session.login_required && !sso_session.consent_required {
                        let code = match self
                            .request_manager
                            .set_authorized(&uri, device_id, &sso_session.account)
                            .await
                        {
                            Ok(code) => code,
                            Err(e) => {
                                self.delete_request(uri).await?;
                                return Err(OAuthError::AccessDeniedError(
                                    parameters,
                                    "invalid_request".to_string(),
                                    None,
                                ));
                            }
                        };
                        return Ok(AuthorizationResult::Redirect(AuthorizationResultRedirect {
                            issuer,
                            parameters,
                            redirect: AuthorizationResponseParameters {
                                code: Some(code),
                                id_token: None,
                                access_token: None,
                                token_type: None,
                                expires_in: None,
                                response: None,
                                session_state: None,
                                error: None,
                                error_description: None,
                                error_uri: None,
                            },
                        }));
                    }
                }
            }
        }

        let mut res_sessions: Vec<Session> = vec![];
        for session in sessions {
            res_sessions.push(Session::from(session));
        }
        Ok(AuthorizationResult::Authorize(
            AuthorizationResultAuthorize {
                issuer,
                client,
                parameters,
                authorize: Authorize {
                    uri,
                    scope_details: None,
                    sessions: res_sessions,
                },
            },
        ))
    }

    pub async fn get_sessions(
        &self,
        client: &Client,
        device_id: &DeviceId,
        parameters: &OAuthAuthorizationRequestParameters,
    ) -> Result<Vec<OAuthProviderSession>, OAuthError> {
        let accounts = self.account_manager.list(device_id).await;

        let hint = parameters.login_hint.clone();

        fn matches_hint(account: Account, hint: Option<String>) -> bool {
            (account.sub.get() == hint.clone().unwrap())
                || (account.preferred_username.is_some()
                    && account.preferred_username.unwrap() == hint.unwrap())
        }

        let mut sessions = Vec::new();
        for account_info in accounts {
            let account = account_info.account;
            let info = account_info.info;
            // If an account uses the sub of another account as preferred_username,
            // there might be multiple accounts matching the hint. In that case,
            // selecting the account automatically may have unexpected results (i.e.
            // not able to login using desired account).
            // TODO
            let selected = if parameters.prompt.unwrap_or(Prompt::None) != Prompt::SelectAccount
                && matches_hint(account.clone(), hint.clone())
            {
                true
            } else {
                false
            };
            let login_required = parameters.prompt.unwrap_or(Prompt::None) == Prompt::Login
                || self.login_required(&info);

            // @TODO the "authorizedClients" should also include the scopes that
            // were already authorized for the client. Otherwise a client could
            // use silent authentication to get additional scopes without consent.
            let consent_required = parameters.prompt.unwrap_or(Prompt::None) == Prompt::Consent
                || !info.authorized_clients.contains(client.id.clone());
            let matches_hint = hint.is_none() || matches_hint(account.clone(), hint.clone());

            let session = OAuthProviderSession {
                account,
                info,
                selected,
                login_required,
                consent_required,
                matches_hint,
            };
            sessions.push(session);
        }

        Ok(sessions)
    }

    pub async fn sign_in(
        &mut self,
        device_id: DeviceId,
        uri: RequestUri,
        client_id: OAuthClientId,
        credentials: SignInCredentials,
    ) -> Result<SignInResponse, OAuthError> {
        let client = self.client_manager.get_client(&client_id).await?;

        // Ensure the request is still valid (and update the request expiration)
        // @TODO use the returned scopes to determine if consent is required
        self.request_manager
            .get(uri, client_id, device_id.clone())
            .await?;

        let account_info = match self
            .account_manager
            .sign_in(credentials, device_id.clone())
            .await
        {
            Ok(res) => res,
            Err(error) => return Err(error),
        };
        let account = account_info.account;
        let info = account_info.info;
        let consent_required = match client.info.is_first_party {
            true => false,
            false => {
                // @TODO: the "authorizedClients" should also include the scopes that
                // were already authorized for the client. Otherwise a client could
                // use silent authentication to get additional scopes without consent.
                !info.authorized_clients.contains(&client.id)
            }
        };
        Ok(SignInResponse {
            account,
            consent_required,
        })
    }

    pub async fn accept_request(
        &mut self,
        device_id: DeviceId,
        uri: RequestUri,
        client_id: OAuthClientId,
        sub: Sub,
    ) -> Result<AuthorizationResultRedirect, OAuthError> {
        let client = self.client_manager.get_client(&client_id).await?;

        let result = self
            .request_manager
            .get(uri.clone(), client_id, device_id.clone())
            .await?;
        let parameters = result.parameters;
        let client_auth = result.client_auth;

        let result = match self.account_manager.get(&device_id, sub).await {
            Ok(res) => res,
            Err(e) => {
                self.delete_request(uri).await?;
                return Err(OAuthError::AccessDeniedError(
                    parameters,
                    "".to_string(),
                    None,
                ));
            }
        };
        let account = result.account;
        let info = result.info;

        // The user is trying to authorize without a fresh login
        if self.login_required(&info) {
            return Err(OAuthError::LoginRequiredError(parameters, None));
        }

        let code = match self
            .request_manager
            .set_authorized(&uri, &device_id, &account)
            .await
        {
            Ok(res) => res,
            Err(e) => {
                self.delete_request(uri).await?;
                return Err(OAuthError::AccessDeniedError(
                    parameters,
                    "".to_string(),
                    None,
                ));
            }
        };

        self.account_manager
            .add_authorized_client(device_id, account, client, client_auth)
            .await;

        Ok(AuthorizationResultRedirect {
            issuer: self.oauth_verifier.issuer.clone(),
            parameters,
            redirect: AuthorizationResponseParameters {
                code: Some(code),
                id_token: None,
                access_token: None,
                token_type: None,
                expires_in: None,
                response: None,
                session_state: None,
                error: None,
                error_description: None,
                error_uri: None,
            },
        })
    }

    pub async fn reject_request(
        &mut self,
        device_id: DeviceId,
        uri: RequestUri,
        client_id: OAuthClientId,
    ) -> Result<AuthorizationResultRedirect, OAuthError> {
        let request_info = self
            .request_manager
            .get(uri.clone(), client_id, device_id)
            .await?;

        self.delete_request(uri).await?;

        Ok(AuthorizationResultRedirect {
            issuer: self.oauth_verifier.issuer.clone(),
            parameters: request_info.parameters,
            redirect: AuthorizationResponseParameters {
                code: None,
                id_token: None,
                access_token: None,
                token_type: None,
                expires_in: None,
                response: None,
                session_state: None,
                error: Some("access_denied".to_string()),
                error_description: Some("Access denied".to_string()),
                error_uri: None,
            },
        })
    }

    pub async fn token(
        &mut self,
        credentials: OAuthClientCredentials,
        request: OAuthTokenRequest,
        dpop_jkt: Option<String>,
    ) -> Result<OAuthTokenResponse, OAuthError> {
        let (client, client_auth) = self.authenticate_client(credentials).await?;

        let request_grant_type = request.as_oauth_grant_type_enum();
        if let Some(grant_types_supported) = &self.metadata.grant_types_supported {
            if !grant_types_supported.contains(&request_grant_type) {
                return Err(OAuthError::InvalidGrantError(format!(
                    "Grant type {request_grant_type} is not supported by the server"
                )));
            }
        }

        if !client.metadata.grant_types.contains(&request_grant_type) {
            return Err(OAuthError::InvalidGrantError(format!(
                "Grant type {request_grant_type} is not supported by the server"
            )));
        }

        match request {
            OAuthTokenRequest::AuthorizationCode(request) => {
                self.code_grant(client, client_auth, request, dpop_jkt)
                    .await
            }
            OAuthTokenRequest::RefreshToken(request) => {
                self.refresh_token_grant(client, client_auth, request, dpop_jkt)
                    .await
            }
            OAuthTokenRequest::Password(_) => Err(OAuthError::InvalidGrantError(format!(
                "Grant type {request_grant_type} is not supported by the server"
            ))),
            OAuthTokenRequest::ClientCredentials(_) => Err(OAuthError::InvalidGrantError(format!(
                "Grant type {request_grant_type} is not supported by the server"
            ))),
        }
    }

    pub async fn code_grant(
        &mut self,
        client: Client,
        client_auth: ClientAuth,
        input: OAuthAuthorizationCodeGrantTokenRequest,
        dpop_jkt: Option<String>,
    ) -> Result<OAuthTokenResponse, OAuthError> {
        let code = match Code::new(input.code().clone().into_inner()) {
            Ok(code) => code,
            Err(_) => return Err(OAuthError::InvalidRequestError("Invalid code".to_string())),
        };

        let request_data_authorized = self
            .request_manager
            .find_code(client.clone(), client_auth.clone(), code)
            .await?;

        // the following check prevents re-use of PKCE challenges, enforcing the
        // clients to generate a new challenge for each authorization request. The
        // replay manager typically prevents replay over a certain time frame,
        // which might not cover the entire lifetime of the token (depending on
        // the implementation of the replay store). For this reason, we should
        // ideally ensure that the code_challenge was not already used by any
        // existing token or any other pending request.
        //
        // The current implementation will cause client devs not issuing a new
        // code challenge for each authorization request to fail, which should be
        // a good enough incentive to follow the best practices, until we have a
        // better implementation.
        if let Some(ref code_challenge) = request_data_authorized.parameters.code_challenge {
            let unique = self
                .oauth_verifier
                .replay_manager
                .unique_code_challenge(code_challenge.clone())
                .await;
            if !unique {
                return Err(OAuthError::InvalidGrantError(
                    "Code challenge already used".to_string(),
                ));
            }
        }

        let account_info = self
            .account_manager
            .get(
                &request_data_authorized.device_id,
                request_data_authorized.sub,
            )
            .await?;

        self.token_manager
            .create(
                client,
                client_auth,
                account_info.account,
                Some((request_data_authorized.device_id, account_info.info)),
                request_data_authorized.parameters,
                None, // input,
                CreateTokenInput::Authorization(input),
            )
            .await
    }

    pub async fn refresh_token_grant(
        &self,
        client: Client,
        client_auth: ClientAuth,
        input: OAuthRefreshTokenGrantTokenRequest,
        dpop_jkt: Option<String>,
    ) -> Result<OAuthTokenResponse, OAuthError> {
        self.token_manager
            .refresh(client, client_auth, input, dpop_jkt)
            .await
    }

    /**
     * @see {@link https://datatracker.ietf.org/doc/html/rfc7009#section-2.1 rfc7009}
     */
    pub async fn revoke(&mut self, token: OAuthTokenIdentification) -> Result<(), OAuthError> {
        // @TODO this should also remove the account-device association (or, at least, mark it as expired)
        self.token_manager.revoke(token).await
    }

    /**
     * @see {@link https://datatracker.ietf.org/doc/html/rfc7662#section-2.1 rfc7662}
     */
    pub async fn introspect(
        &mut self,
        credentials: OAuthClientCredentials,
        token: OAuthTokenIdentification,
    ) -> Result<OAuthIntrospectionResponse, OAuthError> {
        let (client, client_auth) = self.authenticate_client(credentials).await?;

        // RFC7662 states the following:
        //
        // > To prevent token scanning attacks, the endpoint MUST also require some
        // > form of authorization to access this endpoint, such as client
        // > authentication as described in OAuth 2.0 [RFC6749] or a separate OAuth
        // > 2.0 access token such as the bearer token described in OAuth 2.0 Bearer
        // > Token Usage [RFC6750]. The methods of managing and validating these
        // > authentication credentials are out of scope of this specification.
        if client_auth.method == "none" {
            return Err(OAuthError::UnauthorizedClientError(
                "Client authentication required".to_string(),
            ));
        }

        let token_info = match self
            .token_manager
            .client_token_info(&client, &client_auth, &token)
            .await
        {
            Ok(res) => res,
            Err(e) => return Err(e),
        };

        let token_type = match token_info.data.parameters.dpop_jkt {
            None => OAuthTokenType::Bearer,
            Some(_) => OAuthTokenType::DPoP,
        };
        let signer = self.oauth_verifier.signer.read().await;
        Ok(OAuthIntrospectionResponse::Active(ActiveTokenInfo {
            scope: Some(token_info.data.parameters.scope.unwrap().into_inner()),
            client_id: Some(token_info.data.client_id),
            username: token_info.account.preferred_username,
            token_type: Some(token_type),
            authorization_details: token_info.data.details,
            aud: None, //token_info.account.aud,
            exp: Some(token_info.data.expires_at.timestamp()),
            iat: Some(token_info.data.updated_at.timestamp()),
            iss: Some(signer.issuer.to_string()),
            jti: Some(token_info.id.val()),
            nbf: None,
            sub: Some(token_info.account.sub.get()),
        }))
    }

    async fn authenticate_token(
        &self,
        token_type: OAuthTokenType,
        token: OAuthAccessToken,
        dpop_jkt: Option<String>,
        verify_options: Option<VerifyTokenClaimsOptions>,
    ) -> Result<VerifyTokenClaimsResult, OAuthError> {
        if let Ok(token_id) = TokenId::new(token.clone().into_inner()) {
            self.oauth_verifier
                .assert_token_type_allowed(token_type.clone(), AccessTokenType::ID)?;

            return Ok(self
                .token_manager
                .authenticate_token_id(token_type, token_id, dpop_jkt, verify_options)
                .await?
                .verify_token_claims_result);
        }

        self.oauth_verifier
            .authenticate_token(token_type, token, dpop_jkt, verify_options)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jwk::{Audience, Key, SignedJwt};
    use crate::jwk_jose::jose_key::JoseKey;
    use crate::oauth_provider::account::account_store::AccountInfo;
    use crate::oauth_provider::client::client_info::ClientInfo;
    use crate::oauth_provider::constants::PAR_EXPIRES_IN;
    use crate::oauth_provider::device::device_data::DeviceData;
    use crate::oauth_provider::device::device_store::PartialDeviceData;
    use crate::oauth_provider::request::request_data::RequestData;
    use crate::oauth_provider::request::request_id::RequestId;
    use crate::oauth_provider::request::request_store::{FoundRequestResult, UpdateRequestData};
    use crate::oauth_provider::signer::signer::Signer;
    use crate::oauth_provider::token::refresh_token::RefreshToken;
    use crate::oauth_provider::token::token_data::TokenData;
    use crate::oauth_provider::token::token_store::{NewTokenData, TokenInfo};
    use crate::oauth_types::{
        Display, OAuthClientCredentialsJwtBearer, OAuthCodeChallengeMethod,
        OAuthEndpointAuthMethod, OAuthGrantType, OAuthRedirectUri, OAuthRefreshToken,
        OAuthResponseType, OAuthScope, ResponseMode, TokenTypeHint, ValidUri, WebUri,
    };
    use chrono::Utc;
    use jsonwebtoken::jwk::{
        AlgorithmParameters, CommonParameters, EllipticCurve, EllipticCurveKeyParameters,
        EllipticCurveKeyType, KeyAlgorithm, KeyOperations, PublicKeyUse,
    };
    use std::future::Future;
    use std::pin::Pin;

    struct TestStore {}

    impl AccountStore for TestStore {
        fn authenticate_account(
            &self,
            credentials: SignInCredentials,
            device_id: DeviceId,
        ) -> Pin<Box<dyn Future<Output = Result<Option<AccountInfo>, OAuthError>> + Send + Sync + '_>>
        {
            let credentials = credentials;
            let device_id = device_id;
            Box::pin(async move {
                Ok(Some(AccountInfo {
                    account: Account {
                        sub: Sub::new("did:plc:khvyd3oiw46vif5gm7hijslk").unwrap(),
                        aud: Audience::Single("did:web:pds.ripperoni.com".to_string()),
                        preferred_username: None,
                        email: None,
                        email_verified: None,
                        picture: None,
                        name: None,
                    },
                    info: DeviceAccountInfo {
                        remembered: false,
                        authenticated_at: Utc::now(),
                        authorized_clients: vec![],
                    },
                }))
            })
        }

        fn add_authorized_client(
            &self,
            device_id: DeviceId,
            sub: Sub,
            client_id: OAuthClientId,
        ) -> Pin<Box<dyn Future<Output = Result<(), OAuthError>> + Send + Sync + '_>> {
            unimplemented!()
        }

        fn get_device_account(
            &self,
            device_id: DeviceId,
            sub: Sub,
        ) -> Pin<Box<dyn Future<Output = Result<Option<AccountInfo>, OAuthError>> + Send + Sync + '_>>
        {
            unimplemented!()
        }

        fn remove_device_account(
            &self,
            device_id: DeviceId,
            sub: Sub,
        ) -> Pin<Box<dyn Future<Output = Result<(), OAuthError>> + Send + Sync + '_>> {
            unimplemented!()
        }

        fn list_device_accounts(
            &self,
            device_id: DeviceId,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<AccountInfo>, OAuthError>> + Send + Sync + '_>>
        {
            unimplemented!()
        }
    }

    impl DeviceStore for TestStore {
        fn create_device(
            &mut self,
            device_id: DeviceId,
            data: DeviceData,
        ) -> Pin<Box<dyn Future<Output = Result<(), OAuthError>> + Send + Sync + '_>> {
            unimplemented!()
        }

        fn read_device(
            &self,
            device_id: DeviceId,
        ) -> Pin<Box<dyn Future<Output = Result<Option<DeviceData>, OAuthError>> + Send + Sync + '_>>
        {
            unimplemented!()
        }

        fn update_device(
            &mut self,
            device_id: DeviceId,
            data: PartialDeviceData,
        ) -> Pin<Box<dyn Future<Output = Result<(), OAuthError>> + Send + Sync + '_>> {
            unimplemented!()
        }

        fn delete_device(
            &mut self,
            device_id: DeviceId,
        ) -> Pin<Box<dyn Future<Output = Result<(), OAuthError>> + Send + Sync + '_>> {
            unimplemented!()
        }
    }

    impl TokenStore for TestStore {
        fn create_token(
            &mut self,
            token_id: TokenId,
            data: TokenData,
            refresh_token: Option<RefreshToken>,
        ) -> Pin<Box<dyn Future<Output = Result<(), OAuthError>> + Send + Sync + '_>> {
            unimplemented!()
        }

        fn read_token(
            &self,
            token_id: TokenId,
        ) -> Pin<Box<dyn Future<Output = Result<Option<TokenInfo>, OAuthError>> + Send + Sync + '_>>
        {
            unimplemented!()
        }

        fn delete_token(
            &mut self,
            token_id: TokenId,
        ) -> Pin<Box<dyn Future<Output = Result<(), OAuthError>> + Send + Sync + '_>> {
            Box::pin(async move {
                if token_id
                    == TokenId::new(
                        "tok-a41a16a716951a211b9ba177b121ce469128fb4eeb4cea6f10fd949014ab48c4",
                    )
                    .unwrap()
                {
                    Ok(())
                } else {
                    panic!()
                }
            })
        }

        fn rotate_token(
            &mut self,
            token_id: TokenId,
            new_token_id: TokenId,
            new_refresh_token: RefreshToken,
            new_data: NewTokenData,
        ) -> Pin<Box<dyn Future<Output = Result<(), OAuthError>> + Send + Sync + '_>> {
            unimplemented!()
        }

        fn find_token_by_refresh_token(
            &self,
            refresh_token: RefreshToken,
        ) -> Pin<Box<dyn Future<Output = Result<Option<TokenInfo>, OAuthError>> + Send + Sync + '_>>
        {
            let refresh_token = refresh_token;
            Box::pin(async move {
                if refresh_token
                    == RefreshToken::new(
                        "ref-a41a16a716951a211b9ba177b121ce469128fb4eeb4cea6f10fd949014ab48c4",
                    )
                    .unwrap()
                {
                    Ok(Some(TokenInfo {
                        id: TokenId::new(
                            "tok-a41a16a716951a211b9ba177b121ce469128fb4eeb4cea6f10fd949014ab48c4",
                        )
                        .unwrap(),
                        data: TokenData {
                            created_at: Utc::now(),
                            updated_at: Utc::now(),
                            expires_at: Utc::now(),
                            client_id: OAuthClientId::new(
                                "https://cleanfollow-bsky.pages.dev/client-metadata.json",
                            )
                            .unwrap(),
                            client_auth: ClientAuth {
                                method: "POST".to_string(),
                                alg: "".to_string(),
                                kid: "".to_string(),
                                jkt: "".to_string(),
                            },
                            device_id: None,
                            sub: Sub::new("sub1").unwrap(),
                            parameters: OAuthAuthorizationRequestParameters {
                                client_id: OAuthClientId::new(
                                    "https://cleanfollow-bsky.pages.dev/client-metadata.json",
                                )
                                .unwrap(),
                                state: None,
                                redirect_uri: None,
                                scope: None,
                                response_type: OAuthResponseType::Code,
                                code_challenge: None,
                                code_challenge_method: None,
                                dpop_jkt: None,
                                response_mode: None,
                                nonce: None,
                                max_age: None,
                                claims: None,
                                login_hint: None,
                                ui_locales: None,
                                id_token_hint: None,
                                display: None,
                                prompt: None,
                                authorization_details: None,
                            },
                            details: None,
                            code: None,
                        },
                        account: Account {
                            sub: Sub::new("sub1").unwrap(),
                            aud: Audience::Single("did:web:pds.ripperoni.com".to_string()),
                            preferred_username: None,
                            email: None,
                            email_verified: None,
                            picture: None,
                            name: None,
                        },
                        info: None,
                        current_refresh_token: None,
                    }))
                } else {
                    panic!()
                }
            })
        }

        fn find_token_by_code(
            &self,
            code: Code,
        ) -> Pin<Box<dyn Future<Output = Result<Option<TokenInfo>, OAuthError>> + Send + Sync + '_>>
        {
            unimplemented!()
        }
    }

    impl RequestStore for TestStore {
        fn create_request(
            &mut self,
            id: RequestId,
            data: RequestData,
        ) -> Pin<Box<dyn Future<Output = Result<(), OAuthError>> + Send + Sync + '_>> {
            unimplemented!()
        }

        fn read_request(
            &self,
            id: &RequestId,
        ) -> Pin<Box<dyn Future<Output = Result<Option<RequestData>, OAuthError>> + Send + Sync + '_>>
        {
            let id = id.clone();
            Box::pin(async move {
                if id == RequestId::new("req-f46e8a935aa5343574848e8a3c260fae").unwrap() {
                    Ok(Some(RequestData {
                        client_id: OAuthClientId::new(
                            "https://cleanfollow-bsky.pages.dev/client-metadata.json",
                        )
                        .unwrap(),
                        client_auth: ClientAuth {
                            method: "".to_string(),
                            alg: "".to_string(),
                            kid: "".to_string(),
                            jkt: "".to_string(),
                        },
                        parameters: OAuthAuthorizationRequestParameters {
                            client_id: OAuthClientId::new(
                                "https://cleanfollow-bsky.pages.dev/client-metadata.json",
                            )
                            .unwrap(),
                            state: None,
                            redirect_uri: None,
                            scope: None,
                            response_type: OAuthResponseType::Code,
                            code_challenge: None,
                            code_challenge_method: None,
                            dpop_jkt: None,
                            response_mode: None,
                            nonce: None,
                            max_age: None,
                            claims: None,
                            login_hint: None,
                            ui_locales: None,
                            id_token_hint: None,
                            display: None,
                            prompt: None,
                            authorization_details: None,
                        },
                        expires_at: Utc::now(),
                        device_id: None,
                        sub: None,
                        code: None,
                    }))
                } else {
                    panic!()
                }
            })
        }

        fn update_request(
            &mut self,
            id: RequestId,
            data: UpdateRequestData,
        ) -> Pin<Box<dyn Future<Output = Result<(), OAuthError>> + Send + Sync + '_>> {
            let id = id;
            Box::pin(async move {
                if id == RequestId::new("req-f46e8a935aa5343574848e8a3c260fae").unwrap() {
                    Ok(())
                } else {
                    panic!()
                }
            })
        }

        fn delete_request(
            &mut self,
            id: RequestId,
        ) -> Pin<Box<dyn Future<Output = Result<(), OAuthError>> + Send + Sync + '_>> {
            let id = id;
            Box::pin(async move {
                if id == RequestId::new("req-f46e8a935aa5343574848e8a3c260fae").unwrap() {
                    Ok(())
                } else {
                    panic!()
                }
            })
        }

        fn find_request_by_code(
            &self,
            code: Code,
        ) -> Pin<Box<dyn Future<Output = Option<FoundRequestResult>> + Send + Sync + '_>> {
            unimplemented!()
        }
    }

    impl ClientStore for TestStore {
        fn find_client(&self, client_id: OAuthClientId) -> Result<OAuthClientMetadata, OAuthError> {
            unimplemented!()
        }
    }

    impl ReplayStore for TestStore {
        fn unique(&mut self, namespace: &str, nonce: &str, timeframe: f64) -> bool {
            unimplemented!()
        }
    }

    fn create_client_metadata() -> OAuthClientMetadata {
        OAuthClientMetadata {
            client_id: Some("https://cleanfollow-bsky.pages.dev/client-metadata.json".to_string()),
            client_name: Some("cleanfollow-bsky".to_string()),
            client_uri: Some(WebUri::validate("https://cleanfollow-bsky.pages.dev").unwrap()),
            redirect_uris: vec![
                OAuthRedirectUri::new("https://cleanfollow-bsky.pages.dev/").unwrap()
            ],
            scope: Some(OAuthScope::new("atproto transition:generic").unwrap()),
            grant_types: vec![
                OAuthGrantType::AuthorizationCode,
                OAuthGrantType::RefreshToken,
            ],
            response_types: vec![OAuthResponseType::Code],
            dpop_bound_access_tokens: Some(true),
            token_endpoint_auth_method: Some(OAuthEndpointAuthMethod::None),
            application_type: ApplicationType::Web,
            ..Default::default()
        }
    }

    async fn create_signer() -> Signer {
        let jwk = Jwk {
            common: CommonParameters {
                public_key_use: Some(PublicKeyUse::Signature),
                key_algorithm: Some(KeyAlgorithm::ES256),
                ..Default::default()
            },
            algorithm: AlgorithmParameters::EllipticCurve(EllipticCurveKeyParameters {
                key_type: Default::default(),
                curve: Default::default(),
                x: "vf9j5yujiO25FukCWswD9GFGU30xwm6D6JlVIp40FUU".to_string(),
                y: "5EqgG67c-QjyCgHmhiq65kjqEo0Wig8a97h322vTtq4".to_string(),
            }),
        };
        let jose_key = JoseKey::from_jwk(jwk, None).await;
        let issuer = OAuthIssuerIdentifier::new("http://pds.ripperoni.com").unwrap();
        let keyset = Keyset::new(vec![Box::new(jose_key)]);
        let keyset = Arc::new(RwLock::new(keyset));

        let token = SignedJwt::new("eyJ0eXAiOiJKV1QiLCJhbGciOiJSUzI1NiIsImtpZCI6Ik5FTXlNRUZDTXpVd01URTFRVE5CT1VGRE1FUTFPRGN6UmprNU56QkdRelk0UVRrMVEwWkVPUSJ9.eyJpc3MiOiJodHRwczovL2Rldi1lanRsOTg4dy5hdXRoMC5jb20vIiwic3ViIjoiZ1pTeXNwQ1k1ZEk0aDFaM3Fwd3BkYjlUNFVQZEdENWtAY2xpZW50cyIsImF1ZCI6Imh0dHA6Ly9oZWxsb3dvcmxkIiwiaWF0IjoxNTcyNDA2NDQ3LCJleHAiOjE1NzI0OTI4NDcsImF6cCI6ImdaU3lzcENZNWRJNGgxWjNxcHdwZGI5VDRVUGRHRDVrIiwiZ3R5IjoiY2xpZW50LWNyZWRlbnRpYWxzIn0.nupgm7iFqSnERq9GxszwBrsYrYfMuSfUGj8tGQlkY3Ksh3o_IDfq1GO5ngHQLZuYPD-8qPIovPBEVomGZCo_jYvsbjmYkalAStmF01TvSoXQgJd09ygZstH0liKsmINStiRE8fTA-yfEIuBYttROizx-cDoxiindbKNIGOsqf6yOxf7ww8DrTBJKYRnHVkAfIK8wm9LRpsaOVzWdC7S3cbhCKvANjT0RTRpAx8b_AOr_UCpOr8paj-xMT9Zc9HVCMZLBfj6OZ6yVvnC9g6q_SlTa--fY9SL5eqy6-q1JGoyK_-BQ_YrCwrRdrjoJsJ8j-XFRFWJX09W3oDuZ990nGA").unwrap();

        Signer::new(issuer, keyset)
    }

    async fn create_request_manager() -> RequestManager {
        let store: Arc<RwLock<dyn RequestStore>> = Arc::new(RwLock::new(TestStore {}));
        let signer: Arc<RwLock<Signer>> = Arc::new(RwLock::new(create_signer().await));
        let access_token_type = AccessTokenType::JWT;
        let max_age = Some(TOKEN_MAX_AGE);
        let oauth_hooks = OAuthHooks {
            on_client_info: Some(Box::new(
                |client_id: OAuthClientId,
                 oauth_client_metadata: OAuthClientMetadata,
                 jwks: Option<JwkSet>|
                 -> ClientInfo {
                    ClientInfo {
                        is_first_party: client_id
                            == OAuthClientId::new(
                                "https://cleanfollow-bsky.pages.dev/client-metadata.json",
                            )
                            .unwrap(),
                        // @TODO make client client list configurable:
                        is_trusted: false,
                    }
                },
            )),
            on_authorization_details: None,
        };
        let metadata = OAuthAuthorizationServerMetadata::new(
            OAuthIssuerIdentifier::new("https://pds.ripperoni.com").unwrap(),
            WebUri::validate("https://pds.ripperoni.com/oauth/authorize").unwrap(),
            WebUri::validate("https://pds.ripperoni.com/oauth/token").unwrap(),
        );
        RequestManager::new(
            store,
            signer,
            metadata,
            max_age.unwrap(),
            Arc::new(oauth_hooks),
        )
    }

    async fn create_oauth_provider() -> OAuthProvider {
        let keyset = Arc::new(RwLock::new(build_keyset().await));
        let oauth_hooks = OAuthHooks {
            on_client_info: Some(Box::new(
                |client_id: OAuthClientId,
                 oauth_client_metadata: OAuthClientMetadata,
                 jwks: Option<JwkSet>|
                 -> ClientInfo {
                    ClientInfo {
                        is_first_party: client_id
                            == OAuthClientId::new("https://pds.ripperoni.com/client-metadata.json")
                                .unwrap(),
                        // @TODO make client client list configurable:
                        is_trusted: false,
                    }
                },
            )),
            on_authorization_details: None,
        };
        let issuer = OAuthIssuerIdentifier::new("https://pds.ripperoni.com").unwrap();
        let custom_metadata = build_custom_metadata(issuer.clone());
        let options = OAuthProviderOptions {
            authentication_max_age: None,
            token_max_age: None,
            metadata: Some(custom_metadata),
            customization: None,
            safe_fetch: false,
            redis: None,
            account_store: Arc::new(RwLock::new(TestStore {})),
            device_store: Arc::new(RwLock::new(TestStore {})),
            client_store: Some(Arc::new(RwLock::new(TestStore {}))),
            replay_store: Some(Arc::new(RwLock::new(TestStore {}))),
            request_store: Some(Arc::new(RwLock::new(TestStore {}))),
            token_store: Arc::new(RwLock::new(TestStore {})),
            client_jwks_cache: Arc::new(Default::default()),
            client_metadata_cache: Arc::new(Default::default()),
            loopback_metadata: None,
            dpop_secret: None,
            dpop_step: None,
            issuer: issuer.clone(),
            keyset: Some(keyset),
            access_token_type: Some(AccessTokenType::ID),
            oauth_hooks: Arc::new(oauth_hooks),
        };
        OAuthProvider::new(options).unwrap()
    }

    fn build_custom_metadata(issuer: OAuthIssuerIdentifier) -> CustomMetadata {
        CustomMetadata {
            scopes_supported: Some(vec![
                "transition:generic".to_string(),
                "transition:chat.bsky".to_string(),
            ]),
            authorization_details_type_supported: None,
            protected_resources: Some(vec![WebUri::validate(issuer.as_ref()).unwrap()]),
        }
    }

    #[tokio::test]
    async fn test_get_jwks() {
        let oauth_provider = create_oauth_provider().await;
        let result = oauth_provider.get_jwks().await;

        let jwk = Jwk {
            common: CommonParameters {
                public_key_use: Some(PublicKeyUse::Signature),
                key_operations: Some(vec![KeyOperations::Sign]),
                key_algorithm: Some(KeyAlgorithm::PS256),
                key_id: Some("test".to_string()),
                x509_url: None,
                x509_chain: None,
                x509_sha1_fingerprint: None,
                x509_sha256_fingerprint: None,
            },
            algorithm: AlgorithmParameters::EllipticCurve(EllipticCurveKeyParameters {
                key_type: EllipticCurveKeyType::EC,
                curve: EllipticCurve::P256,
                x: "GgskXhf9OJFxYNovWiwq35akQopFXS6Tzuv0Y-B6q8I".to_string(),
                y: "Cv8TnJVvra7TmYsaO-_nwhpD2jpfdnRE_TAeuvxLgJE".to_string(),
            }),
        };

        assert_eq!(result, vec![jwk]);
    }

    #[tokio::test]
    async fn test_pushed_authorization_request() {
        let mut oauth_provider = create_oauth_provider().await;
        let credentials = OAuthClientCredentialsJwtBearer::new(
            OAuthClientId::new("https://cleanfollow-bsky.pages.dev/client-metadata.json").unwrap(),
            "eyJ0eXAiOiJkcG9wK2p3dCIsImFsZyI6IkVTMjU2IiwiandrIjp7ImFsZyI6IkVTMjU2IiwiY3J2IjoiUC0yNTYiLCJrdHkiOiJFQyIsIngiOiJWSEw4aHhxbVFoQlVRUDBZSGNfZ1Z5YnFCREtTeDg3RTU5dnp0SUdqSk1BIiwieSI6ImFCQmpJMnBsZFIyOTM5UEJFby03Y1hBWlRyNUhyQ1hHeWx6VGkxSkN5cGMifX0.eyJpc3MiOiJodHRwczovL2NsZWFuZm9sbG93LWJza3kucGFnZXMuZGV2L2NsaWVudC1tZXRhZGF0YS5qc29uIiwiaWF0IjoxNzQ0NjA1MTgzLCJqdGkiOiJoNmVzazFxMmk4OnlkcGg0d2lpeGhpaiIsImh0bSI6IlBPU1QiLCJodHUiOiJodHRwczovL3Bkcy5yaXBwZXJvbmkuY29tL29hdXRoL3BhciIsIm5vbmNlIjoid1hOQV8zbzdyRndfenZ5ek4wcDFWblFkTzJlSEN6cksxcGJQa3diTWpPYyJ9.kiNqo-6CmJzee-5MMuCNcRft_J3eVvdfC91mO2MvsBeNaeSmC8TR6PM8KCeCx8DM2wexmTNgDc8a85fvW82uzA",
        )
            .unwrap();
        let parameters = OAuthAuthorizationRequestParameters {
            client_id: OAuthClientId::new(
                "https://cleanfollow-bsky.pages.dev/client-metadata.json",
            )
            .unwrap(),
            state: Some("1ZXHAvyXLYoYV90WDHDVcg".to_string()),
            redirect_uri: Some(
                OAuthRedirectUri::new("https://cleanfollow-bsky.pages.dev/").unwrap(),
            ),
            scope: Some(OAuthScope::new("atproto transition:generic").unwrap()),
            response_type: OAuthResponseType::Code,
            code_challenge: Some("iR8-lgCjZHJvuQmdxdq3wTPlDCJ902KGKRjjyQF0UTQ".to_string()),
            code_challenge_method: Some(OAuthCodeChallengeMethod::S256),
            dpop_jkt: None,
            response_mode: Some(ResponseMode::Fragment),
            nonce: None,
            max_age: None,
            claims: None,
            login_hint: Some("ripperoni.com".to_string()),
            ui_locales: None,
            id_token_hint: None,
            display: Some(Display::Page),
            prompt: None,
            authorization_details: None,
        };
        let authorization_request = OAuthAuthorizationRequestPar::Parameters(parameters);
        let dpop_jkt = Some("".to_string());
        oauth_provider
            .pushed_authorization_request(
                OAuthClientCredentials::JwtBearer(credentials),
                authorization_request,
                dpop_jkt,
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_authorize() {
        let mut oauth_provider = create_oauth_provider().await;
        let device_id = DeviceId::generate();
        let credentials = OAuthClientCredentials::JwtBearer(
            OAuthClientCredentialsJwtBearer::new(OAuthClientId::new("https://cleanfollow-bsky.pages.dev/client-metadata.json").unwrap(), "eyJ0eXAiOiJkcG9wK2p3dCIsImFsZyI6IkVTMjU2IiwiandrIjp7ImFsZyI6IkVTMjU2IiwiY3J2IjoiUC0yNTYiLCJrdHkiOiJFQyIsIngiOiJFTEpEcHJfazF1Mlhza0w3SjVOb1NmVlJvUGgyeGpKY3d6OWIybTBPZzBjIiwieSI6Ild2RTdHdTRSVFFHQ2std2hic1ZZakpiaWlWLU82MmgxVXBxY25BMDR2VzAifX0.eyJpc3MiOiJodHRwczovL2NsZWFuZm9sbG93LWJza3kucGFnZXMuZGV2L2NsaWVudC1tZXRhZGF0YS5qc29uIiwiaWF0IjoxNzQ0NjIwMTM0LCJqdGkiOiJoNmV6ZmI5emJjOjJ3Zzh2eWNuamMweHAiLCJodG0iOiJQT1NUIiwiaHR1IjoiaHR0cHM6Ly9wZHMucmlwcGVyb25pLmNvbS9vYXV0aC9wYXIiLCJub25jZSI6ImhnbTFOV0pqSS00cm8zdFhTejNfaE1mMGpmTlZRb0prSFNORWwxUU9PNlEifQ.Qvj1kj91a7Rz4LTCgYo90_6kmBKa7OaXQvD7nrDyhYWgNoiCUEY4pZ20amfFRXsDv1YE4MLP0Qk4mqJgmZCc4Q")
                .unwrap(),
        );
        let parameters = OAuthAuthorizationRequestParameters {
            client_id: OAuthClientId::new(
                "https://cleanfollow-bsky.pages.dev/client-metadata.json",
            )
            .unwrap(),
            state: Some("1ZXHAvyXLYoYV90WDHDVcg".to_string()),
            redirect_uri: Some(
                OAuthRedirectUri::new("https://cleanfollow-bsky.pages.dev/").unwrap(),
            ),
            scope: Some(OAuthScope::new("atproto transition:generic").unwrap()),
            response_type: OAuthResponseType::Code,
            code_challenge: Some("iR8-lgCjZHJvuQmdxdq3wTPlDCJ902KGKRjjyQF0UTQ".to_string()),
            code_challenge_method: Some(OAuthCodeChallengeMethod::S256),
            dpop_jkt: None,
            response_mode: Some(ResponseMode::Fragment),
            nonce: None,
            max_age: None,
            claims: None,
            login_hint: Some("ripperoni.com".to_string()),
            ui_locales: None,
            id_token_hint: None,
            display: Some(Display::Page),
            prompt: None,
            authorization_details: None,
        };
        let query = OAuthAuthorizationRequestQuery::from_parameters(parameters.clone());
        let result = oauth_provider
            .authorize(&device_id, &credentials, &query)
            .await
            .unwrap();
        let expected = AuthorizationResult::Authorize(AuthorizationResultAuthorize {
            issuer: OAuthIssuerIdentifier::new("https://pds.ripperoni.com").unwrap(),
            client: Client::new(
                OAuthClientId::new("https://cleanfollow-bsky.pages.dev/client-metadata.json")
                    .unwrap(),
                OAuthClientMetadata {
                    redirect_uris: vec![],
                    response_types: vec![],
                    grant_types: vec![],
                    scope: None,
                    token_endpoint_auth_method: None,
                    token_endpoint_auth_signing_alg: None,
                    userinfo_signed_response_alg: None,
                    userinfo_encrypted_response_alg: None,
                    jwks_uri: None,
                    jwks: None,
                    application_type: Default::default(),
                    subject_type: None,
                    request_object_signing_alg: None,
                    id_token_signed_response_alg: None,
                    authorization_signed_response_alg: "".to_string(),
                    authorization_encrypted_response_enc: None,
                    authorization_encrypted_response_alg: None,
                    client_id: None,
                    client_name: None,
                    client_uri: None,
                    policy_uri: None,
                    tos_uri: None,
                    logo_uri: None,
                    default_max_age: None,
                    require_auth_time: None,
                    contacts: None,
                    tls_client_certificate_bound_access_tokens: None,
                    dpop_bound_access_tokens: None,
                    authorization_details_types: None,
                },
                None,
                Default::default(),
            ),
            parameters,
            authorize: Authorize {
                uri: RequestUri::new(
                    "urn:ietf:params:oauth:request_uri:req-f776c3fd9760348fc92e1600448b71a9",
                )
                .unwrap(),
                scope_details: None,
                sessions: vec![],
            },
        });
        assert_eq!(result, expected);
    }

    #[tokio::test]
    async fn test_get_sessions() {
        let oauth_provider = create_oauth_provider().await;
        let id =
            OAuthClientId::new("https://cleanfollow-bsky.pages.dev/client-metadata.json").unwrap();
        let metadata = OAuthClientMetadata {
            redirect_uris: vec![
                OAuthRedirectUri::new("https://cleanfollow-bsky.pages.dev/").unwrap()
            ],
            response_types: vec![OAuthResponseType::Code],
            grant_types: vec![
                OAuthGrantType::AuthorizationCode,
                OAuthGrantType::RefreshToken,
            ],
            scope: Some(OAuthScope::new("atproto transition:generic").unwrap()),
            token_endpoint_auth_method: Some(OAuthEndpointAuthMethod::None),
            token_endpoint_auth_signing_alg: None,
            userinfo_signed_response_alg: None,
            userinfo_encrypted_response_alg: None,
            jwks_uri: None,
            jwks: None,
            application_type: ApplicationType::Web,
            subject_type: None,
            request_object_signing_alg: None,
            id_token_signed_response_alg: None,
            authorization_signed_response_alg: "".to_string(),
            authorization_encrypted_response_enc: None,
            authorization_encrypted_response_alg: None,
            client_id: None,
            client_name: None,
            client_uri: None,
            policy_uri: None,
            tos_uri: None,
            logo_uri: None,
            default_max_age: None,
            require_auth_time: None,
            contacts: None,
            tls_client_certificate_bound_access_tokens: None,
            dpop_bound_access_tokens: Some(true),
            authorization_details_types: None,
        };
        let info = ClientInfo {
            is_first_party: false,
            is_trusted: false,
        };
        let client = Client::new(id, metadata, None, info);
        let device_id = DeviceId::generate();
        let parameters = OAuthAuthorizationRequestParameters {
            client_id: OAuthClientId::new(
                "https://cleanfollow-bsky.pages.dev/client-metadata.json",
            )
            .unwrap(),
            state: None,
            redirect_uri: None,
            scope: None,
            response_type: OAuthResponseType::Code,
            code_challenge: None,
            code_challenge_method: None,
            dpop_jkt: None,
            response_mode: None,
            nonce: None,
            max_age: None,
            claims: None,
            login_hint: None,
            ui_locales: None,
            id_token_hint: None,
            display: None,
            prompt: None,
            authorization_details: None,
        };
        let result = oauth_provider
            .get_sessions(&client, &device_id, &parameters)
            .await
            .unwrap();
        let expected = vec![];
        assert_eq!(result, expected)
    }

    #[tokio::test]
    async fn test_sign_in() {
        let mut oauth_provider = create_oauth_provider().await;
        let device_id = DeviceId::generate();
        let uri = RequestUri::new(
            "urn:ietf:params:oauth:request_uri:req-f46e8a935aa5343574848e8a3c260fae",
        )
        .unwrap();
        let client_id =
            OAuthClientId::new("https://cleanfollow-bsky.pages.dev/client-metadata.json").unwrap();
        let credentials = SignInCredentials {
            username: "".to_string(),
            password: "".to_string(),
            remember: None,
            email_otp: None,
        };
        let result = oauth_provider
            .sign_in(device_id, uri, client_id, credentials)
            .await
            .unwrap();
        let expected = SignInResponse {
            account: Account {
                sub: Sub::new("did:plc:khvyd3oiw46vif5gm7hijslk").unwrap(),
                aud: Audience::Single("did:web:pds.ripperoni.com".to_string()),
                preferred_username: None,
                email: None,
                email_verified: None,
                picture: None,
                name: None,
            },
            consent_required: true,
        };
        assert_eq!(result, expected)
    }

    #[tokio::test]
    async fn test_accept_request() {
        let mut oauth_provider = create_oauth_provider().await;
        let device_id = DeviceId::generate();
        let uri = RequestUri::new(
            "urn:ietf:params:oauth:request_uri:req-f776c3fd9760348fc92e1600448b71a9",
        )
        .unwrap();
        let client_id =
            OAuthClientId::new("https://cleanfollow-bsky.pages.dev/client-metadata.json").unwrap();
        let sub = Sub::new("sub").unwrap();
        let result = oauth_provider
            .accept_request(device_id, uri, client_id.clone(), sub)
            .await
            .unwrap();
        let expected = AuthorizationResultRedirect {
            issuer: OAuthIssuerIdentifier::new("https://pds.ripperoni.com").unwrap(),
            parameters: OAuthAuthorizationRequestParameters {
                client_id: client_id.clone(),
                state: None,
                redirect_uri: None,
                scope: None,
                response_type: OAuthResponseType::Code,
                code_challenge: None,
                code_challenge_method: None,
                dpop_jkt: None,
                response_mode: None,
                nonce: None,
                max_age: None,
                claims: None,
                login_hint: None,
                ui_locales: None,
                id_token_hint: None,
                display: None,
                prompt: None,
                authorization_details: None,
            },
            redirect: AuthorizationResponseParameters {
                code: None,
                id_token: None,
                access_token: None,
                token_type: None,
                expires_in: None,
                response: None,
                session_state: None,
                error: None,
                error_description: None,
                error_uri: None,
            },
        };
        assert_eq!(result, expected)
    }

    #[tokio::test]
    async fn test_reject_request() {
        let mut oauth_provider = create_oauth_provider().await;
        let device_id = DeviceId::generate();
        let uri = RequestUri::new("").unwrap();
        let client_id =
            OAuthClientId::new("https://cleanfollow-bsky.pages.dev/client-metadata.json").unwrap();
        let result = oauth_provider
            .reject_request(device_id.clone(), uri.clone(), client_id.clone())
            .await
            .unwrap();
        let expected = AuthorizationResultRedirect {
            issuer: OAuthIssuerIdentifier::new("https://pds.ripperoni.com").unwrap(),
            parameters: OAuthAuthorizationRequestParameters {
                client_id: client_id.clone(),
                state: None,
                redirect_uri: None,
                scope: None,
                response_type: OAuthResponseType::Code,
                code_challenge: None,
                code_challenge_method: None,
                dpop_jkt: None,
                response_mode: None,
                nonce: None,
                max_age: None,
                claims: None,
                login_hint: None,
                ui_locales: None,
                id_token_hint: None,
                display: None,
                prompt: None,
                authorization_details: None,
            },
            redirect: AuthorizationResponseParameters {
                code: None,
                id_token: None,
                access_token: None,
                token_type: None,
                expires_in: None,
                response: None,
                session_state: None,
                error: None,
                error_description: None,
                error_uri: None,
            },
        };
        assert_eq!(result, expected)
    }

    #[tokio::test]
    async fn test_code_grant() {
        let mut oauth_provider = create_oauth_provider().await;
        let client = create_client();
        let client_auth = ClientAuth {
            method: "urn:ietf:params:oauth:client-assertion-type:jwt-bearer".to_string(),
            alg: "".to_string(),
            kid: "".to_string(),
            jkt: "".to_string(),
        };
        let input = OAuthAuthorizationCodeGrantTokenRequest::new(
            Code::generate(),
            OAuthRedirectUri::new("https://cleanfollow-bsky.pages.dev/").unwrap(),
            None::<String>,
        )
        .unwrap();
        let dpop_jkt: Option<String> = Some("".to_string());
        let result = oauth_provider
            .code_grant(client, client_auth, input, dpop_jkt)
            .await
            .unwrap();
        let expected = OAuthTokenResponse {
            access_token: OAuthAccessToken::new("").unwrap(),
            token_type: OAuthTokenType::DPoP,
            scope: None,
            refresh_token: None,
            expires_in: None,
            id_token: None,
            authorization_details: None,
            additional_fields: Default::default(),
        };
        assert_eq!(result, expected)
    }

    #[tokio::test]
    async fn test_refresh_token_grant() {
        let mut oauth_provider = create_oauth_provider().await;
        let client = create_client();
        let client_auth = ClientAuth {
            method: "urn:ietf:params:oauth:client-assertion-type:jwt-bearer".to_string(),
            alg: "".to_string(),
            kid: "".to_string(),
            jkt: "".to_string(),
        };
        let input = OAuthRefreshTokenGrantTokenRequest::new(OAuthRefreshToken::new("").unwrap());
        let dpop_jkt: Option<String> = Some("".to_string());
        let result = oauth_provider
            .refresh_token_grant(client, client_auth, input, dpop_jkt)
            .await
            .unwrap();
        let expected = OAuthTokenResponse {
            access_token: OAuthAccessToken::new("").unwrap(),
            token_type: OAuthTokenType::DPoP,
            scope: None,
            refresh_token: None,
            expires_in: None,
            id_token: None,
            authorization_details: None,
            additional_fields: Default::default(),
        };
        assert_eq!(result, expected)
    }

    #[tokio::test]
    async fn test_revoke() {
        let mut oauth_provider = create_oauth_provider().await;
        let token = OAuthTokenIdentification::new(
            "ref-a41a16a716951a211b9ba177b121ce469128fb4eeb4cea6f10fd949014ab48c4",
            None,
        )
        .unwrap();
        oauth_provider.revoke(token).await.unwrap();
    }

    #[tokio::test]
    async fn test_introspect() {
        let mut oauth_provider = create_oauth_provider().await;
        let client_id =
            OAuthClientId::new("https://cleanfollow-bsky.pages.dev/client-metadata.json").unwrap();
        let client_assertion = "eyJ0eXAiOiJkcG9wK2p3dCIsImFsZyI6IkVTMjU2IiwiandrIjp7ImFsZyI6IkVTMjU2IiwiY3J2IjoiUC0yNTYiLCJrdHkiOiJFQyIsIngiOiJFTEpEcHJfazF1Mlhza0w3SjVOb1NmVlJvUGgyeGpKY3d6OWIybTBPZzBjIiwieSI6Ild2RTdHdTRSVFFHQ2std2hic1ZZakpiaWlWLU82MmgxVXBxY25BMDR2VzAifX0.eyJpc3MiOiJodHRwczovL2NsZWFuZm9sbG93LWJza3kucGFnZXMuZGV2L2NsaWVudC1tZXRhZGF0YS5qc29uIiwiaWF0IjoxNzQ0NjIwMTM0LCJqdGkiOiJoNmV6ZmI5emJjOjJ3Zzh2eWNuamMweHAiLCJodG0iOiJQT1NUIiwiaHR1IjoiaHR0cHM6Ly9wZHMucmlwcGVyb25pLmNvbS9vYXV0aC9wYXIiLCJub25jZSI6ImhnbTFOV0pqSS00cm8zdFhTejNfaE1mMGpmTlZRb0prSFNORWwxUU9PNlEifQ.Qvj1kj91a7Rz4LTCgYo90_6kmBKa7OaXQvD7nrDyhYWgNoiCUEY4pZ20amfFRXsDv1YE4MLP0Qk4mqJgmZCc4Q";
        let credentials = OAuthClientCredentials::JwtBearer(
            OAuthClientCredentialsJwtBearer::new(client_id, client_assertion).unwrap(),
        );

        let token =
            OAuthTokenIdentification::new("test", Some(TokenTypeHint::AccessToken)).unwrap();
        let result = oauth_provider.introspect(credentials, token).await.unwrap();
        let expected = OAuthIntrospectionResponse::Inactive;
        assert_eq!(result, expected);
    }

    fn create_client() -> Client {
        let id =
            OAuthClientId::new("https://cleanfollow-bsky.pages.dev/client-metadata.json").unwrap();
        let metadata = OAuthClientMetadata {
            redirect_uris: vec![],
            response_types: vec![],
            grant_types: vec![],
            scope: None,
            token_endpoint_auth_method: None,
            token_endpoint_auth_signing_alg: None,
            userinfo_signed_response_alg: None,
            userinfo_encrypted_response_alg: None,
            jwks_uri: None,
            jwks: None,
            application_type: Default::default(),
            subject_type: None,
            request_object_signing_alg: None,
            id_token_signed_response_alg: None,
            authorization_signed_response_alg: "".to_string(),
            authorization_encrypted_response_enc: None,
            authorization_encrypted_response_alg: None,
            client_id: None,
            client_name: None,
            client_uri: None,
            policy_uri: None,
            tos_uri: None,
            logo_uri: None,
            default_max_age: None,
            require_auth_time: None,
            contacts: None,
            tls_client_certificate_bound_access_tokens: None,
            dpop_bound_access_tokens: None,
            authorization_details_types: None,
        };
        let jwks = JwkSet { keys: vec![] };
        let info = ClientInfo {
            is_first_party: false,
            is_trusted: false,
        };
        Client::new(id, metadata, None, info)
    }

    async fn build_keyset() -> Keyset {
        let mut keys = Vec::new();
        let jwk = Jwk {
            common: CommonParameters {
                public_key_use: Some(PublicKeyUse::Signature),
                key_operations: Some(vec![KeyOperations::Sign]),
                key_algorithm: Some(KeyAlgorithm::PS256),
                key_id: None,
                x509_url: None,
                x509_chain: None,
                x509_sha1_fingerprint: None,
                x509_sha256_fingerprint: None,
            },
            algorithm: AlgorithmParameters::EllipticCurve(EllipticCurveKeyParameters {
                key_type: EllipticCurveKeyType::EC,
                curve: EllipticCurve::P256,
                x: "GgskXhf9OJFxYNovWiwq35akQopFXS6Tzuv0Y-B6q8I".to_string(),
                y: "Cv8TnJVvra7TmYsaO-_nwhpD2jpfdnRE_TAeuvxLgJE".to_string(),
            }),
        };
        let key = JoseKey::from_jwk(jwk, None).await;
        keys.push(Box::new(key) as Box<dyn Key>);
        Keyset::new(keys)
    }
}
