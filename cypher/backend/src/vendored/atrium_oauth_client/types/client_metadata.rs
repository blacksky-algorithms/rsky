use crate::vendored::atrium_oauth_client::keyset::Keyset;
use jose_jwk::JwkSet;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct OAuthClientMetadata {
    pub client_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_uri: Option<String>,
    pub redirect_uris: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grant_types: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_endpoint_auth_method: Option<String>,
    // https://datatracker.ietf.org/doc/html/rfc9449#section-5.2
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dpop_bound_access_tokens: Option<bool>,
    // https://datatracker.ietf.org/doc/html/rfc7591#section-2
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jwks_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jwks: Option<JwkSet>,
    // https://openid.net/specs/openid-connect-registration-1_0.html#ClientMetadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_endpoint_auth_signing_alg: Option<String>,
}

pub trait TryIntoOAuthClientMetadata {
    type Error;

    fn try_into_client_metadata(
        self,
        keyset: &Option<Keyset>,
    ) -> core::result::Result<OAuthClientMetadata, Self::Error>;
}
