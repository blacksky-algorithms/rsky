use crate::jwk::{SignedJwt, VerifyOptions};
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
use crate::oauth_provider::device::device_manager::DeviceManagerOptions;
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_provider::now_as_secs;
use crate::oauth_provider::request::code::Code;
use crate::oauth_provider::signer::signer::{AccessTokenOptions, Signer};
use crate::oauth_provider::token::refresh_token::{generate_refresh_token, RefreshToken};
use crate::oauth_provider::token::token_claims::TokenClaims;
use crate::oauth_provider::token::token_id::{generate_token_id, TokenId};
use crate::oauth_provider::token::token_store::{NewTokenData, TokenInfo, TokenStore};
use crate::oauth_provider::token::verify_token_claims::{
    verify_token_claims, VerifyTokenClaimsOptions, VerifyTokenClaimsResult,
};
use crate::oauth_types::{
    OAuthAccessToken, OAuthAuthorizationCodeGrantTokenRequest, OAuthAuthorizationDetails,
    OAuthAuthorizationRequestParameters, OAuthClientCredentialsGrantTokenRequest, OAuthGrantType,
    OAuthPasswordGrantTokenRequest, OAuthRefreshTokenGrantTokenRequest, OAuthTokenIdentification,
    OAuthTokenResponse, OAuthTokenType, CLIENT_ASSERTION_TYPE_JWT_BEARER,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct AuthenticateTokenIdResult {
    verify_token_claims_result: VerifyTokenClaimsResult,
    token_info: TokenInfo,
}

enum CreateTokenInput {
    Authorization(OAuthAuthorizationCodeGrantTokenRequest),
    Client(OAuthClientCredentialsGrantTokenRequest),
    Password(OAuthPasswordGrantTokenRequest),
}

pub struct TokenManager {
    pub store: Arc<RwLock<dyn TokenStore>>,
    pub signer: Arc<RwLock<Signer>>,
    pub access_token_type: AccessTokenType,
    pub token_max_age: u64,
}

pub type TokenManagerCreator = Box<
    dyn Fn(Arc<RwLock<dyn TokenStore>>, Option<DeviceManagerOptions>) -> TokenManager + Send + Sync,
>;

impl TokenManager {
    pub fn creator() -> TokenManagerCreator {
        Box::new(
            move |store: Arc<RwLock<dyn TokenStore>>,
                  signer: Arc<RwLock<Signer>>,
                  access_token_type: AccessTokenType,
                  max_age: Option<u64>|
                  -> TokenManager {
                TokenManager::new(store, signer, access_token_type, max_age)
            },
        )
    }

    pub fn new(
        store: Arc<RwLock<dyn TokenStore>>,
        signer: Arc<RwLock<Signer>>,
        access_token_type: AccessTokenType,
        max_age: Option<u64>,
    ) -> Self {
        let token_max_age = max_age.unwrap_or_else(|| TOKEN_MAX_AGE);
        TokenManager {
            store,
            signer,
            access_token_type,
            token_max_age,
        }
    }

    fn create_token_expiry(&self, now: Option<u64>) -> u64 {
        let time = now_as_secs();
        let now = now.unwrap_or_else(|| time);
        now + self.token_max_age
    }

    fn use_jwt_access_token(&self, account: Account) -> bool {
        if self.access_token_type == AccessTokenType::AUTO {
            return if account.aud.len() == 1 {
                self.signer
                    .blocking_read()
                    .issuer
                    .to_string()
                    .eq(account.aud.get(0).unwrap())
            } else {
                false
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
        input: Option<String>,
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
            if (dpop_jkt.is_some()) {
                parameters.dpop_jkt = dpop_jkt;
            }
        } else if parameters.dpop_jkt != dpop_jkt {
            return Err(OAuthError::InvalidDpopKeyBindingError);
        }

        if client_auth.method == CLIENT_ASSERTION_TYPE_JWT_BEARER {
            // Clients **must not** use their private key to sign DPoP proofs.
            if parameters.dpop_jkt.is_some() && client_auth.jkt == parameters.dpop_jkt.unwrap() {
                return Err(OAuthError::InvalidRequestError(
                    "The DPoP proof must be signed with a different key than the client assertion"
                        .to_string(),
                ));
            }
        }

        unimplemented!()
        // if  client.metadata.grant_types.contains(input)
    }

    async fn build_token_response(
        &self,
        access_token: OAuthAccessToken,
        refresh_token: Option<RefreshToken>,
        expires_at: u64,
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
            expires_in: Some(expires_at),
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
        if token_info.data.client_id != client.id {
            return Err(OAuthError::InvalidGrantError(
                "Token was not issued to this client".to_string(),
            ));
        }

        if let Some(info) = token_info.info.clone() {
            if !info.authorized_clients.contains(&client.id) {
                return Err(OAuthError::InvalidGrantError(
                    "Client no longer trusted by user".to_string(),
                ));
            }
        }

        if token_info.data.client_auth.method != client_auth.method {
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

        let token_info = match self
            .store
            .blocking_read()
            .find_token_by_refresh_token(refresh_token.clone())?
        {
            None => {
                return Err(OAuthError::InvalidGrantError(
                    "Invalid refresh token".to_string(),
                ))
            }
            Some(token_info) => token_info,
        };

        let account = token_info.account;
        let data = token_info.data;
        let parameters = data.parameters;

        if let Some(token_refresh_token) = token_info.current_refresh_token {
            if token_refresh_token != refresh_token {
                self.store.blocking_write().delete_token(token_info.id)?;
                return Err(OAuthError::InvalidGrantError(
                    "refresh token replayed".to_string(),
                ));
            }
        } else {
            self.store.blocking_write().delete_token(token_info.id)?;
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
            // In case the client metadata was updated after the token was issued
            self.store.blocking_write().delete_token(token_info.id)?;
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
                self.store.blocking_write().delete_token(token_info.id)?;
                return Err(OAuthError::InvalidDpopProofError(
                    "DPoP proof required".to_string(),
                ));
            }
        }

        let last_activity = data.updated_at;
        let inactivity_timeout = if client_auth.method == "none" && !client.info.is_first_party {
            UNAUTHENTICATED_REFRESH_INACTIVITY_TIMEOUT
        } else {
            AUTHENTICATED_REFRESH_INACTIVITY_TIMEOUT
        };
        if last_activity + inactivity_timeout < now_as_secs() {
            self.store.blocking_write().delete_token(token_info.id)?;
            return Err(OAuthError::InvalidGrantError(
                "Refresh token exceeded inactivity timeout".to_string(),
            ));
        }

        let lifetime = if client_auth.method == "none" && !client.info.is_first_party {
            UNAUTHENTICATED_REFRESH_LIFETIME
        } else {
            AUTHENTICATED_REFRESH_LIFETIME
        };
        if data.created_at + lifetime < now_as_secs() {
            self.store.blocking_write().delete_token(token_info.id)?;
            return Err(OAuthError::InvalidGrantError(
                "Refresh token expired".to_string(),
            ));
        }

        let authorization_details;

        let next_token_id = generate_token_id();
        let next_refresh_token = generate_refresh_token().await;

        let now = now_as_secs();
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
            updated_at: now,
        };
        self.store.blocking_write().rotate_token(
            token_info.id,
            next_token_id.clone(),
            next_refresh_token.clone(),
            new_token_data,
        )?;

        let access_token = match !self.use_jwt_access_token(account.clone()) {
            true => next_token_id.val(),
            false => {
                // We don't specify the alg here. We suppose the Resource server will be
                // able to verify the token using any alg.
                let authorization_details = match authorization_details {
                    None => None,
                    Some(details) => Some(details.clone()),
                };
                let options = AccessTokenOptions {
                    aud: account.aud.clone(),
                    sub: account.sub.clone(),
                    jti: next_token_id,
                    exp: expires_at as i64,
                    iat: Some(now as i64),
                    alg: None,
                    cnf: None,
                    authorization_details: Some(authorization_details),
                };
                self.signer
                    .blocking_read()
                    .access_token(client.clone(), parameters.clone(), options)
                    .await
            }
        };

        self.build_token_response(
            client,
            access_token,
            Some(next_refresh_token),
            expires_at,
            parameters,
            account,
            authorization_details,
        )
        .await
    }

    /**
     * @see {@link https://datatracker.ietf.org/doc/html/rfc7009#section-2.2 | RFC7009 Section 2.2}
     */
    pub async fn revoke(&mut self, token: &OAuthTokenIdentification) -> Result<(), OAuthError> {
        let token = token.token.clone();
        if let Ok(token_id) = TokenId::new(token.as_str()) {
            self.store.blocking_write().delete_token(token_id)?;
        } else if let Ok(signed_jwt) = SignedJwt::new(token.as_str()) {
            let options = VerifyOptions {
                audience: None,
                clock_tolerance: None,
                issuer: None,
                max_token_age: None,
                subject: None,
                typ: None,
                current_date: None,
                required_claims: vec!["jti".to_string()],
            };
            let verify_result = self
                .signer
                .blocking_read()
                .verify(signed_jwt, Some(options))
                .await;
            let token_id = match TokenId::new(verify_result.payload) {
                Ok(token_id) => token_id,
                Err(_) => return Err(OAuthError::RuntimeError("".to_string())),
            };
            self.store.blocking_write().delete_token(token_id)?;
            return Ok(());
        } else if let Ok(refresh_token) = RefreshToken::new(token.as_str()) {
            let token_info = self
                .store
                .blocking_read()
                .find_token_by_refresh_token(refresh_token)?;
            if let Some(token_info) = token_info {
                self.store.blocking_write().delete_token(token_info.id)?;
            }
        } else if let Ok(code) = Code::new(token.as_str()) {
            let token_info = self.store.blocking_read().find_token_by_code(code)?;
            if let Some(token_info) = token_info {
                self.store.blocking_write().delete_token(token_info.id)?;
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
            Ok(res) => {}
            Err(e) => {
                self.store.blocking_write().delete_token(token_info.id)?;
                return Err(e);
            }
        }

        let now = 1;
        if token_info.data.expires_at < now {
            return Err(OAuthError::InvalidGrantError("Token expired".to_string()));
        }

        Ok(token_info)
    }

    async fn find_token_info(
        &self,
        token: &OAuthTokenIdentification,
    ) -> Result<Option<TokenInfo>, OAuthError> {
        let token_val = token.token.clone();

        if let Ok(token_id) = TokenId::new(token_val.as_str()) {
            return self.store.blocking_read().read_token(token_id);
        }

        if let Ok(signed_jwt) = SignedJwt::new(token_val.as_str()) {
            let payload = match self
                .signer
                .blocking_read()
                .verify_access_token(signed_jwt, None)
                .await
            {
                Ok(response) => response.payload,
                Err(_) => return Ok(None),
            };

            let token_info = match self.store.blocking_read().read_token(payload.jti)? {
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
            let token_info = self
                .store
                .blocking_read()
                .find_token_by_refresh_token(refresh_token)?;
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
        let token_info = self.store.blocking_read().read_token(token_id)?;

        match token_info {
            None => Err(OAuthError::InvalidTokenError(
                token_type,
                "Invalid token".to_string(),
            )),
            Some(token_info) => {
                if token_info.data.expires_at > 1u64 {
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
        let token_info = self.get_token_info(token_type, token).await?;
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
            exp: Some(token_info.data.expires_at.clone()),
            iat: Some(token_info.data.updated_at.clone()),
            scope: token_info.data.parameters.scope.clone(),
            client_id: Some(token_info.data.client_id.clone()),
            cnf,
            ..Default::default()
        };

        let result = verify_token_claims(token, token_type, dpop_jkt, claims, verify_options)?;

        Ok(AuthenticateTokenIdResult {
            verify_token_claims_result: result,
            token_info,
        })
    }
}
