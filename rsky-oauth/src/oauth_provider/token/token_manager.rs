use crate::jwk::SignedJwt;
use crate::oauth_provider::access_token::access_token_type::AccessTokenType;
use crate::oauth_provider::account::account::Account;
use crate::oauth_provider::account::account_store::DeviceAccountInfo;
use crate::oauth_provider::client::client::Client;
use crate::oauth_provider::client::client_auth::ClientAuth;
use crate::oauth_provider::constants::TOKEN_MAX_AGE;
use crate::oauth_provider::device::device_id::DeviceId;
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_provider::now_as_secs;
use crate::oauth_provider::request::code::Code;
use crate::oauth_provider::signer::signer::Signer;
use crate::oauth_provider::token::refresh_token::{is_refresh_token, RefreshToken};
use crate::oauth_provider::token::token_claims::TokenClaims;
use crate::oauth_provider::token::token_id::{is_token_id, TokenId};
use crate::oauth_provider::token::token_store::{TokenInfo, TokenStore};
use crate::oauth_provider::token::verify_token_claims::{
    verify_token_claims, VerifyTokenClaimsOptions, VerifyTokenClaimsResult,
};
use crate::oauth_types::{
    OAuthAccessToken, OAuthAuthorizationCodeGrantTokenRequest, OAuthAuthorizationDetails,
    OAuthAuthorizationRequestParameters, OAuthClientCredentialsGrantTokenRequest,
    OAuthPasswordGrantTokenRequest, OAuthRefreshTokenGrantTokenRequest, OAuthTokenIdentification,
    OAuthTokenResponse, OAuthTokenType, CLIENT_ASSERTION_TYPE_JWT_BEARER,
};
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
    pub signer: Signer,
    pub access_token_type: AccessTokenType,
    pub token_max_age: u64,
}

impl TokenManager {
    pub fn new(
        store: Arc<RwLock<dyn TokenStore>>,
        signer: Signer,
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
        client: Client,
        access_token: OAuthAccessToken,
        refresh_token: Option<String>,
        expires_at: u64,
        parameters: OAuthAuthorizationRequestParameters,
        account: Account,
        authorization_details: Option<OAuthAuthorizationDetails>,
    ) -> OAuthTokenResponse {
        unimplemented!()
    }

    async fn validate_access(
        &self,
        client: &Client,
        client_auth: &ClientAuth,
        token_info: &TokenInfo,
    ) -> Result<(), OAuthError> {
        unimplemented!()
    }

    pub async fn refresh(
        &self,
        client: Client,
        client_auth: ClientAuth,
        input: OAuthRefreshTokenGrantTokenRequest,
        dpop_jkt: Option<String>,
    ) -> OAuthTokenResponse {
        //Refresh Token Parsed

        // let token_info = self.store.blocking_read().find_token_by_refresh_token(input.refresh_token())?;
        unimplemented!()
    }

    /**
     * @see {@link https://datatracker.ietf.org/doc/html/rfc7009#section-2.2 | RFC7009 Section 2.2}
     */
    pub async fn revoke(&mut self, token: &OAuthTokenIdentification) -> Result<(), OAuthError> {
        unimplemented!()
        // if is_token_id(token.token.as_str()) {
        //     let token_id = TokenId::new(token.token.clone());
        //     self.store.blocking_write().delete_token(token_id)?;
        // } else if let Ok(signed_jwt) = SignedJwt::new(token.token.as_str()) {
        //     let verify_result = self.signer.verify(token.token.as_str()).await;
        //     // let token_id = TokenId::new(verify_result.payload)
        //     unimplemented!()
        // } else if let Ok(refresh_token) = RefreshToken::new(token.token.as_str()) {
        //     let token_info = self
        //         .store
        //         .blocking_read()
        //         .find_token_by_refresh_token(refresh_token)?;
        //     if let Some(token_info) = token_info {
        //         self.store.blocking_write().delete_token(token_info.id)?;
        //     }
        // } else if let Ok(code) = Code::new(token.token.as_str()) {
        //     let token_info = self.store.blocking_read().find_token_by_code(code)?;
        //     if let Some(token_info) = token_info {
        //         self.store.blocking_write().delete_token(token_info.id)?;
        //     }
        // } else {
        //     // No error should be returned if the token is not valid
        // }
        // Ok(())
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
        unimplemented!()
        // let token_val = token.token.clone();
        //
        // if is_token_id(token_val.as_str()) {
        //     return self.store.blocking_read().read_token(token_val.as_str());
        // }
        //
        // return Ok(None);
        // // if is_signed_jwt {  }
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
        unimplemented!()
        // let token_info = self.get_token_info(token_type, token).await?;
        // let parameters = token_info.data.clone();
        //
        // // Construct a list of claim, as if the token was a JWT.
        // let claims = TokenClaims {
        //     aud: Some(token_info.account.aud.clone()),
        //     sub: Some(token_info.account.sub.clone()),
        //     exp: Some(token_info.data.expires_at.clone()),
        //     iat: Some(token_info.data.updated_at.clone()),
        //     scope: token_info.data.parameters.scope.clone(),
        //     client_id: Some(token_info.data.client_id.clone()),
        //     cnf: None,
        //     ..Default::default()
        // };
        //
        // let result =
        //     verify_token_claims(token, token, token_type, dpop_jkt, claims, verify_options)?;
        //
        // Ok(AuthenticateTokenIdResult {
        //     verify_token_claims_result: result,
        //     token_info,
        // })
    }
}
