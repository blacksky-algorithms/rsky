use crate::jwk::{
    Audience, JwtConfirmation, JwtPayload, Keyset, SignedJwt, VerifyOptions, VerifyResult,
};
use crate::oauth_provider::client::client::Client;
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_provider::oidc::sub::Sub;
use crate::oauth_provider::signer::signed_token_payload::SignedTokenPayload;
use crate::oauth_provider::token::token_id::TokenId;
use crate::oauth_types::{
    OAuthAuthorizationDetails, OAuthAuthorizationRequestParameters, OAuthIssuerIdentifier,
};
use jsonwebtoken::{Algorithm, Header};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct Signer {
    pub issuer: OAuthIssuerIdentifier,
    pub keyset: Arc<RwLock<Keyset>>,
}

pub type SignerCreator = Box<dyn Fn(Arc<RwLock<Keyset>>) -> Signer + Send + Sync>;

impl Signer {
    pub fn creator(issuer: OAuthIssuerIdentifier) -> SignerCreator {
        Box::new(move |keyset: Arc<RwLock<Keyset>>| -> Signer {
            Signer::new(issuer.clone(), keyset)
        })
    }

    pub fn new(issuer: OAuthIssuerIdentifier, keyset: Arc<RwLock<Keyset>>) -> Self {
        Signer { issuer, keyset }
    }

    pub async fn verify(
        &self,
        signed_jwt: SignedJwt,
        verify_options: Option<VerifyOptions>,
    ) -> VerifyResult {
        self.keyset
            .blocking_read()
            .verify_jwt(signed_jwt, verify_options)
            .await
            .unwrap()
    }

    pub async fn sign(&self, sign_header: Header, payload: JwtPayload) -> SignedJwt {
        self.keyset
            .blocking_write()
            .create_jwt(sign_header, payload)
            .await
    }

    pub async fn access_token(
        &self,
        client: Client,
        parameters: OAuthAuthorizationRequestParameters,
        options: AccessTokenOptions,
    ) -> SignedJwt {
        let mut header = Header::default();
        header.typ = Some("at+jwt".to_string());
        header.alg = options.alg.unwrap();

        let mut payload = JwtPayload::default();
        payload.aud = Some(options.aud);
        payload.iat = options.iat;
        payload.exp = Some(options.exp);
        payload.sub = Some(options.sub);
        payload.jti = Some(options.jti);
        payload.cnf = options.cnf;
        // // https://datatracker.ietf.org/doc/html/rfc8693#section-4.3
        payload.client_id = Some(client.id);
        payload.scope = parameters.scope;

        // TODO
        // payload.authorization_details = options.authorization_details.unwrap();

        self.sign(header, payload).await
    }

    pub async fn verify_access_token(
        &self,
        token: SignedJwt,
        options: Option<VerifyOptions>,
    ) -> Result<VerifyAccessTokenResponse, OAuthError> {
        let options = match options {
            None => VerifyOptions {
                audience: None,
                clock_tolerance: None,
                issuer: None,
                max_token_age: None,
                subject: None,
                typ: Some("at+jwt".to_string()),
                current_date: None,
                required_claims: vec![],
            },
            Some(options) => {
                let mut options = options.clone();
                options.issuer = None;
                options.typ = Some("at+jwt".to_string());
                options
            }
        };
        let result = self.verify(token, Some(options)).await;
        let protected_header = result.protected_header;
        let payload = match SignedTokenPayload::new(result.payload) {
            Ok(payload) => payload,
            Err(e) => return Err(OAuthError::InvalidRequestError("Bad payload".to_string())),
        };
        Ok(VerifyAccessTokenResponse {
            protected_header,
            payload,
        })
    }
}

pub struct AccessTokenOptions {
    pub aud: Audience,
    pub sub: Sub,
    pub jti: TokenId,
    pub exp: i64,
    pub iat: Option<i64>,
    pub alg: Option<Algorithm>,
    pub cnf: Option<JwtConfirmation>,
    pub authorization_details: Option<OAuthAuthorizationDetails>,
}

pub struct VerifyAccessTokenResponse {
    pub protected_header: Header,
    pub payload: SignedTokenPayload,
}
