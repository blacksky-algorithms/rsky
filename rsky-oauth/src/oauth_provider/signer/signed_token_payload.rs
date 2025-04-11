use crate::jwk::{Audience, JwtPayload};
use crate::oauth_provider::oidc::sub::Sub;
use crate::oauth_provider::token::token_claims::TokenClaims;
use crate::oauth_provider::token::token_id::TokenId;
use crate::oauth_types::OAuthClientId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedTokenPayload {
    pub iat: u64,
    pub iss: String,
    pub aud: Audience,
    pub exp: u64,
    pub jti: TokenId,
    pub sub: Sub,
    pub client_id: OAuthClientId,
    pub nbf: Option<u64>,
    pub htm: Option<String>,
    pub htu: Option<String>,
    pub ath: Option<String>,
    pub acr: Option<String>,
    pub azp: Option<String>,
    pub amr: Option<Vec<String>>,
}

impl SignedTokenPayload {
    pub fn new(jwt_payload: JwtPayload) -> Result<Self, SignedTokenPayloadError> {
        let iat = match jwt_payload.iat {
            None => return Err(SignedTokenPayloadError::Invalid),
            Some(iat) => iat,
        };
        let iss = match jwt_payload.iss {
            None => return Err(SignedTokenPayloadError::Invalid),
            Some(iss) => iss,
        };
        let aud = match jwt_payload.aud {
            None => return Err(SignedTokenPayloadError::Invalid),
            Some(aud) => aud,
        };
        let exp = match jwt_payload.exp {
            None => return Err(SignedTokenPayloadError::Invalid),
            Some(exp) => exp,
        };
        let jti = match jwt_payload.jti {
            None => return Err(SignedTokenPayloadError::Invalid),
            Some(jti) => jti,
        };
        let sub = match jwt_payload.sub {
            None => return Err(SignedTokenPayloadError::Invalid),
            Some(sub) => sub,
        };
        let client_id = match jwt_payload.client_id {
            None => return Err(SignedTokenPayloadError::Invalid),
            Some(client_id) => client_id,
        };
        Ok(Self {
            iat,
            iss,
            aud,
            exp,
            jti,
            sub,
            client_id,
            nbf: jwt_payload.nbf,
            htm: jwt_payload.htm,
            htu: jwt_payload.htu,
            ath: jwt_payload.ath,
            acr: jwt_payload.acr,
            azp: jwt_payload.azp,
            amr: jwt_payload.amr,
        })
    }

    pub fn as_token_claims(&self) -> TokenClaims {
        TokenClaims {
            iss: Some(self.iss.clone()),
            aud: Some(self.aud.clone()),
            sub: Some(self.sub.clone()),
            exp: Some(self.exp.clone()),
            nbf: self.nbf.clone(),
            iat: Some(self.iat.clone()),
            jti: Some(self.jti.clone()),
            htm: self.htm.clone(),
            htu: self.htu.clone(),
            ath: self.ath.clone(),
            acr: self.acr.clone(),
            azp: self.azp.clone(),
            amr: self.amr.clone(),
            cnf: None,
            client_id: Some(self.client_id.clone()),
            scope: None,
            nonce: None,
            at_hash: None,
            c_hash: None,
            s_hash: None,
            auth_time: None,
            name: None,
            family_name: None,
            given_name: None,
            middle_name: None,
            nickname: None,
            preferred_username: None,
            gender: None,
            picture: None,
            profile: None,
            website: None,
            birthdate: None,
            zoneinfo: None,
            locale: None,
            updated_at: None,
            email: None,
            email_verified: None,
            phone_number: None,
            phone_number_verified: None,
            address: None,
            authorization_details: None,
        }
    }
}

/// Errors that can occur when working with token identification.
#[derive(Debug, PartialEq, Eq)]
pub enum SignedTokenPayloadError {
    Invalid,
}
