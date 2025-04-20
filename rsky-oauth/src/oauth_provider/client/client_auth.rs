use crate::oauth_provider::errors::OAuthError;
use crate::oauth_types::CLIENT_ASSERTION_TYPE_JWT_BEARER;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone, Eq, PartialEq)]
pub struct ClientAuth {
    pub method: String,
    pub alg: String,
    pub kid: String,
    pub jkt: String,
}

pub fn compare_client_auth(a: &ClientAuth, b: &ClientAuth) -> Result<bool, OAuthError> {
    if a.method == "none" {
        if b.method != a.method {
            return Ok(false);
        }

        return Ok(true);
    }

    if a.method == CLIENT_ASSERTION_TYPE_JWT_BEARER {
        if b.method != a.method {
            return Ok(false);
        }

        return Ok(true);
    }

    // Fool-proof
    Err(OAuthError::InvalidClientAuthMethod(
        "Invalid ClientAuth method".to_string(),
    ))
}

pub async fn auth_jwk_thumbprint(key: Vec<u8>) -> String {
    unimplemented!()
}
