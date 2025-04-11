use crate::jwk::Audience;
use crate::oauth_provider::oidc::sub::Sub;
use crate::oauth_provider::token::token_id::TokenId;
use crate::oauth_types::{OAuthClientId, OAuthScope};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize, Clone, Default, Debug, Eq, PartialEq)]
pub struct TokenClaims {
    pub iss: Option<String>,
    pub aud: Option<Audience>,
    pub sub: Option<Sub>,
    pub exp: Option<u64>,
    pub nbf: Option<u64>,
    pub iat: Option<u64>,
    pub jti: Option<TokenId>,
    pub htm: Option<String>,
    pub htu: Option<String>,
    pub ath: Option<String>,
    pub acr: Option<String>,
    pub azp: Option<String>,
    pub amr: Option<Vec<String>>,

    // https://openid.net/specs/openid-connect-core-1_0.html#StandardClaims
    pub cnf: Option<Value>,

    // https://datatracker.ietf.org/doc/html/rfc7800
    pub client_id: Option<OAuthClientId>,

    pub scope: Option<OAuthScope>,
    pub nonce: Option<String>,

    pub at_hash: Option<String>,
    pub c_hash: Option<String>,
    pub s_hash: Option<String>,
    pub auth_time: Option<u64>,

    // https://openid.net/specs/openid-connect-core-1_0.html#StandardClaims

    // OpenID: "profile" scope
    pub name: Option<String>,
    pub family_name: Option<String>,
    pub given_name: Option<String>,
    pub middle_name: Option<String>,
    pub nickname: Option<String>,
    pub preferred_username: Option<String>,
    pub gender: Option<String>, // OpenID only defines "male" and "female" without forbidding other values
    pub picture: Option<String>,
    pub profile: Option<String>,
    pub website: Option<String>,
    pub birthdate: Option<String>,
    pub zoneinfo: Option<String>,
    pub locale: Option<String>,
    pub updated_at: Option<u64>,

    // OpenID: "email" scope
    pub email: Option<String>,
    pub email_verified: Option<bool>,

    // OpenID: "phone" scope
    pub phone_number: Option<String>,
    pub phone_number_verified: Option<bool>,
    // OpenID: "address" scope
    // https://openid.net/specs/openid-connect-core-1_0.html#AddressClaim
    pub address: Option<Value>,

    // https://datatracker.ietf.org/doc/html/rfc9396#section-14.2
    pub authorization_details: Option<Value>,
}
