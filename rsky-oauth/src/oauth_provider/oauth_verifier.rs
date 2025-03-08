use crate::jwk::Keyset;
use crate::oauth_provider::access_token::access_token_type::AccessTokenType;
use crate::oauth_provider::dpop::dpop_manager::DpopManager;
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_provider::replay::replay_manager::ReplayManager;
use crate::oauth_provider::replay::replay_store_memory::ReplayStoreMemory;
use crate::oauth_provider::signer::signer::Signer;
use crate::oauth_provider::token::verify_token_claims::{
    verify_token_claims, VerifyTokenClaimsOptions, VerifyTokenClaimsResult,
};
use crate::oauth_types::{OAuthAccessToken, OAuthIssuerIdentifier, OAuthTokenType};
use std::any::Any;

pub struct OAuthVerifierOptions {
    /**
     * The "issuer" identifier of the OAuth provider, this is the base URL of the
     * OAuth provider.
     */
    pub issuer: OAuthIssuerIdentifier,
    /**
     * The keyset used to sign access tokens.
     */
    pub keyset: Keyset,
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
    pub replay_store: Option<ReplayStoreMemory>,
}

pub struct OAuthVerifier {
    pub issuer: OAuthIssuerIdentifier,
    pub keyset: Keyset,
    pub access_token_type: AccessTokenType,
    pub dpop_manager: DpopManager,
    pub replay_manager: ReplayManager,
    pub signer: Signer,
    pub redis: Option<String>,
}

impl OAuthVerifier {
    pub fn new(opts: OAuthVerifierOptions) -> Self {
        OAuthVerifier {
            issuer: opts.issuer.clone(),
            keyset: opts.keyset.clone(),
            access_token_type: AccessTokenType::JWT,
            dpop_manager: DpopManager::new(None),
            replay_manager: ReplayManager::new(ReplayStoreMemory::new()),
            signer: Signer::new(opts.issuer.clone(), opts.keyset.clone()),
            redis: None,
        }
    }

    pub fn next_dpop_nonce(&self) {
        self.dpop_manager.next_nonce();
    }

    pub async fn check_dpop_proof(
        &self,
        proof: &str,
        htm: &str, // HTTP Method
        htu: &str, // HTTP URL
        access_token: Option<OAuthAccessToken>,
    ) -> Option<String> {
        let res = self
            .dpop_manager
            .check_proof(proof, htm, htu, access_token)
            .await;
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
        unimplemented!()
        // if !is_signed_jwt(token.as_str()) {
        //     return Err(OAuthError::InvalidTokenError(
        //         token_type,
        //         "Malformed token".to_string(),
        //     ));
        // }
        //
        // self.assert_token_type_allowed(token_type.clone(), AccessTokenType::JWT)?;
        //
        // let payload = self.signer.verify_access_token(token.clone(), None)?;
        //
        // verify_token_claims(
        //     token,
        //     payload.jti,
        //     token_type,
        //     dpop_jkt,
        //     payload,
        //     verify_options
        // )
    }

    pub async fn authenticate_request(
        &self,
        method: String,
        url: String,
        headers: (Option<String>, Option<String>),
        verify_options: Option<VerifyTokenClaimsOptions>,
    ) -> Result<VerifyTokenClaimsResult, OAuthError> {
        unimplemented!()
        // let (token_type, token) = parse_authorization_header(headers.0);
        // let dpop_jkt = self
        //     .check_dpop_proof(headers.1.unwrap(), method, url, Some(token.clone()))
        //     .await;
        //
        // if token_type.type_id() == OAuthTokenType::DPoP.type_id() && dpop_jkt.is_none() {
        //     return Err(OAuthError::InvalidDpopProofError(
        //         "DPoP proof required".to_string(),
        //     ));
        // }
        //
        // self.authenticate_token(token_type, token, dpop_jkt, verify_options)
        //     .await
    }
}
