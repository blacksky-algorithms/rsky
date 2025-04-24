use crate::jwk::{Audience, JwtConfirmation, SignedJwt, VerifyOptions};
use crate::oauth_provider::access_token::access_token_type::AccessTokenType;
use crate::oauth_provider::account::account::Account;
use crate::oauth_provider::account::account_store::DeviceAccountInfo;
use crate::oauth_provider::client::client::Client;
use crate::oauth_provider::client::client_auth::ClientAuth;
use crate::oauth_provider::constants::{
    AUTHENTICATED_REFRESH_INACTIVITY_TIMEOUT, AUTHENTICATED_REFRESH_LIFETIME, TOKEN_MAX_AGE,
    UNAUTHENTICATED_REFRESH_INACTIVITY_TIMEOUT, UNAUTHENTICATED_REFRESH_LIFETIME,
};
use crate::oauth_provider::device::device_id::DeviceId;
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_provider::now_as_secs;
use crate::oauth_provider::oauth_hooks::OAuthHooks;
use crate::oauth_provider::request::code::Code;
use crate::oauth_provider::signer::signer::{AccessTokenOptions, Signer};
use crate::oauth_provider::token::refresh_token::RefreshToken;
use crate::oauth_provider::token::token_claims::TokenClaims;
use crate::oauth_provider::token::token_data::TokenData;
use crate::oauth_provider::token::token_id::TokenId;
use crate::oauth_provider::token::token_store::{NewTokenData, TokenInfo, TokenStore};
use crate::oauth_provider::token::verify_token_claims::{
    verify_token_claims, VerifyTokenClaimsOptions, VerifyTokenClaimsResult,
};
use crate::oauth_types::{
    OAuthAccessToken, OAuthAuthorizationCodeGrantTokenRequest, OAuthAuthorizationDetails,
    OAuthAuthorizationRequestParameters, OAuthClientCredentialsGrantTokenRequest, OAuthClientId,
    OAuthCodeChallengeMethod, OAuthGrantType, OAuthPasswordGrantTokenRequest,
    OAuthRefreshTokenGrantTokenRequest, OAuthTokenIdentification, OAuthTokenResponse,
    OAuthTokenType, CLIENT_ASSERTION_TYPE_JWT_BEARER,
};
use base64ct::{Base64, Encoding};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Eq, PartialEq)]
pub struct AuthenticateTokenIdResult {
    pub verify_token_claims_result: VerifyTokenClaimsResult,
    pub token_info: TokenInfo,
}

pub enum CreateTokenInput {
    Authorization(OAuthAuthorizationCodeGrantTokenRequest),
    Client(OAuthClientCredentialsGrantTokenRequest),
    Password(OAuthPasswordGrantTokenRequest),
}

impl CreateTokenInput {
    pub fn grant_type(&self) -> OAuthGrantType {
        match self {
            CreateTokenInput::Authorization(_) => OAuthGrantType::AuthorizationCode,
            CreateTokenInput::Client(_) => OAuthGrantType::ClientCredentials,
            CreateTokenInput::Password(_) => OAuthGrantType::Password,
        }
    }
}

pub struct TokenManager {
    pub store: Arc<RwLock<dyn TokenStore>>,
    pub signer: Arc<RwLock<Signer>>,
    pub access_token_type: AccessTokenType,
    pub token_max_age: i64,
    pub oauth_hooks: Arc<OAuthHooks>,
}

pub type TokenManagerCreator = Box<
    dyn Fn(
            Arc<RwLock<dyn TokenStore>>,
            Arc<RwLock<Signer>>,
            AccessTokenType,
            Option<i64>,
            Arc<OAuthHooks>,
        ) -> TokenManager
        + Send
        + Sync,
>;

impl TokenManager {
    pub fn creator() -> TokenManagerCreator {
        Box::new(
            move |store: Arc<RwLock<dyn TokenStore>>,
                  signer: Arc<RwLock<Signer>>,
                  access_token_type: AccessTokenType,
                  max_age: Option<i64>,
                  hooks: Arc<OAuthHooks>|
                  -> TokenManager {
                TokenManager::new(store, signer, access_token_type, max_age, hooks)
            },
        )
    }

    pub fn new(
        store: Arc<RwLock<dyn TokenStore>>,
        signer: Arc<RwLock<Signer>>,
        access_token_type: AccessTokenType,
        max_age: Option<i64>,
        oauth_hooks: Arc<OAuthHooks>,
    ) -> Self {
        let token_max_age = max_age.unwrap_or_else(|| TOKEN_MAX_AGE);
        TokenManager {
            store,
            signer,
            access_token_type,
            token_max_age,
            oauth_hooks,
        }
    }

    fn create_token_expiry(&self, now: Option<DateTime<Utc>>) -> DateTime<Utc> {
        let now = now.unwrap_or(Utc::now());
        DateTime::from_timestamp(now.timestamp() + self.token_max_age, 0).unwrap()
    }

    fn use_jwt_access_token(&self, account: Account) -> bool {
        if self.access_token_type == AccessTokenType::AUTO {
            return match account.aud {
                Audience::Single(aud) => self.signer.blocking_read().issuer.to_string() == aud,
                Audience::Multiple(_) => false,
            };
        }
        self.access_token_type == AccessTokenType::JWT
    }

    pub async fn create(
        &self,
        client: Client,
        client_auth: ClientAuth,
        account: Account,
        device: Option<(DeviceId, DeviceAccountInfo)>,
        mut parameters: OAuthAuthorizationRequestParameters,
        dpop_jkt: Option<String>,
        input: CreateTokenInput,
    ) -> Result<OAuthTokenResponse, OAuthError> {
        // @NOTE the atproto specific DPoP requirement is enforced though the
        // "dpop_bound_access_tokens" metadata, which is enforced by the
        // ClientManager class.
        if client.metadata.dpop_bound_access_tokens.is_some() && dpop_jkt.is_none() {
            return Err(OAuthError::InvalidDpopProofError(
                "DPoP proof required".to_string(),
            ));
        }

        if parameters.dpop_jkt.is_none() {
            // Allow clients to bind their access tokens to a DPoP key during
            // token request if they didn't provide a "dpop_jkt" during the
            // authorization request.
            if dpop_jkt.is_some() {
                parameters.dpop_jkt = dpop_jkt;
            }
        } else if parameters.dpop_jkt != dpop_jkt {
            return Err(OAuthError::InvalidDpopKeyBindingError);
        }

        if let ClientAuth::Some(details) = client_auth.clone() {
            // Clients **must not** use their private key to sign DPoP proofs.
            if parameters.dpop_jkt.is_some() && details.jkt == parameters.dpop_jkt.clone().unwrap()
            {
                return Err(OAuthError::InvalidRequestError(
                    "The DPoP proof must be signed with a different key than the client assertion"
                        .to_string(),
                ));
            }
        }

        let input_grant_type = input.grant_type();
        if !client.metadata.grant_types.contains(&input_grant_type) {
            return Err(OAuthError::InvalidGrantError(format!(
                "This client is not allowed to use the \"{input_grant_type}\" grant type"
            )));
        }

        match input {
            CreateTokenInput::Authorization(input) => {
                let store = self.store.read().await;
                let result = store.find_token_by_code(input.code().clone()).await?;
                if let Some(token_info) = result {
                    let mut store = self.store.write().await;
                    store.delete_token(token_info.id).await?;
                    return Err(OAuthError::InvalidGrantError("Code replayed".to_string()));
                }

                if let Some(ref params_redirect_uri) = parameters.redirect_uri {
                    if params_redirect_uri.clone() != input.redirect_uri().clone() {
                        return Err(OAuthError::InvalidGrantError("The redirect_uri parameter must match the one used in the authorization request".to_string()));
                    }
                } else {
                    return Err(OAuthError::InvalidGrantError("The redirect_uri parameter must match the one used in the authorization request".to_string()));
                }

                if let Some(ref code_challenge) = parameters.code_challenge {
                    let code_verifier = match input.code_verifier() {
                        None => {
                            return Err(OAuthError::InvalidGrantError(
                                "code_verifier is required".to_string(),
                            ));
                        }
                        Some(code_verifier) => code_verifier,
                    };
                    if code_verifier.len() < 43 {
                        return Err(OAuthError::InvalidGrantError(
                            "code_verifier too short".to_string(),
                        ));
                    }

                    if let Some(code_challenge_method) = parameters.code_challenge_method {
                        match code_challenge_method {
                            OAuthCodeChallengeMethod::S256 => {
                                //todo
                                // let input_challenge = code_challenge.clone();
                                // let base64_input_challenge =
                                //     Base64::encode_string(&input_challenge.as_bytes());
                                // let computed_challenge =
                                //     Base64::encode_string(&Sha256::digest(code_verifier));
                                // if input_challenge != computed_challenge {
                                //     return Err(OAuthError::InvalidGrantError(
                                //         "Invalid code_verifier".to_string(),
                                //     ));
                                // }
                            }
                            OAuthCodeChallengeMethod::Plain => {
                                if code_challenge != code_verifier {
                                    return Err(OAuthError::InvalidGrantError(
                                        "Invalid code_verifier".to_string(),
                                    ));
                                }
                            }
                        }
                    }
                } else if input.code_verifier().is_some() {
                    return Err(OAuthError::InvalidRequestError(
                        "code_challenge parameter wasn't provided".to_string(),
                    ));
                }
            }
            _ => {
                // Other grants (e.g "password", "client_credentials") could be added
                // here in the future...
                return Err(OAuthError::InvalidGrantError(format!(
                    "Unsupported grant type \"{input_grant_type}\""
                )));
            }
        }

        let token_id = TokenId::generate();
        let refresh_token = if client
            .metadata
            .grant_types
            .contains(&OAuthGrantType::RefreshToken)
        {
            Some(RefreshToken::generate())
        } else {
            None
        };

        let now = Utc::now();
        let expires_at = self.create_token_expiry(Some(now));

        let details = match &self.oauth_hooks.on_authorization_details {
            None => None,
            Some(details) => Some(details(client.clone(), parameters.clone(), account.clone())),
        };

        let device_id = match device {
            None => None,
            Some((device_id, _device_account_info)) => Some(device_id),
        };
        let token_data = TokenData {
            created_at: Utc::now(),
            updated_at: Utc::now(),
            expires_at,
            client_id: client.id.clone(),
            client_auth,
            device_id,
            sub: account.sub.clone(),
            parameters: parameters.clone(),
            details: details.clone(),
            code: None,
        };

        let mut store = self.store.write().await;
        store
            .create_token(token_id.clone(), token_data, refresh_token.clone())
            .await?;
        drop(store);

        //inside try catch
        let access_token = if !self.use_jwt_access_token(account.clone()) {
            token_id.val()
        } else {
            let signer = self.signer.read().await;
            let cnf = match parameters.dpop_jkt.clone() {
                None => None,
                Some(dpop_jkt) => Some(JwtConfirmation {
                    kid: None,
                    jwk: None,
                    jwe: None,
                    jku: None,
                    jkt: Some(dpop_jkt),
                    x5t_s256: None,
                    osc: None,
                }),
            };
            let options = AccessTokenOptions {
                // We don't specify the alg here. We suppose the Resource server will be
                // able to verify the token using any alg.
                aud: account.aud.clone(),
                sub: account.sub.clone(),
                jti: token_id.clone(),
                exp: expires_at,
                iat: Some(now),
                alg: None,
                cnf,
                authorization_details: details.clone(),
            };
            match signer
                .access_token(client.clone(), parameters.clone(), options)
                .await
            {
                Ok(signed_jwt) => signed_jwt.val(),
                Err(error) => {
                    // Just in case the token could not be issued, we delete it from the store
                    let mut store = self.store.write().await;
                    store.delete_token(token_id).await?;
                    return Err(OAuthError::RuntimeError("".to_string()));
                }
            }
        };

        Ok(self
            .build_token_response(
                OAuthAccessToken::new(access_token).unwrap(),
                refresh_token,
                expires_at,
                parameters,
                account,
                details,
            )
            .await)
    }

    async fn build_token_response(
        &self,
        access_token: OAuthAccessToken,
        refresh_token: Option<RefreshToken>,
        expires_at: DateTime<Utc>,
        parameters: OAuthAuthorizationRequestParameters,
        account: Account,
        authorization_details: Option<OAuthAuthorizationDetails>,
    ) -> OAuthTokenResponse {
        let mut additional_fields = HashMap::new();
        additional_fields.insert(
            String::from("sub"),
            serde_json::Value::String(account.sub.get()),
        );
        let token_type = match parameters.dpop_jkt {
            None => OAuthTokenType::Bearer,
            Some(_) => OAuthTokenType::DPoP,
        };
        let token_response = OAuthTokenResponse {
            access_token,
            token_type,
            scope: parameters.scope,
            refresh_token,
            expires_in: Some(expires_at.timestamp() - now_as_secs()),
            id_token: None,
            authorization_details,
            // ATPROTO extension: add the sub claim to the token response to allow
            // clients to resolve the PDS url (audience) using the did resolution
            // mechanism.
            additional_fields,
        };
        token_response
    }

    async fn validate_access(
        &self,
        client: &Client,
        client_auth: &ClientAuth,
        token_info: &TokenInfo,
    ) -> Result<(), OAuthError> {
        if token_info.data.client_id != client.id.clone() {
            return Err(OAuthError::InvalidGrantError(
                "Token was not issued to this client".to_string(),
            ));
        }

        if let Some(info) = token_info.info.clone() {
            if !info.authorized_clients.contains(&client.id.clone()) {
                return Err(OAuthError::InvalidGrantError(
                    "Client no longer trusted by user".to_string(),
                ));
            }
        }

        if token_info.data.client_auth.method() != client_auth.method() {
            return Err(OAuthError::InvalidGrantError(
                "Client authentication method mismatch".to_string(),
            ));
        }

        if !client
            .validate_client_auth(&token_info.data.client_auth)
            .await
        {
            return Err(OAuthError::InvalidGrantError(
                "Client authentication mismatch".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn refresh(
        &self,
        client: Client,
        client_auth: ClientAuth,
        input: OAuthRefreshTokenGrantTokenRequest,
        dpop_jkt: Option<String>,
    ) -> Result<OAuthTokenResponse, OAuthError> {
        let refresh_token = match RefreshToken::new(input.refresh_token().to_string()) {
            Ok(refresh_token) => refresh_token,
            Err(_) => {
                return Err(OAuthError::InvalidRequestError(
                    "Invalid refresh token".to_string(),
                ))
            }
        };

        let store = self.store.read().await;
        let token_info = match store
            .find_token_by_refresh_token(refresh_token.clone())
            .await?
        {
            None => {
                return Err(OAuthError::InvalidGrantError(
                    "Invalid refresh token".to_string(),
                ))
            }
            Some(token_info) => token_info,
        };
        drop(store);

        let account = token_info.account.clone();
        let data = token_info.data.clone();
        let parameters = data.parameters;

        if let Some(token_refresh_token) = token_info.current_refresh_token.clone() {
            if token_refresh_token != refresh_token {
                let mut store = self.store.write().await;
                store.delete_token(token_info.id).await?;
                return Err(OAuthError::InvalidGrantError(
                    "refresh token replayed".to_string(),
                ));
            }
        } else {
            let mut store = self.store.write().await;
            store.delete_token(token_info.id).await?;
            return Err(OAuthError::InvalidGrantError(
                "refresh token replayed".to_string(),
            ));
        }

        self.validate_access(&client, &client_auth, &token_info)
            .await?;

        if !client
            .metadata
            .grant_types
            .contains(&OAuthGrantType::RefreshToken)
        {
            let mut store = self.store.write().await;
            // In case the client metadata was updated after the token was issued
            store.delete_token(token_info.id).await?;
            return Err(OAuthError::InvalidGrantError(
                "This client is not allowed to use the".to_string(),
            ));
        }

        if let Some(p_dpop_jkt) = parameters.dpop_jkt.clone() {
            if let Some(dpop_jkt) = dpop_jkt {
                if dpop_jkt != p_dpop_jkt {
                    return Err(OAuthError::InvalidDpopKeyBindingError);
                }
            } else {
                let mut store = self.store.write().await;
                store.delete_token(token_info.id).await?;
                return Err(OAuthError::InvalidDpopProofError(
                    "DPoP proof required".to_string(),
                ));
            }
        }

        let last_activity = data.updated_at;
        let inactivity_timeout = if client_auth.method() == "none" && !client.info.is_first_party {
            UNAUTHENTICATED_REFRESH_INACTIVITY_TIMEOUT
        } else {
            AUTHENTICATED_REFRESH_INACTIVITY_TIMEOUT
        };
        if last_activity.timestamp() + inactivity_timeout < now_as_secs() {
            let mut store = self.store.write().await;
            store.delete_token(token_info.id).await?;
            return Err(OAuthError::InvalidGrantError(
                "Refresh token exceeded inactivity timeout".to_string(),
            ));
        }

        let lifetime = if client_auth.method() == "none" && !client.info.is_first_party {
            UNAUTHENTICATED_REFRESH_LIFETIME
        } else {
            AUTHENTICATED_REFRESH_LIFETIME
        };
        if data.created_at.timestamp() + lifetime < now_as_secs() {
            let mut store = self.store.write().await;
            store.delete_token(token_info.id).await?;
            return Err(OAuthError::InvalidGrantError(
                "Refresh token expired".to_string(),
            ));
        }

        //TODO
        // let authorization_details;
        let authorization_details = Some(OAuthAuthorizationDetails::new());

        let next_token_id = TokenId::generate();
        let next_refresh_token = RefreshToken::generate();

        let now = Utc::now();
        let expires_at = self.create_token_expiry(Some(now));

        let new_token_data = NewTokenData {
            // When clients rotate their public keys, we store the key that was
            // used by the client to authenticate itself while requesting new
            // tokens. The validateAccess() method will ensure that the client
            // still advertises the key that was used to issue the previous
            // refresh token. If a client stops advertising a key, all tokens
            // bound to that key will no longer be be refreshable. This allows
            // clients to proactively invalidate tokens when a key is compromised.
            // Note that the original DPoP key cannot be rotated. This protects
            // users in case the ownership of the client id changes. In the latter
            // case, a malicious actor could still advertises the public keys of
            // the previous owner, but the new owner would not be able to present
            // a valid DPoP proof.
            client_auth,
            expires_at,
            updated_at: Utc::now(),
        };
        let mut store = self.store.write().await;
        store
            .rotate_token(
                token_info.id,
                next_token_id.clone(),
                next_refresh_token.clone(),
                new_token_data,
            )
            .await?;
        drop(store);

        let access_token = match !self.use_jwt_access_token(account.clone()) {
            true => next_token_id.val(),
            false => {
                // We don't specify the alg here. We suppose the Resource server will be
                // able to verify the token using any alg.
                let options = AccessTokenOptions {
                    aud: account.aud.clone(),
                    sub: account.sub.clone(),
                    jti: next_token_id,
                    exp: expires_at,
                    iat: Some(now),
                    alg: None,
                    cnf: None,
                    authorization_details: authorization_details.clone(),
                };
                let signer = self.signer.read().await;
                match signer
                    .access_token(client.clone(), parameters.clone(), options)
                    .await
                {
                    Ok(res) => res.val(),
                    Err(_) => return Err(OAuthError::RuntimeError("".to_string())),
                }
            }
        };

        Ok(self
            .build_token_response(
                OAuthAccessToken::new(access_token).unwrap(),
                Some(next_refresh_token),
                expires_at,
                parameters,
                account,
                authorization_details,
            )
            .await)
    }

    /**
     * @see {@link https://datatracker.ietf.org/doc/html/rfc7009#section-2.2 | RFC7009 Section 2.2}
     */
    pub async fn revoke(&mut self, token: OAuthTokenIdentification) -> Result<(), OAuthError> {
        let token = token.token();
        if let Ok(token_id) = TokenId::new(token.as_str()) {
            let mut store = self.store.write().await;
            store.delete_token(token_id).await?;
            drop(store);
        } else if let Ok(signed_jwt) = SignedJwt::new(token.as_str()) {
            let mut options = VerifyOptions {
                audience: None,
                clock_tolerance: None,
                issuer: None,
                max_token_age: None,
                subject: None,
                typ: None,
                current_date: None,
                required_claims: vec![],
            };
            options.required_claims.push("jti".to_string());
            let signer = self.signer.read().await;
            let verify_result = match signer.verify(signed_jwt, Some(options)).await {
                Ok(res) => res,
                Err(error) => return Err(OAuthError::RuntimeError("".to_string())),
            };
            let token_id = match verify_result.payload.jti {
                Some(token_id) => token_id,
                None => return Err(OAuthError::RuntimeError("".to_string())),
            };
            let mut store = self.store.write().await;
            store.delete_token(TokenId::new(token_id).unwrap()).await?;
            return Ok(());
        } else if let Ok(refresh_token) = RefreshToken::new(token.as_str()) {
            let store = self.store.read().await;
            let token_info = store.find_token_by_refresh_token(refresh_token).await?;
            drop(store);
            if let Some(token_info) = token_info {
                let mut store = self.store.write().await;
                store.delete_token(token_info.id).await?;
            }
        } else if let Ok(code) = Code::new(token.as_str()) {
            let store = self.store.read().await;
            let token_info = store.find_token_by_code(code).await?;
            drop(store);
            if let Some(token_info) = token_info {
                let mut store = self.store.write().await;
                store.delete_token(token_info.id).await?;
            }
        }
        Ok(())
    }

    /**
     * Allows an (authenticated) client to obtain information about a token.
     *
     * @see {@link https://datatracker.ietf.org/doc/html/rfc7662 RFC7662}
     */
    pub async fn client_token_info(
        &mut self,
        client: &Client,
        client_auth: &ClientAuth,
        token: &OAuthTokenIdentification,
    ) -> Result<TokenInfo, OAuthError> {
        let token_info = self.find_token_info(token).await?;
        let token_info = match token_info {
            None => return Err(OAuthError::InvalidGrantError("Invalid token".to_string())),
            Some(res) => res,
        };

        match self.validate_access(client, client_auth, &token_info).await {
            Ok(_) => {}
            Err(e) => {
                let mut store = self.store.write().await;
                store.delete_token(token_info.id).await?;
                return Err(e);
            }
        }

        let now = now_as_secs();
        if token_info.data.expires_at.timestamp() < now {
            return Err(OAuthError::InvalidGrantError("Token expired".to_string()));
        }

        Ok(token_info)
    }

    async fn find_token_info(
        &self,
        token: &OAuthTokenIdentification,
    ) -> Result<Option<TokenInfo>, OAuthError> {
        let token_val = token.token();

        if let Ok(token_id) = TokenId::new(token_val.as_str()) {
            let store = self.store.read().await;
            return store.read_token(token_id).await;
        }

        if let Ok(signed_jwt) = SignedJwt::new(token_val.as_str()) {
            let signer = self.signer.read().await;
            let payload = match signer.verify_access_token(signed_jwt, None).await {
                Ok(response) => response.payload,
                Err(_) => return Ok(None),
            };

            let store = self.store.read().await;
            let token_info = match store.read_token(payload.jti).await? {
                None => return Ok(None),
                Some(token_info) => token_info,
            };

            // Audience changed (e.g. user was moved to another resource server)
            if payload.aud != token_info.account.aud {
                return Ok(None);
            }

            // Invalid store implementation ?
            if payload.sub != token_info.account.sub {
                return Err(OAuthError::RuntimeError(
                    "Account sub does not match token sub".to_string(),
                ));
            }

            return Ok(Some(token_info));
        }

        if let Ok(refresh_token) = RefreshToken::new(token_val.as_str()) {
            let store = self.store.read().await;
            let token_info = store.find_token_by_refresh_token(refresh_token).await?;
            return if let Some(token_info) = token_info {
                if token_info.current_refresh_token.is_none() {
                    Ok(None)
                } else {
                    Ok(Some(token_info))
                }
            } else {
                Ok(None)
            };
        }

        // Should never happen
        Ok(None)
    }

    pub async fn get_token_info(
        &self,
        token_type: OAuthTokenType,
        token_id: TokenId,
    ) -> Result<TokenInfo, OAuthError> {
        let store = self.store.read().await;
        let token_info = store.read_token(token_id).await?;

        match token_info {
            None => Err(OAuthError::InvalidTokenError(
                token_type,
                "Invalid token".to_string(),
            )),
            Some(token_info) => {
                if token_info.data.expires_at.timestamp() < now_as_secs() {
                    return Err(OAuthError::InvalidTokenError(
                        token_type,
                        "Token expired".to_string(),
                    ));
                }

                Ok(token_info)
            }
        }
    }

    pub async fn authenticate_token_id(
        &self,
        token_type: OAuthTokenType,
        token: TokenId,
        dpop_jkt: Option<String>,
        verify_options: Option<VerifyTokenClaimsOptions>,
    ) -> Result<AuthenticateTokenIdResult, OAuthError> {
        let token_info = self
            .get_token_info(token_type.clone(), token.clone())
            .await?;
        let token_data = token_info.data.clone();
        let cnf = match token_data.parameters.dpop_jkt {
            None => None,
            Some(dpop_jkt) => Some(serde_json::json!(
                "{jkt: ".to_string() + dpop_jkt.as_str() + " }"
            )),
        };
        // Construct a list of claim, as if the token was a JWT.
        let claims = TokenClaims {
            aud: Some(token_info.account.aud.clone()),
            sub: Some(token_info.account.sub.clone()),
            exp: Some(token_info.data.expires_at.timestamp()),
            iat: Some(token_info.data.updated_at.timestamp()),
            scope: token_info.data.parameters.scope.clone(),
            client_id: Some(token_info.data.client_id.clone()),
            cnf,
            ..Default::default()
        };

        let oauth_access_token = OAuthAccessToken::new(token.clone().val()).unwrap();
        let result = verify_token_claims(
            oauth_access_token,
            token,
            token_type,
            dpop_jkt,
            claims,
            verify_options,
        )?;

        Ok(AuthenticateTokenIdResult {
            verify_token_claims_result: result,
            token_info,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jwk::Keyset;
    use crate::jwk_jose::jose_key::JoseKey;
    use crate::oauth_provider::client::client_auth::ClientAuthDetails;
    use crate::oauth_provider::client::client_info::ClientInfo;
    use crate::oauth_provider::oidc::sub::Sub;
    use crate::oauth_provider::token::token_data::TokenData;
    use crate::oauth_types::{
        Display, OAuthClientId, OAuthClientMetadata, OAuthIssuerIdentifier, OAuthRedirectUri,
        OAuthRefreshToken, OAuthResponseType, OAuthScope, Prompt, ResponseMode,
    };
    use biscuit::jwa::Algorithm;
    use biscuit::jwk::{AlgorithmParameters, CommonParameters, JWKSet, RSAKeyParameters, JWK};
    use biscuit::{jwa, Empty};
    use num_bigint::BigUint;
    use std::future::Future;
    use std::pin::Pin;

    struct TestStore {}

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
            let token_id = token_id;
            Box::pin(async move {
                if token_id
                    == TokenId::new("tok-7bddad4f0bf4f788dfb71edc41b4247f".to_string()).unwrap()
                {
                    Ok(Some(TokenInfo {
                        id: TokenId::new("tok-7bddad4f0bf4f788dfb71edc41b4247f").unwrap(),
                        data: TokenData {
                            created_at: "2024-11-28T12:00:09Z".parse::<DateTime<Utc>>().unwrap(),
                            updated_at: "2024-11-28T12:00:09Z".parse::<DateTime<Utc>>().unwrap(),
                            expires_at: "2026-11-28T12:00:09Z".parse::<DateTime<Utc>>().unwrap(),
                            client_id: OAuthClientId::new("https://cleanfollow.com").unwrap(),
                            client_auth: ClientAuth::new(None),
                            device_id: Some(DeviceId::new("dev-64976a0a962c4b7521abd679789c44a8").unwrap()),
                            sub: Sub::new("did:plc:khvyd3oiw46vif5gm7hijslk").unwrap(),
                            parameters: OAuthAuthorizationRequestParameters {
                                client_id: OAuthClientId::new("https://cleanfollow.com").unwrap(),
                                state: Some("-87HL5S0U7rA71zRMMucDA".to_string()),
                                redirect_uri: Some(
                                    OAuthRedirectUri::new("https://cleanfollow-bsky.pages.dev/".to_string())
                                        .unwrap(),
                                ),
                                scope: Some(OAuthScope::new("atproto transition:generic".to_string()).unwrap()),
                                response_type: OAuthResponseType::Code,
                                code_challenge: Some("E0fpC2Qz0OT-iUqtvwxcuaCciTAgsEuE3XmMlz6hcCk".to_string()),
                                code_challenge_method: Some(OAuthCodeChallengeMethod::S256),
                                dpop_jkt: Some("9LLGPTQ9RxcuOdbzwsteKXWHF-9VaMqM76on5tcjiMc".to_string()),
                                response_mode: Some(ResponseMode::Fragment),
                                nonce: None,
                                max_age: Some("2026-11-28T12:00:09Z".parse::<DateTime<Utc>>().unwrap().timestamp() as u32),
                                claims: None,
                                login_hint: Some("ripperoni.com".to_string()),
                                ui_locales: None,
                                id_token_hint: None,
                                display: Some(Display::Page),
                                prompt: Some(Prompt::Consent),
                                authorization_details: None,
                            },
                            details: None,
                            code: Some(
                                Code::new(
                                    "cod-2fe27accba2e80759d069825d88af324fe0f8c8f9b0e4b67a70b1b624649d78c",
                                )
                                    .unwrap(),
                            ),
                        },
                        account: Account {
                            sub: Sub::new("did:plc:khvyd3oiw46vif5gm7hijslk").unwrap(),
                            aud: Audience::Single("did:web:pds.ripperoni.com".to_string()),
                            preferred_username: None,
                            email: None,
                            email_verified: None,
                            picture: None,
                            name: None,
                        },
                        info: None,
                        current_refresh_token: Some(
                            RefreshToken::new(
                                "ref-82f4c3cc0a01f200cae304154af0a531b3b3e4c18478a238b2d9e224e44b28ac",
                            )
                                .unwrap(),
                        ),
                    }))
                } else {
                    Ok(None)
                }
            })
        }

        fn delete_token(
            &mut self,
            token_id: TokenId,
        ) -> Pin<Box<dyn Future<Output = Result<(), OAuthError>> + Send + Sync + '_>> {
            let token_id = token_id;
            Box::pin(async move {
                if token_id == TokenId::new("tok-7e415d9b2aec8f78d11d2b8c7144b87d").unwrap() {
                    return Ok(());
                } else {
                    return Err(OAuthError::RuntimeError("Error".to_string()));
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
                Ok(Some(TokenInfo {
                    id: TokenId::new("tok-7e415d9b2aec8f78d11d2b8c7144b87d").unwrap(),
                    data: TokenData {
                        created_at: Utc::now(),
                        updated_at: Utc::now(),
                        expires_at: Utc::now(),
                        client_id: OAuthClientId::new("client1".to_string()).unwrap(),
                        client_auth: ClientAuth::new(None),
                        device_id: None,
                        sub: Sub::new("sub1").unwrap(),
                        parameters: OAuthAuthorizationRequestParameters {
                            client_id: OAuthClientId::new("client1".to_string()).unwrap(),
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
                        aud: Audience::Single("client".to_string()),
                        preferred_username: None,
                        email: None,
                        email_verified: None,
                        picture: None,
                        name: None,
                    },
                    info: None,
                    current_refresh_token: None,
                }))
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

    async fn create_signer() -> Signer {
        let jwk = JWK {
            common: CommonParameters {
                algorithm: Some(Algorithm::Signature(jwa::SignatureAlgorithm::RS256)),
                key_id: Some("2011-04-29".to_string()),
                ..Default::default()
            },
            algorithm: AlgorithmParameters::RSA(RSAKeyParameters {
                n: BigUint::new(vec![
                    2661337731, 446995658, 1209332140, 183172752, 955894533, 3140848734, 581365968,
                    3217299938, 3520742369, 1559833632, 1548159735, 2303031139, 1726816051,
                    92775838, 37272772, 1817499268, 2876656510, 1328166076, 2779910671, 4258539214,
                    2834014041, 3172137349, 4008354576, 121660540, 1941402830, 1620936445,
                    993798294, 47616683, 272681116, 983097263, 225284287, 3494334405, 4005126248,
                    1126447551, 2189379704, 4098746126, 3730484719, 3232696701, 2583545877,
                    428738419, 2533069420, 2922211325, 2227907999, 4154608099, 679827337,
                    1165541732, 2407118218, 3485541440, 799756961, 1854157941, 3062830172,
                    3270332715, 1431293619, 3068067851, 2238478449, 2704523019, 2826966453,
                    1548381401, 3719104923, 2605577849, 2293389158, 273345423, 169765991,
                    3539762026,
                ]),
                e: BigUint::new(vec![65537]),
                ..Default::default()
            }),
            additional: Default::default(),
        };
        let jose_key = JoseKey::from_jwk(jwk, None).await;
        let issuer = OAuthIssuerIdentifier::new("http://pds.ripperoni.com").unwrap();
        let keyset = Keyset::new(vec![Box::new(jose_key)]);
        let keyset = Arc::new(RwLock::new(keyset));

        let token = SignedJwt::new("eyJ0eXAiOiJKV1QiLCJhbGciOiJSUzI1NiIsImtpZCI6Ik5FTXlNRUZDTXpVd01URTFRVE5CT1VGRE1FUTFPRGN6UmprNU56QkdRelk0UVRrMVEwWkVPUSJ9.eyJpc3MiOiJodHRwczovL2Rldi1lanRsOTg4dy5hdXRoMC5jb20vIiwic3ViIjoiZ1pTeXNwQ1k1ZEk0aDFaM3Fwd3BkYjlUNFVQZEdENWtAY2xpZW50cyIsImF1ZCI6Imh0dHA6Ly9oZWxsb3dvcmxkIiwiaWF0IjoxNTcyNDA2NDQ3LCJleHAiOjE1NzI0OTI4NDcsImF6cCI6ImdaU3lzcENZNWRJNGgxWjNxcHdwZGI5VDRVUGRHRDVrIiwiZ3R5IjoiY2xpZW50LWNyZWRlbnRpYWxzIn0.nupgm7iFqSnERq9GxszwBrsYrYfMuSfUGj8tGQlkY3Ksh3o_IDfq1GO5ngHQLZuYPD-8qPIovPBEVomGZCo_jYvsbjmYkalAStmF01TvSoXQgJd09ygZstH0liKsmINStiRE8fTA-yfEIuBYttROizx-cDoxiindbKNIGOsqf6yOxf7ww8DrTBJKYRnHVkAfIK8wm9LRpsaOVzWdC7S3cbhCKvANjT0RTRpAx8b_AOr_UCpOr8paj-xMT9Zc9HVCMZLBfj6OZ6yVvnC9g6q_SlTa--fY9SL5eqy6-q1JGoyK_-BQ_YrCwrRdrjoJsJ8j-XFRFWJX09W3oDuZ990nGA").unwrap();

        Signer::new(issuer, keyset)
    }

    async fn create_token_manager() -> TokenManager {
        let store: Arc<RwLock<dyn TokenStore>> = Arc::new(RwLock::new(TestStore {}));
        let signer: Arc<RwLock<Signer>> = Arc::new(RwLock::new(create_signer().await));
        let access_token_type = AccessTokenType::JWT;
        let max_age = Some(TOKEN_MAX_AGE);
        let oauth_hooks = OAuthHooks {
            on_client_info: Some(Box::new(
                |client_id: OAuthClientId,
                 oauth_client_metadata: OAuthClientMetadata,
                 jwks: Option<JWKSet<Empty>>|
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
        TokenManager::new(
            store,
            signer,
            access_token_type,
            max_age,
            Arc::new(oauth_hooks),
        )
    }

    #[tokio::test]
    async fn test_create() {
        let token_manager = create_token_manager().await;
        let client = Client {
            id: OAuthClientId::new("https://cleanfollow-bsky.pages.dev/client-metadata.json")
                .unwrap(),
            metadata: OAuthClientMetadata {
                ..Default::default()
            },
            jwks: None,
            info: Default::default(),
        };
        let client_auth = ClientAuth::new(Some(ClientAuthDetails {
            alg: "ES256".to_string(),
            kid: "".to_string(),
            jkt: "".to_string(),
        }));
        let account = Account {
            sub: Sub::new("did:plc:khvyd3oiw46vif5gm7hijslk").unwrap(),
            aud: Audience::Single("did:web:pds.ripperoni.com".to_string()),
            preferred_username: None,
            email: None,
            email_verified: None,
            picture: None,
            name: None,
        };
        let device: Option<(DeviceId, DeviceAccountInfo)> = None;
        let parameters = OAuthAuthorizationRequestParameters {
            client_id: OAuthClientId::new("client123").unwrap(),
            state: None,
            redirect_uri: Some(
                OAuthRedirectUri::new("https://cleanfollow-bsky.pages.dev/").unwrap(),
            ),
            scope: Some(OAuthScope::new("atproto transition:generic").unwrap()),
            response_type: OAuthResponseType::Code,
            code_challenge: Some("E0fpC2Qz0OT-iUqtvwxcuaCciTAgsEuE3XmMlz6hcCk".to_string()),
            code_challenge_method: Some(OAuthCodeChallengeMethod::S256),
            dpop_jkt: Some("9LLGPTQ9RxcuOdbzwsteKXWHF-9VaMqM76on5tcjiMc".to_string()),
            response_mode: Some(ResponseMode::Fragment),
            nonce: None,
            max_age: Some(1),
            claims: None,
            login_hint: Some("ripperoni.com".to_string()),
            ui_locales: None,
            id_token_hint: None,
            display: Some(Display::Page),
            prompt: Some(Prompt::Consent),
            authorization_details: None,
        };
        let dpop_jkt: Option<String> =
            Some("9LLGPTQ9RxcuOdbzwsteKXWHF-9VaMqM76on5tcjiMc".to_string());
        let input = OAuthAuthorizationCodeGrantTokenRequest::new(
            Code::generate(),
            OAuthRedirectUri::new("https://cleanfollow-bsky.pages.dev/").unwrap(),
            Some("E0fpC2Qz0OT-iUqtvwxcuaCciTAgsEuE3XmMlz6hcCk".to_string()),
        )
        .unwrap();
        let result = token_manager
            .create(
                client,
                client_auth,
                account,
                device,
                parameters,
                dpop_jkt,
                CreateTokenInput::Authorization(input),
            )
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
    async fn test_refresh() {
        let token_manager = create_token_manager().await;
        let client = Client {
            id: OAuthClientId::new("client1").unwrap(),
            metadata: OAuthClientMetadata {
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
            jwks: None,
            info: Default::default(),
        };
        let client_auth = ClientAuth::new(Some(ClientAuthDetails {
            alg: "ESS256".to_string(),
            kid: "".to_string(),
            jkt: "".to_string(),
        }));
        let input = OAuthRefreshTokenGrantTokenRequest::new(
            OAuthRefreshToken::new(
                "ref-a41a16a716951a211b9ba177b121ce469128fb4eeb4cea6f10fd949014ab48c4".to_string(),
            )
            .unwrap(),
        );
        let dpop_jkt: Option<String> = Some("".to_string());
        let result = token_manager
            .refresh(client, client_auth, input, dpop_jkt)
            .await
            .unwrap();
        let expected = OAuthTokenResponse {
            access_token: OAuthAccessToken::new("").unwrap(),
            token_type: OAuthTokenType::DPoP,
            scope: Some(OAuthScope::new("".to_string()).unwrap()),
            refresh_token: Some(RefreshToken::new("".to_string()).unwrap()),
            expires_in: None,
            id_token: None,
            authorization_details: None,
            additional_fields: Default::default(),
        };
        assert_eq!(result, expected)
    }

    #[tokio::test]
    async fn test_revoke() {
        let mut token_manager = create_token_manager().await;
        let oauth_token = OAuthTokenIdentification::new(
            "ref-0733f6779a644420c60f2d630adc34d8bc5f2fe048a2747a96e22d519cf6d3ea",
            None,
        )
        .unwrap();
        token_manager.revoke(oauth_token).await.unwrap();
    }

    #[tokio::test]
    async fn test_client_token_info() {
        let mut token_manager = create_token_manager().await;
        let client = Client {
            id: OAuthClientId::new("client123").unwrap(),
            metadata: OAuthClientMetadata {
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
            jwks: None,
            info: Default::default(),
        };
        let client_auth = ClientAuth::new(None);
        let token = OAuthTokenIdentification::new("", None).unwrap();
        let result = token_manager
            .client_token_info(&client, &client_auth, &token)
            .await
            .unwrap();
        let expected = TokenInfo {
            id: TokenId::new("").unwrap(),
            data: TokenData {
                created_at: Utc::now(),
                updated_at: Utc::now(),
                expires_at: Utc::now(),
                client_id: OAuthClientId::new(
                    "https://cleanfollow-bsky.pages.dev/client-metadata.json",
                )
                .unwrap(),
                client_auth: ClientAuth::new(None),
                device_id: None,
                sub: Sub::new("did:plc:khvyd3oiw46vif5gm7hijslk").unwrap(),
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
                sub: Sub::new("did:plc:khvyd3oiw46vif5gm7hijslk").unwrap(),
                aud: Audience::Single("did:web:pds.ripperoni.com".to_string()),
                preferred_username: None,
                email: None,
                email_verified: None,
                picture: None,
                name: None,
            },
            info: None,
            current_refresh_token: None,
        };
        assert_eq!(result, expected)
    }

    #[tokio::test]
    async fn test_get_token_info() {
        let token_manager = create_token_manager().await;
        let token_type: OAuthTokenType = OAuthTokenType::DPoP;
        let token_id: TokenId = TokenId::new("tok-7bddad4f0bf4f788dfb71edc41b4247f").unwrap();
        let result = token_manager
            .get_token_info(token_type, token_id)
            .await
            .unwrap();
        let expected = TokenInfo {
            id: TokenId::new("tok-7bddad4f0bf4f788dfb71edc41b4247f").unwrap(),
            data: TokenData {
                created_at: "2024-11-28T12:00:09Z".parse::<DateTime<Utc>>().unwrap(),
                updated_at: "2024-11-28T12:00:09Z".parse::<DateTime<Utc>>().unwrap(),
                expires_at: "2026-11-28T12:00:09Z".parse::<DateTime<Utc>>().unwrap(),
                client_id: OAuthClientId::new("https://cleanfollow.com").unwrap(),
                client_auth: ClientAuth::new(None),
                device_id: Some(DeviceId::new("dev-64976a0a962c4b7521abd679789c44a8").unwrap()),
                sub: Sub::new("did:plc:khvyd3oiw46vif5gm7hijslk").unwrap(),
                parameters: OAuthAuthorizationRequestParameters {
                    client_id: OAuthClientId::new("https://cleanfollow.com").unwrap(),
                    state: Some("-87HL5S0U7rA71zRMMucDA".to_string()),
                    redirect_uri: Some(
                        OAuthRedirectUri::new("https://cleanfollow-bsky.pages.dev/".to_string())
                            .unwrap(),
                    ),
                    scope: Some(OAuthScope::new("atproto transition:generic".to_string()).unwrap()),
                    response_type: OAuthResponseType::Code,
                    code_challenge: Some("E0fpC2Qz0OT-iUqtvwxcuaCciTAgsEuE3XmMlz6hcCk".to_string()),
                    code_challenge_method: Some(OAuthCodeChallengeMethod::S256),
                    dpop_jkt: Some("9LLGPTQ9RxcuOdbzwsteKXWHF-9VaMqM76on5tcjiMc".to_string()),
                    response_mode: Some(ResponseMode::Fragment),
                    nonce: None,
                    max_age: Some(
                        "2026-11-28T12:00:09Z"
                            .parse::<DateTime<Utc>>()
                            .unwrap()
                            .timestamp() as u32,
                    ),
                    claims: None,
                    login_hint: Some("ripperoni.com".to_string()),
                    ui_locales: None,
                    id_token_hint: None,
                    display: Some(Display::Page),
                    prompt: Some(Prompt::Consent),
                    authorization_details: None,
                },
                details: None,
                code: Some(
                    Code::new(
                        "cod-2fe27accba2e80759d069825d88af324fe0f8c8f9b0e4b67a70b1b624649d78c",
                    )
                    .unwrap(),
                ),
            },
            account: Account {
                sub: Sub::new("did:plc:khvyd3oiw46vif5gm7hijslk").unwrap(),
                aud: Audience::Single("did:web:pds.ripperoni.com".to_string()),
                preferred_username: None,
                email: None,
                email_verified: None,
                picture: None,
                name: None,
            },
            info: None,
            current_refresh_token: Some(
                RefreshToken::new(
                    "ref-82f4c3cc0a01f200cae304154af0a531b3b3e4c18478a238b2d9e224e44b28ac",
                )
                .unwrap(),
            ),
        };
        assert_eq!(result, expected)
    }

    #[tokio::test]
    async fn test_authenticate_token_id() {
        let token_manager = create_token_manager().await;
        let token_type = OAuthTokenType::DPoP;
        let token = TokenId::new("tok-739361c165c76408088de74ee136cf66").unwrap();
        let dpop_jkt: Option<String> = Some("DPoP eyJ0eXAiOiJkcG9wK2p3dCIsImFsZyI6IkVTMjU2IiwiandrIjp7ImFsZyI6IkVTMjU2IiwiY3J2IjoiUC0yNTYiLCJrdHkiOiJFQyIsIngiOiJfVlFPaVBrQ0NHbHFkODljdWJ1UkNTWE01bnJtbUJZTW5fQ0Q5RWNtQUhvIiwieSI6ImNrZTF3TUJYOXNabWktVzBrOTVFa1VSZkNobFk2bWpuUm1TQzhsMElxRG8ifX0.eyJpc3MiOiJodHRwczovL2NsZWFuZm9sbG93LWJza3kucGFnZXMuZGV2L2NsaWVudC1tZXRhZGF0YS5qc29uIiwiaWF0IjoxNzQ0NTk1OTM4LCJqdGkiOiJoNmVvYjVyeXJrOjI0OXc3MjZ5ZjFkc3oiLCJodG0iOiJQT1NUIiwiaHR1IjoiaHR0cHM6Ly9wZHMucmlwcGVyb25pLmNvbS9vYXV0aC9wYXIiLCJub25jZSI6IndYTkFfM283ckZ3X3p2eXpOMHAxVm5RZE8yZUhDenJLMXBiUGt3Yk1qT2MifQ.0k23eIpVoT9Xmb3owTMMSzPkFLe7ULVyd0v_qdHzCNzPM7Z3sA-sOpVWg-Mkx6qutu-7S8Oa4-KL8awB1DKEKA".to_string());
        let result = token_manager
            .authenticate_token_id(token_type, token, dpop_jkt, None)
            .await
            .unwrap();
        let expected = AuthenticateTokenIdResult {
            verify_token_claims_result: VerifyTokenClaimsResult {
                token: OAuthAccessToken::new("").unwrap(),
                token_id: TokenId::new("").unwrap(),
                token_type: OAuthTokenType::DPoP,
                claims: Default::default(),
            },
            token_info: TokenInfo {
                id: TokenId::new("").unwrap(),
                data: TokenData {
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                    expires_at: Utc::now(),
                    client_id: OAuthClientId::new(
                        "https://cleanfollow-bsky.pages.dev/client-metadata.json",
                    )
                    .unwrap(),
                    client_auth: ClientAuth::new(None),
                    device_id: None,
                    sub: Sub::new("did:plc:khvyd3oiw46vif5gm7hijslk").unwrap(),
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
                    sub: Sub::new("did:plc:khvyd3oiw46vif5gm7hijslk").unwrap(),
                    aud: Audience::Single("audience".to_string()),
                    preferred_username: None,
                    email: None,
                    email_verified: None,
                    picture: None,
                    name: None,
                },
                info: None,
                current_refresh_token: None,
            },
        };
        assert_eq!(result, expected)
    }
}
