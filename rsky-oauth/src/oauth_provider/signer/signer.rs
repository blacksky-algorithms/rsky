use crate::jwk::{
    Audience, JwtConfirmation, JwtPayload, Keyset, SignedJwt, VerifyOptions, VerifyResult,
};
use crate::oauth_provider::client::client::Client;
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_types::{
    OAuthAuthorizationDetails, OAuthAuthorizationRequestParameters, OAuthIssuerIdentifier,
};
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::Header;
use serde_json::Value;

#[derive(Clone)]
pub struct Signer {
    pub issuer: OAuthIssuerIdentifier,
    pub keyset: Keyset,
}

impl Signer {
    pub fn new(issuer: OAuthIssuerIdentifier, keyset: Keyset) -> Self {
        Signer { issuer, keyset }
    }

    pub async fn verify(
        &self,
        signed_jwt: &SignedJwt,
        clock_tolerance: bool,
        required_claims: String,
    ) -> VerifyResult {
        unimplemented!()
        // self.keyset.verify_jwt(token).await
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
