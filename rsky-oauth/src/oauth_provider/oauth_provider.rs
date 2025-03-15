use crate::jwk::Keyset;
use crate::oauth_provider::access_token::access_token_type::AccessTokenType;
use crate::oauth_provider::account::account::Account;
use crate::oauth_provider::account::account_manager::AccountManager;
use crate::oauth_provider::account::account_store::{
    AccountStore, DeviceAccountInfo, SignInCredentials,
};
use crate::oauth_provider::client::client::Client;
use crate::oauth_provider::client::client_auth::ClientAuth;
use crate::oauth_provider::client::client_id::ClientId;
use crate::oauth_provider::client::client_manager::ClientManager;
use crate::oauth_provider::client::client_store::ClientStore;
use crate::oauth_provider::constants::AUTHENTICATION_MAX_AGE;
use crate::oauth_provider::device::device_id::DeviceId;
use crate::oauth_provider::device::device_store::DeviceStore;
use crate::oauth_provider::dpop::dpop_nonce::DpopNonceInput;
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_provider::metadata::build_metadata::{build_metadata, CustomMetadata};
use crate::oauth_provider::now_as_secs;
use crate::oauth_provider::oauth_verifier::{OAuthVerifier, OAuthVerifierOptions};
use crate::oauth_provider::oidc::sub::Sub;
use crate::oauth_provider::output::customization::Customization;
use crate::oauth_provider::output::send_authorize_redirect::AuthorizationResult;
use crate::oauth_provider::replay::replay_store_memory::ReplayStoreMemory;
use crate::oauth_provider::request::code::{parse, Code};
use crate::oauth_provider::request::request_info::RequestInfo;
use crate::oauth_provider::request::request_manager::RequestManager;
use crate::oauth_provider::request::request_store_memory::RequestStoreMemory;
use crate::oauth_provider::request::request_uri::RequestUri;
use crate::oauth_provider::token::token_id::is_token_id;
use crate::oauth_provider::token::token_manager::TokenManager;
use crate::oauth_provider::token::token_store::{TokenInfo, TokenStore};
use crate::oauth_provider::token::verify_token_claims::{
    VerifyTokenClaimsOptions, VerifyTokenClaimsResult,
};
use crate::oauth_types::{
    ActiveTokenInfo, ApplicationType, OAuthAccessToken, OAuthAuthorizationCodeGrantTokenRequest,
    OAuthAuthorizationRequestJar, OAuthAuthorizationRequestPar,
    OAuthAuthorizationRequestParameters, OAuthAuthorizationRequestQuery,
    OAuthAuthorizationServerMetadata, OAuthClientCredentials, OAuthClientCredentialsNone,
    OAuthClientMetadata, OAuthIntrospectionResponse, OAuthIssuerIdentifier, OAuthParResponse,
    OAuthRefreshTokenGrantTokenRequest, OAuthRequestUri, OAuthTokenIdentification,
    OAuthTokenRequest, OAuthTokenResponse, OAuthTokenType,
};
use jsonwebtoken::jwk::{Jwk, JwkSet};
use rocket::form::validate::Contains;
use rocket::uri;
use std::any::Any;
use std::collections::BTreeMap;
use url::quirks::username;

pub struct OAuthProviderSession {
    account: Account,
    info: DeviceAccountInfo,
    selected: bool,
    login_required: bool,
    consent_required: bool,
    matches_hint: bool,
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

pub struct SignInResponse {
    pub account: Account,
    pub consent_required: bool,
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
    pub redis: String,

    /**
     * This will be used as the default store for all the stores. If a store is
     * not provided, this store will be used instead. If the `store` does not
     * implement a specific store, a runtime error will be thrown. Make sure that
     * this store implements all the interfaces not provided in the other
     * `<name>Store` options.
     */
    // pub store: Option<OAuthProviderStore>,
    pub account_store: AccountStore,
    pub device_store: DeviceStore,
    pub client_store: ClientStore,
    pub replay_store: ReplayStoreMemory,
    pub request_store: RequestStoreMemory,
    pub token_store: TokenStore,

    /**
     * In order to speed up the client fetching process, you can provide a cache
     * to store HTTP responses.
     *
     * @note the cached entries should automatically expire after a certain time (typically 10 minutes)
     */
    pub client_jwks_cache: Option<BTreeMap<String, JwkSet>>,

    /**
     * In order to speed up the client fetching process, you can provide a cache
     * to store HTTP responses.
     *
     * @note the cached entries should automatically expire after a certain time (typically 10 minutes)
     */
    pub client_metadata_cache: Option<BTreeMap<String, OAuthClientMetadata>>,

    /**
     * In order to enable loopback clients, you can provide a function that
     * returns the client metadata for a given loopback URL. This is useful for
     * development and testing purposes. This function is not called for internet
     * clients.
     *
     * @default is as specified by ATPROTO
     */
    pub loopback_metadata: String,
    pub dpop_secret: Option<DpopNonceInput>,
    pub dpop_step: Option<u64>,
    pub issuer: OAuthIssuerIdentifier,
    pub keyset: Option<Keyset>,
    pub access_token_type: Option<AccessTokenType>,
}

pub struct OAuthProvider {
    pub metadata: OAuthAuthorizationServerMetadata,
    customization: Option<Customization>,

    authentication_max_age: u64,

    client_manager: ClientManager,
    request_manager: RequestManager,
    token_manager: TokenManager,
    pub oauth_verifier: OAuthVerifier,
    device_store: DeviceStore,

    account_manager: AccountManager,
}

impl OAuthProvider {
    pub fn new(options: OAuthProviderOptions) -> Result<Self, OAuthError> {
        let verifier_opts = OAuthVerifierOptions {
            issuer: options.issuer.clone(),
            keyset: options.keyset.unwrap(),
            access_token_type: options.access_token_type,
            redis: Some(options.redis),
            replay_store: Some(options.replay_store),
        };
        let oauth_verifier = OAuthVerifier::new(verifier_opts);

        let request_store = RequestStoreMemory::new();

        let authentication_max_age = options
            .authentication_max_age
            .unwrap_or_else(|| AUTHENTICATION_MAX_AGE);
        let metadata = build_metadata(options.issuer, options.metadata);
        let customization = options.customization;

        let device_store = options.device_store;

        let account_manager = AccountManager::new(options.account_store);
        let client_manager = ClientManager::new();
        let request_manager = RequestManager::new(
            request_store,
            oauth_verifier.signer.clone(),
            metadata.clone(),
            authentication_max_age,
        );
        let token_manager = TokenManager::new(
            options.token_store,
            oauth_verifier.signer.clone(),
            oauth_verifier.access_token_type.clone(),
            Some(authentication_max_age),
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

    pub fn get_jwks(&self) -> Vec<Jwk> {
        self.oauth_verifier.keyset.public_jwks()
    }

    fn login_required(&self, info: &DeviceAccountInfo) -> bool {
        unimplemented!()
        // /* in seconds */
        // let now = now_as_secs();
        // let auth_age = now - info.authenticated_at;
        //
        // // Fool-proof (invalid date, or suspiciously in the future)
        // if auth_age < 0 {
        //     return true;
        // }
        //
        // auth_age >= self.authentication_max_age
    }

    //Done
    async fn authenticate_client(
        &mut self,
        credentials: OAuthClientCredentials,
    ) -> Result<(Client, ClientAuth), OAuthError> {
        let client_id = credentials.client_id();
        let client = self.client_manager.get_client(client_id).await?;
        let (client_auth, nonce) = client
            .verify_credentials(credentials, &self.oauth_verifier.issuer)
            .await;

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

        //TODO
        // if payload.jti.is_none() {
        //     return Err(OAuthError::InvalidParametersError(
        //         "Request object must contain a jti claim".to_string(),
        //     ));
        // }

        // if (!(await this.replayManager.uniqueJar(result.payload.jti, client.id))) {
        // throw new InvalidParametersError(
        // payload,
        // 'Request object jti is not unique',
        // )
        // }
        //
        // if ('protectedHeader' in result) {
        //     if (!result.protectedHeader.kid) {
        //         throw new InvalidParametersError(payload, 'Missing "kid" in header')
        //     }
        //
        //     return {
        //         jkt: await authJwkThumbprint(result.key),
        //         payload,
        //         protectedHeader: result.protectedHeader as {
        //             alg: string
        //             kid: string
        //         },
        //     }
        // }
        //
        // if ('header' in result) {
        //     return {
        //         payload,
        //     }
        // }

        // Should never happen
        Err(OAuthError::RuntimeError(
            "Invalid request object".to_string(),
        ))
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
            .create_authorization_request(&client, &client_auth, &parameters, None, dpop_jkt)
            .await;

        let response = OAuthParResponse::new(res.uri.into_inner(), res.expires_at).unwrap();
        Ok(response)
    }

    async fn process_authorization_request(
        &mut self,
        client: Client,
        device_id: DeviceId,
        query: OAuthAuthorizationRequestQuery,
    ) -> Result<RequestInfo, OAuthError> {
        unimplemented!()
        // match query {
        //     OAuthAuthorizationRequestQuery::Parameters(query) => {
        //         let auth = ClientAuth {
        //             method: "none".to_string(),
        //             alg: "".to_string(),
        //             kid: "".to_string(),
        //             jkt: "".to_string(),
        //         };
        //         return Ok(self
        //             .request_manager
        //             .create_authorization_request(&client, &auth, &query, Some(device_id), None)
        //             .await);
        //     }
        //     OAuthAuthorizationRequestQuery::Jar(query) => {
        //         let request_object = self.decode_jar(&client, query).await?;
        //     }
        //     OAuthAuthorizationRequestQuery::Uri(query) => {
        //         let request_uri = query.request_uri().clone();
        //         return self
        //             .request_manager
        //             .get(&request_uri, &client.id, &device_id)
        //             .await;
        //     }
        // }
    }

    async fn delete_request(
        &mut self,
        uri: OAuthRequestUri,
        parameters: OAuthAuthorizationRequestParameters,
    ) -> Result<(), OAuthError> {
        self.request_manager.delete(&uri).await;
        Ok(())
    }

    pub async fn authorize(
        &self,
        device_id: &DeviceId,
        credentials: &OAuthClientCredentials,
        query: &OAuthAuthorizationRequestQuery,
    ) -> Result<AuthorizationResult, OAuthError> {
        unimplemented!()
        // let issuer = self.oauth_verifier.issuer.clone();
        //
        // // If there is a chance to redirect the user to the client, let's do
        // // it by wrapping the error in an AccessDeniedError.
        // if let OAuthAuthorizationRequestQuery::Parameters(params) = query {
        //     if params.redirect_uri.is_some() {
        //         // https://datatracker.ietf.org/doc/html/draft-ietf-oauth-v2-1-11#section-4.1.2.1
        //         return Err(OAuthError::AccessDeniedError("invalid_request".to_string()));
        //     }
        // }
        //
        // let client = self
        //     .client_manager
        //     .get_client(&credentials.client_id)
        //     .await?;
        //
        // let request_info = self
        //     .process_authorization_request(client, device_id, query)
        //     .await?;
    }

    async fn inner_authorize(
        &mut self,
        device_id: &DeviceId,
        client: &Client,
        client_auth: &ClientAuth,
        parameters: &OAuthAuthorizationRequestParameters,
    ) -> Result<AuthorizationResult, OAuthError> {
        unimplemented!()
        // let sessions = self
        //     .get_sessions(client, client_auth, device_id, parameters)
        //     .await?;
        //
        // if parameters.prompt.is_none() {
        //     let sso_sessions: Vec<OAuthProviderSession>;
        //     if sso_sessions.len() > 1 {
        //         return Err(OAuthError::AccountSelectionRequiredError);
        //     }
        //
        //     let sso_session = match sso_sessions.get(0) {
        //         None => return Err(OAuthError::LoginRequiredError),
        //         Some(session) => session,
        //     };
        //
        //     if sso_session.login_required {
        //         return Err(OAuthError::LoginRequiredError);
        //     }
        //
        //     if sso_session.consent_required {
        //         return Err(OAuthError::ConsentRequiredError);
        //     }
        //
        //     let code = self
        //         .request_manager
        //         .set_authorized(client, uri, device_id, sso_session.account)
        //         .await?;
        // }
    }

    pub async fn get_sessions(
        &self,
        client: &Client,
        client_auth: &ClientAuth,
        device_id: &DeviceId,
        parameters: &OAuthAuthorizationRequestParameters,
    ) -> Result<Vec<OAuthProviderSession>, OAuthError> {
        let accounts = self.account_manager.list(device_id).await;

        let hint = parameters.login_hint.clone();

        let matches_hint = |account: Account| -> bool {
            (account.sub.get() == hint.clone().unwrap())
                || (account.preferred_username.is_some()
                    && account.preferred_username.unwrap() == hint.unwrap())
        };

        //TODO
        let mut sessions = Vec::new();
        for account_info in accounts {
            let session = OAuthProviderSession {
                account: account_info.account.clone(),
                info: account_info.info.clone(),
                selected: false,
                login_required: false,
                consent_required: false,
                matches_hint: false,
            };
            sessions.push(session);
        }

        Ok(sessions)
    }

    pub async fn sign_in(
        &mut self,
        device_id: DeviceId,
        uri: OAuthRequestUri,
        client_id: ClientId,
        credentials: SignInCredentials,
    ) -> Result<SignInResponse, OAuthError> {
        let client = self.client_manager.get_client(&client_id).await?;

        // Ensure the request is still valid (and update the request expiration)
        // @TODO use the returned scopes to determine if consent is required
        self.request_manager
            .get(&uri, &client_id, &device_id)
            .await?;

        let account_info = match self.account_manager.sign_in(credentials, device_id).await {
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
                !info.authorized_clients.contains(client.id)
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
        uri: OAuthRequestUri,
        client_id: ClientId,
        sub: Sub,
    ) -> Result<AcceptRequestResponse, OAuthError> {
        let client = self.client_manager.get_client(&client_id).await?;

        let result = self
            .request_manager
            .get(&uri, &client_id, &device_id)
            .await?;
        let parameters = result.parameters;
        let client_auth = result.client_auth;

        let result = match self.account_manager.get(&device_id, sub).await {
            Ok(res) => res,
            Err(e) => {
                self.delete_request(uri, parameters).await?;
                return Err(OAuthError::AccessDeniedError("test".to_string()));
            }
        };
        let account = result.account;
        let info = result.info;

        // The user is trying to authorize without a fresh login
        if self.login_required(&info) {
            return Err(OAuthError::LoginRequiredError);
        }

        let code = match self
            .request_manager
            .set_authorized(
                client.clone(),
                uri.clone(),
                device_id.clone(),
                account.clone(),
            )
            .await
        {
            Ok(res) => res,
            Err(e) => {
                self.delete_request(uri, parameters).await?;
                return Err(OAuthError::AccessDeniedError("test".to_string()));
            }
        };

        self.account_manager
            .add_authorized_client(device_id, account, client, client_auth)
            .await;

        Ok(AcceptRequestResponse {
            issuer: self.oauth_verifier.issuer.clone(),
            parameters,
            redirect_code: code,
        })
    }

    pub async fn reject_request(
        &mut self,
        device_id: DeviceId,
        uri: OAuthRequestUri,
        client_id: ClientId,
    ) -> Result<RejectRequestResponse, OAuthError> {
        let request_info = self
            .request_manager
            .get(&uri, &client_id, &device_id)
            .await?;

        self.delete_request(uri, request_info.parameters.clone())
            .await?;

        Ok(RejectRequestResponse {
            issuer: self.oauth_verifier.issuer.clone(),
            parameters: request_info.parameters,
            error: "access_denied".to_string(),
            error_string: "Access denied".to_string(),
        })
    }

    //Done
    pub async fn token(
        &mut self,
        credentials: OAuthClientCredentials,
        request: OAuthTokenRequest,
        dpop_jkt: Option<String>,
    ) -> Result<OAuthTokenResponse, OAuthError> {
        let (client, client_auth) = self.authenticate_client(credentials).await?;

        // if let Some(grant_types_supported) = &self.metadata.grant_types_supported {
        //     if !grant_types_supported.contains(&request.grant_type) {
        //         return Err(OAuthError::InvalidGrantError(
        //             "Grant type TODO is not supported by the server".to_string(),
        //         ));
        //     }
        // }

        // if !client.metadata.grant_types.contains(&request.type_id()) {
        //     return Err(OAuthError::InvalidGrantError(
        //         "Grant type is not supported by the server".to_string(),
        //     ));
        // }
        //
        match request {
            OAuthTokenRequest::AuthorizationCode(request) => {
                self.code_grant(client, client_auth, request, dpop_jkt)
                    .await
            }
            OAuthTokenRequest::RefreshToken(request) => {
                self.refresh_token_grant(client, client_auth, request, dpop_jkt)
                    .await
            }
            OAuthTokenRequest::Password(request) => Err(OAuthError::InvalidGrantError(
                "Grant type TODO is not supported by the server".to_string(),
            )),
            OAuthTokenRequest::ClientCredentials(request) => Err(OAuthError::InvalidGrantError(
                "Grant type TODO is not supported by the server".to_string(),
            )),
        }
    }

    pub async fn code_grant(
        &mut self,
        client: Client,
        client_auth: ClientAuth,
        input: OAuthAuthorizationCodeGrantTokenRequest,
        dpop_jkt: Option<String>,
    ) -> Result<OAuthTokenResponse, OAuthError> {
        unimplemented!()
        // let code = parse(input.code().to_string())?;
        //
        // let request_data_authorized = self
        //     .request_manager
        //     .find_code(client, client_auth, code)
        //     .await?;
        //
        // // the following check prevents re-use of PKCE challenges, enforcing the
        // // clients to generate a new challenge for each authorization request. The
        // // replay manager typically prevents replay over a certain time frame,
        // // which might not cover the entire lifetime of the token (depending on
        // // the implementation of the replay store). For this reason, we should
        // // ideally ensure that the code_challenge was not already used by any
        // // existing token or any other pending request.
        // //
        // // The current implementation will cause client devs not issuing a new
        // // code challenge for each authorization request to fail, which should be
        // // a good enough incentive to follow the best practices, until we have a
        // // better implementation.
        // //
        // // @TODO: Use tokenManager to ensure uniqueness of code_challenge
        // if let Some(code_challenge) = request_data_authorized.parameters.code_challenge {
        //     let unique = self
        //         .oauth_verifier
        //         .replay_manager
        //         .unique_code_challenge(code_challenge)
        //         .await;
        //     if !unique {
        //         return Err(OAuthError::InvalidGrantError(
        //             "Code challenge already used".to_string(),
        //         ));
        //     }
        // }
        //
        // let account_info = self
        //     .account_manager
        //     .get(
        //         &request_data_authorized.device_id,
        //         request_data_authorized.sub,
        //     )
        //     .await?;
        //
        // Ok(self
        //     .token_manager
        //     .create(
        //         client,
        //         client_auth,
        //         account_info.account,
        //         Some((request_data_authorized.device_id, account_info.info)),
        //         request_data_authorized.parameters,
        //         None, // input,
        //         dpop_jkt,
        //     )
        //     .await?)
    }

    pub async fn refresh_token_grant(
        &self,
        client: Client,
        client_auth: ClientAuth,
        input: OAuthRefreshTokenGrantTokenRequest,
        dpop_jkt: Option<String>,
    ) -> Result<OAuthTokenResponse, OAuthError> {
        Ok(self
            .token_manager
            .refresh(client, client_auth, input, dpop_jkt)
            .await)
    }

    /**
     * @see {@link https://datatracker.ietf.org/doc/html/rfc7009#section-2.1 rfc7009}
     */
    pub async fn revoke(&self, token: &OAuthTokenIdentification) -> Result<(), OAuthError> {
        // @TODO this should also remove the account-device association (or, at least, mark it as expired)
        self.token_manager.revoke(token).await;
        Ok(())
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

        let start = now_as_secs();
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
            Some(res) => OAuthTokenType::DPoP,
        };
        Ok(OAuthIntrospectionResponse::Active(ActiveTokenInfo {
            scope: Some(token_info.data.parameters.scope.unwrap().into_inner()),
            client_id: Some(token_info.data.client_id),
            username: token_info.account.preferred_username,
            token_type: Some(token_type),
            authorization_details: token_info.data.details,
            aud: None, //token_info.account.aud,
            exp: Some(token_info.data.expires_at as i64),
            iat: Some(token_info.data.updated_at as i64),
            iss: Some(self.oauth_verifier.signer.issuer.to_string()),
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
        unimplemented!()
        // if is_token_id(token.to_string().as_str()) {
        //     self.oauth_verifier
        //         .assert_token_type_allowed(token_type.clone(), AccessTokenType::ID)?;
        //
        //     //TODO
        //     // return self.token_manager.authenticate_token_id(
        //     //     token_type,
        //     //     token.to_string(),
        //     //     dpop_jkt,
        //     //     verify_options,
        //     // )?;
        // }
        //
        // self.oauth_verifier
        //     .authenticate_token(token_type, token, dpop_jkt, verify_options).await?
    }
}
