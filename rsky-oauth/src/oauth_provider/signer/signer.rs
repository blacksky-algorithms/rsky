use crate::jwk::{
    Audience, JwtConfirmation, JwtPayload, Keyset, SignedJwt, VerifyOptions, VerifyResult,
};
use crate::oauth_provider::client::client::Client;
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_types::{
    OAuthAuthorizationDetails, OAuthAuthorizationRequestParameters, OAuthIssuerIdentifier,
};
use jsonwebtoken::Header;
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
        Box::new(move |keyset: Arc<RwLock<Keyset>>| -> Signer { Signer::new(issuer, keyset) })
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
    }

    pub async fn sign(
        &self,
        sign_header: Header,
        payload: JwtPayload,
    ) -> Result<String, OAuthError> {
        unimplemented!()
        // self.keyset.create_jwt(sign_header, payload).await
    }

    pub async fn access_token(
        &self,
        client: Client,
        parameters: OAuthAuthorizationRequestParameters,
        options: AccessTokenOptions,
    ) -> Result<String, OAuthError> {
        unimplemented!()
        // let mut header = Header::default();
        // header.typ = Some("at+jwt".to_string());
        //
        // let mut payload = JwtPayload::default();
        // payload.aud = Some(options.aud);
        // payload.iat = options.iat;
        // payload.exp = Some(options.exp);
        // payload.sub = Some(options.sub);
        // payload.jti = Some(options.jti);
        // payload.cnf = options.cnf;
        // // // https://datatracker.ietf.org/doc/html/rfc8693#section-4.3
        // payload.client_id = Some(client.id);
        // payload.scope = parameters.scope;
        // // payload.authorization_details = options.authorization_details;
        // //todo
        // self.sign(header, payload).await
    }

    pub async fn verify_access_token(
        &self,
        token: String,
        options: Option<VerifyOptions>,
    ) -> Result<VerifyResult, OAuthError> {
        unimplemented!()
        // let result = self.verify(token.as_str()).await;
    }
}

pub struct AccessTokenOptions {
    pub aud: Audience,
    pub sub: String,
    pub jti: String,
    pub exp: i64,
    pub iat: Option<i64>,
    pub alg: Option<String>,
    pub cnf: Option<JwtConfirmation>,
    pub authorization_details: Option<OAuthAuthorizationDetails>,
}
