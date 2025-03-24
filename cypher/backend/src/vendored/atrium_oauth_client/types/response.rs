use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct OAuthPusehedAuthorizationRequestResponse {
    pub request_uri: String,
    pub expires_in: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum OAuthTokenType {
    DPoP,
    Bearer,
}

// https://datatracker.ietf.org/doc/html/rfc6749#section-5.1
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct OAuthTokenResponse {
    pub access_token: String,
    pub token_type: OAuthTokenType,
    pub expires_in: Option<i64>,
    pub refresh_token: Option<String>,
    pub scope: Option<String>,
    // ATPROTO extension: add the sub claim to the token response to allow
    // clients to resolve the PDS url (audience) using the did resolution
    // mechanism.
    pub sub: Option<String>,
}
