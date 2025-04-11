use crate::jwk::{Keyset, SignedJwt};
use crate::oauth_provider::access_token::access_token_type::AccessTokenType;
use crate::oauth_provider::dpop::dpop_manager::DpopManager;
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_provider::lib::util::authorization_header::AuthorizationHeader;
use crate::oauth_provider::replay::replay_manager::ReplayManager;
use crate::oauth_provider::replay::replay_store::ReplayStore;
use crate::oauth_provider::replay::replay_store_memory::ReplayStoreMemory;
use crate::oauth_provider::signer::signer::Signer;
use crate::oauth_provider::token::verify_token_claims::{
    verify_token_claims, VerifyTokenClaimsOptions, VerifyTokenClaimsResult,
};
use crate::oauth_types::{OAuthAccessToken, OAuthIssuerIdentifier, OAuthTokenType};
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct OAuthVerifierOptions {
    /**
     * The "issuer" identifier of the OAuth provider, this is the base URL of the
     * OAuth provider.
     */
    pub issuer: OAuthIssuerIdentifier,
    /**
     * The keyset used to sign access tokens.
     */
    pub keyset: Arc<RwLock<Keyset>>,
    /**
     * If set to {@link AccessTokenType.jwt}, the provider will use JWTs for
     * access tokens. If set to {@link AccessTokenType.id}, the provider will
     * use tokenId as access tokens. If set to {@link AccessTokenType.auto},
     * JWTs will only be used if the audience is different from the issuer.
     * Defaults to {@link AccessTokenType.jwt}.
     *
     * Here is a comparison of the two types:
     *
     * - pro id: less CPU intensive (no crypto operations)
     * - pro id: less bandwidth (shorter tokens than jwt)
     * - pro id: token data is in sync with database (e.g. revocation)
     * - pro jwt: stateless: no I/O needed (no db lookups through token store)
     * - pro jwt: stateless: allows Resource Server to be on a different
     *   host/server
     */
    pub access_token_type: Option<AccessTokenType>,
    /**
     * A redis instance to use for replay protection. If not provided, replay
     * protection will use memory storage.
     */
    pub redis: Option<String>,
    pub replay_store: Option<Arc<RwLock<dyn ReplayStore>>>,
}

pub struct OAuthVerifier {
    pub issuer: OAuthIssuerIdentifier,
    pub keyset: Arc<RwLock<Keyset>>,
    pub access_token_type: AccessTokenType,
    pub dpop_manager: DpopManager,
    pub replay_manager: ReplayManager,
    pub signer: Arc<RwLock<Signer>>,
    pub redis: Option<String>,
}

impl OAuthVerifier {
    pub fn new(opts: OAuthVerifierOptions) -> Self {
        let replay_store = match opts.replay_store {
            None => match opts.redis {
                None => Arc::new(RwLock::new(ReplayStoreMemory::new())),
                Some(redis) => {
                    unimplemented!()
                }
            },
            Some(replay_store) => replay_store,
        };
        OAuthVerifier {
            issuer: opts.issuer.clone(),
            keyset: opts.keyset.clone(),
            access_token_type: AccessTokenType::JWT,
            dpop_manager: DpopManager::new(None).unwrap(),
            replay_manager: ReplayManager::new(replay_store),
            signer: Arc::new(RwLock::new(Signer::new(
                opts.issuer.clone(),
                opts.keyset.clone(),
            ))),
            redis: None,
        }
    }

    pub fn next_dpop_nonce(self) {
        self.dpop_manager.next_nonce();
    }

    pub async fn check_dpop_proof(
        &mut self,
        proof: &str,
        htm: &str, // HTTP Method
        htu: &str, // HTTP URL
        access_token: Option<OAuthAccessToken>,
    ) -> Result<String, OAuthError> {
        let res = self
            .dpop_manager
            .check_proof(proof, htm, htu, access_token)
            .await?;

        let unique = self.replay_manager.unique_dpop(res.jti, None).await;
        if !unique {
            return Err(OAuthError::InvalidDpopProofError(
                "DPoP proof jti is not unique".to_string(),
            ));
        }

        Ok(res.jkt)
    }

    pub fn assert_token_type_allowed(
        &self,
        token_type: OAuthTokenType,
        access_token_type: AccessTokenType,
    ) -> Result<(), OAuthError> {
        if self.access_token_type != AccessTokenType::AUTO
            && self.access_token_type != access_token_type
        {
            return Err(OAuthError::InvalidTokenError(
                token_type,
                "Invalid token type".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn authenticate_token(
        &self,
        token_type: OAuthTokenType,
        token: OAuthAccessToken,
        dpop_jkt: Option<String>,
        verify_options: Option<VerifyTokenClaimsOptions>,
    ) -> Result<VerifyTokenClaimsResult, OAuthError> {
        let signed_jwt = match SignedJwt::new(token.clone().into_inner()) {
            Ok(signed_jwt) => signed_jwt,
            Err(_) => {
                return Err(OAuthError::InvalidTokenError(
                    token_type,
                    "Malformed token".to_string(),
                ));
            }
        };

        self.assert_token_type_allowed(token_type.clone(), AccessTokenType::JWT)?;

        let payload = self
            .signer
            .blocking_write()
            .verify_access_token(signed_jwt.clone(), None)
            .await?
            .payload;

        verify_token_claims(
            token,
            payload.jti.clone(),
            token_type,
            dpop_jkt,
            payload.as_token_claims(),
            verify_options,
        )
    }

    pub async fn authenticate_request(
        &mut self,
        method: String,
        url: String,
        headers: (Option<String>, Option<String>),
        verify_options: Option<VerifyTokenClaimsOptions>,
    ) -> Result<VerifyTokenClaimsResult, OAuthError> {
        let authorization_header = match AuthorizationHeader::new(headers.0.unwrap()) {
            Ok(authorization_header) => authorization_header,
            Err(_) => return Err(OAuthError::RuntimeError("".to_string())),
        };
        let token_type = authorization_header.token_type;
        let token = authorization_header.oauth_access_token;

        let dpop_jkt = self
            .check_dpop_proof(
                headers.1.unwrap().as_str(),
                method.as_str(),
                url.as_str(),
                Some(token.clone()),
            )
            .await?;

        self.authenticate_token(token_type, token, Some(dpop_jkt), verify_options)
            .await
    }
}
