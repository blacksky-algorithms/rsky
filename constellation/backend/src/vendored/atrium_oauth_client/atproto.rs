use crate::vendored::atrium_oauth_client::keyset::Keyset;
use crate::vendored::atrium_oauth_client::types::{
    OAuthClientMetadata, TryIntoOAuthClientMetadata,
};
use atrium_xrpc::http::uri::{InvalidUri, Scheme, Uri};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("`client_id` must be a valid URL")]
    InvalidClientId,
    #[error("`grant_types` must include `authorization_code`")]
    InvalidGrantTypes,
    #[error("`scope` must not include `atproto`")]
    InvalidScope,
    #[error("`redirect_uris` must not be empty")]
    EmptyRedirectUris,
    #[error("`private_key_jwt` auth method requires `jwks` keys")]
    EmptyJwks,
    #[error(
        "`private_key_jwt` auth method requires `token_endpoint_auth_signing_alg`, otherwise must not be provided"
    )]
    AuthSigningAlg,
    #[error(transparent)]
    SerdeHtmlForm(#[from] serde_html_form::ser::Error),
    #[error(transparent)]
    LocalhostClient(#[from] LocalhostClientError),
}

#[derive(Error, Debug)]
pub enum LocalhostClientError {
    #[error("invalid redirect_uri: {0}")]
    Invalid(#[from] InvalidUri),
    #[error("loopback client_id must use `http:` redirect_uri")]
    NotHttpScheme,
    #[error("loopback client_id must not use `localhost` as redirect_uri hostname")]
    Localhost,
    #[error("loopback client_id must not use loopback addresses as redirect_uri")]
    NotLoopbackHost,
}

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    None,
    // https://openid.net/specs/openid-connect-core-1_0.html#ClientAuthentication
    PrivateKeyJwt,
}

impl From<AuthMethod> for String {
    fn from(value: AuthMethod) -> Self {
        match value {
            AuthMethod::None => String::from("none"),
            AuthMethod::PrivateKeyJwt => String::from("private_key_jwt"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantType {
    AuthorizationCode,
    RefreshToken,
}

impl From<GrantType> for String {
    fn from(value: GrantType) -> Self {
        match value {
            GrantType::AuthorizationCode => String::from("authorization_code"),
            GrantType::RefreshToken => String::from("refresh_token"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Scope {
    Known(KnownScope),
    Unknown(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum KnownScope {
    #[serde(rename = "atproto")]
    Atproto,
    #[serde(rename = "transition:generic")]
    TransitionGeneric,
    #[serde(rename = "transition:chat.bsky")]
    TransitionChatBsky,
}

impl AsRef<str> for Scope {
    fn as_ref(&self) -> &str {
        match self {
            Self::Known(KnownScope::Atproto) => "atproto",
            Self::Known(KnownScope::TransitionGeneric) => "transition:generic",
            Self::Known(KnownScope::TransitionChatBsky) => "transition:chat.bsky",
            Self::Unknown(value) => value,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AtprotoLocalhostClientMetadata {
    pub redirect_uris: Option<Vec<String>>,
    pub scopes: Option<Vec<Scope>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtprotoClientMetadata {
    pub client_id: String,
    pub client_uri: Option<String>,
    pub redirect_uris: Vec<String>,
    pub token_endpoint_auth_method: AuthMethod,
    pub grant_types: Vec<GrantType>,
    pub scopes: Vec<Scope>,
    pub jwks_uri: Option<String>,
    pub token_endpoint_auth_signing_alg: Option<String>,
}

impl TryIntoOAuthClientMetadata for AtprotoLocalhostClientMetadata {
    type Error = Error;

    fn try_into_client_metadata(self, _: &Option<Keyset>) -> Result<OAuthClientMetadata> {
        // validate redirect_uris
        if let Some(redirect_uris) = &self.redirect_uris {
            for redirect_uri in redirect_uris {
                let uri = redirect_uri
                    .parse::<Uri>()
                    .map_err(LocalhostClientError::Invalid)?;
                if uri.scheme() != Some(&Scheme::HTTP) {
                    return Err(Error::LocalhostClient(LocalhostClientError::NotHttpScheme));
                }
                if uri.host() == Some("localhost") {
                    return Err(Error::LocalhostClient(LocalhostClientError::Localhost));
                }
                if uri
                    .host()
                    .map_or(true, |host| host != "127.0.0.1" && host != "[::1]")
                {
                    return Err(Error::LocalhostClient(
                        LocalhostClientError::NotLoopbackHost,
                    ));
                }
            }
        }
        // determine client_id
        #[derive(serde::Serialize)]
        struct Parameters {
            #[serde(skip_serializing_if = "Option::is_none")]
            redirect_uri: Option<Vec<String>>,
            #[serde(skip_serializing_if = "Option::is_none")]
            scope: Option<String>,
        }
        let query = serde_html_form::to_string(Parameters {
            redirect_uri: self.redirect_uris.clone(),
            scope: self.scopes.map(|scopes| {
                scopes
                    .iter()
                    .map(AsRef::as_ref)
                    .collect::<Vec<_>>()
                    .join(" ")
            }),
        })?;
        let mut client_id = String::from("http://localhost");
        if !query.is_empty() {
            client_id.push_str(&format!("?{query}"));
        }
        Ok(OAuthClientMetadata {
            client_id,
            client_uri: None,
            redirect_uris: self.redirect_uris.unwrap_or(vec![
                String::from("http://127.0.0.1/"),
                String::from("http://[::1]/"),
            ]),
            scope: None,
            grant_types: None, // will be set to `authorization_code` and `refresh_token`
            token_endpoint_auth_method: Some(String::from("none")),
            dpop_bound_access_tokens: None, // will be set to `true`
            jwks_uri: None,
            jwks: None,
            token_endpoint_auth_signing_alg: None,
        })
    }
}

impl TryIntoOAuthClientMetadata for AtprotoClientMetadata {
    type Error = Error;

    fn try_into_client_metadata(self, keyset: &Option<Keyset>) -> Result<OAuthClientMetadata> {
        if self.client_id.parse::<Uri>().is_err() {
            return Err(Error::InvalidClientId);
        }
        if self.redirect_uris.is_empty() {
            return Err(Error::EmptyRedirectUris);
        }
        if !self.grant_types.contains(&GrantType::AuthorizationCode) {
            return Err(Error::InvalidGrantTypes);
        }
        if !self.scopes.contains(&Scope::Known(KnownScope::Atproto)) {
            return Err(Error::InvalidScope);
        }
        let (jwks_uri, mut jwks) = (self.jwks_uri, None);
        match self.token_endpoint_auth_method {
            AuthMethod::None => {
                if self.token_endpoint_auth_signing_alg.is_some() {
                    return Err(Error::AuthSigningAlg);
                }
            }
            AuthMethod::PrivateKeyJwt => {
                if let Some(keyset) = keyset {
                    if self.token_endpoint_auth_signing_alg.is_none() {
                        return Err(Error::AuthSigningAlg);
                    }
                    if jwks_uri.is_none() {
                        jwks = Some(keyset.public_jwks());
                    }
                } else {
                    return Err(Error::EmptyJwks);
                }
            }
        }
        Ok(OAuthClientMetadata {
            client_id: self.client_id,
            client_uri: self.client_uri,
            redirect_uris: self.redirect_uris,
            token_endpoint_auth_method: Some(self.token_endpoint_auth_method.into()),
            grant_types: Some(self.grant_types.into_iter().map(|v| v.into()).collect()),
            scope: Some(
                self.scopes
                    .iter()
                    .map(AsRef::as_ref)
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            dpop_bound_access_tokens: Some(true),
            jwks_uri,
            jwks,
            token_endpoint_auth_signing_alg: self.token_endpoint_auth_signing_alg,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use elliptic_curve::SecretKey;
    use jose_jwk::{Jwk, Key, Parameters};
    use p256::pkcs8::DecodePrivateKey;

    const PRIVATE_KEY: &str = r#"-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgED1AAgC7Fc9kPh5T
4i4Tn+z+tc47W1zYgzXtyjJtD92hRANCAAT80DqC+Z/JpTO7/pkPBmWqIV1IGh1P
gbGGr0pN+oSing7cZ0169JaRHTNh+0LNQXrFobInX6cj95FzEdRyT4T3
-----END PRIVATE KEY-----"#;

    #[test]
    fn test_localhost_client_metadata_default() {
        let metadata = AtprotoLocalhostClientMetadata::default();
        assert_eq!(
            metadata
                .try_into_client_metadata(&None)
                .expect("failed to convert metadata"),
            OAuthClientMetadata {
                client_id: String::from("http://localhost"),
                client_uri: None,
                redirect_uris: vec![
                    String::from("http://127.0.0.1/"),
                    String::from("http://[::1]/"),
                ],
                scope: None,
                grant_types: None,
                token_endpoint_auth_method: Some(AuthMethod::None.into()),
                dpop_bound_access_tokens: None,
                jwks_uri: None,
                jwks: None,
                token_endpoint_auth_signing_alg: None,
            }
        );
    }

    #[test]
    fn test_localhost_client_metadata_custom() {
        let metadata = AtprotoLocalhostClientMetadata {
            redirect_uris: Some(vec![
                String::from("http://127.0.0.1/callback"),
                String::from("http://[::1]/callback"),
            ]),
            scopes: Some(vec![
                Scope::Known(KnownScope::Atproto),
                Scope::Known(KnownScope::TransitionGeneric),
                Scope::Unknown(String::from("unknown")),
            ]),
        };
        assert_eq!(
            metadata
                .try_into_client_metadata(&None)
                .expect("failed to convert metadata"),
            OAuthClientMetadata {
                client_id: String::from(
                    "http://localhost?redirect_uri=http%3A%2F%2F127.0.0.1%2Fcallback&redirect_uri=http%3A%2F%2F%5B%3A%3A1%5D%2Fcallback&scope=atproto+transition%3Ageneric+unknown"
                ),
                client_uri: None,
                redirect_uris: vec![
                    String::from("http://127.0.0.1/callback"),
                    String::from("http://[::1]/callback"),
                ],
                scope: None,
                grant_types: None,
                token_endpoint_auth_method: Some(AuthMethod::None.into()),
                dpop_bound_access_tokens: None,
                jwks_uri: None,
                jwks: None,
                token_endpoint_auth_signing_alg: None,
            }
        );
    }

    #[test]
    fn test_localhost_client_metadata_invalid() {
        {
            let metadata = AtprotoLocalhostClientMetadata {
                redirect_uris: Some(vec![String::from("http://")]),
                ..Default::default()
            };
            let err = metadata
                .try_into_client_metadata(&None)
                .expect_err("expected to fail");
            assert!(matches!(
                err,
                Error::LocalhostClient(LocalhostClientError::Invalid(_))
            ));
        }
        {
            let metadata = AtprotoLocalhostClientMetadata {
                redirect_uris: Some(vec![String::from("https://127.0.0.1/")]),
                ..Default::default()
            };
            let err = metadata
                .try_into_client_metadata(&None)
                .expect_err("expected to fail");
            assert!(matches!(
                err,
                Error::LocalhostClient(LocalhostClientError::NotHttpScheme)
            ));
        }
        {
            let metadata = AtprotoLocalhostClientMetadata {
                redirect_uris: Some(vec![String::from("http://localhost:8000/")]),
                ..Default::default()
            };
            let err = metadata
                .try_into_client_metadata(&None)
                .expect_err("expected to fail");
            assert!(matches!(
                err,
                Error::LocalhostClient(LocalhostClientError::Localhost)
            ));
        }
        {
            let metadata = AtprotoLocalhostClientMetadata {
                redirect_uris: Some(vec![String::from("http://192.168.0.0/")]),
                ..Default::default()
            };
            let err = metadata
                .try_into_client_metadata(&None)
                .expect_err("expected to fail");
            assert!(matches!(
                err,
                Error::LocalhostClient(LocalhostClientError::NotLoopbackHost)
            ));
        }
    }

    #[test]
    fn test_client_metadata() {
        let metadata = AtprotoClientMetadata {
            client_id: String::from("https://example.com/client_metadata.json"),
            client_uri: Some(String::from("https://example.com")),
            redirect_uris: vec![String::from("https://example.com/callback")],
            token_endpoint_auth_method: AuthMethod::PrivateKeyJwt,
            grant_types: vec![GrantType::AuthorizationCode],
            scopes: vec![Scope::Known(KnownScope::Atproto)],
            jwks_uri: None,
            token_endpoint_auth_signing_alg: Some(String::from("ES256")),
        };
        {
            let metadata = metadata.clone();
            let err = metadata
                .try_into_client_metadata(&None)
                .expect_err("expected to fail");
            assert!(matches!(err, Error::EmptyJwks));
        }
        {
            let metadata = metadata.clone();
            let secret_key = SecretKey::<p256::NistP256>::from_pkcs8_pem(PRIVATE_KEY)
                .expect("failed to parse private key");
            let keys = vec![Jwk {
                key: Key::from(&secret_key.into()),
                prm: Parameters {
                    kid: Some(String::from("kid00")),
                    ..Default::default()
                },
            }];
            let keyset = Keyset::try_from(keys.clone()).expect("failed to create keyset");
            assert_eq!(
                metadata
                    .try_into_client_metadata(&Some(keyset.clone()))
                    .expect("failed to convert metadata"),
                OAuthClientMetadata {
                    client_id: String::from("https://example.com/client_metadata.json"),
                    client_uri: Some(String::from("https://example.com")),
                    redirect_uris: vec![String::from("https://example.com/callback"),],
                    scope: Some(String::from("atproto")),
                    grant_types: Some(vec![String::from("authorization_code")]),
                    token_endpoint_auth_method: Some(AuthMethod::PrivateKeyJwt.into()),
                    dpop_bound_access_tokens: Some(true),
                    jwks_uri: None,
                    jwks: Some(keyset.public_jwks()),
                    token_endpoint_auth_signing_alg: Some(String::from("ES256")),
                }
            );
        }
    }

    #[test]
    fn test_scope_serde() {
        #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
        struct Scopes {
            scopes: Vec<Scope>,
        }

        let scopes = Scopes {
            scopes: vec![
                Scope::Known(KnownScope::Atproto),
                Scope::Known(KnownScope::TransitionGeneric),
                Scope::Unknown(String::from("unknown")),
            ],
        };
        let json = serde_json::to_string(&scopes).expect("failed to serialize scopes");
        assert_eq!(
            json,
            r#"{"scopes":["atproto","transition:generic","unknown"]}"#
        );
        let deserialized =
            serde_json::from_str::<Scopes>(&json).expect("failed to deserialize scopes");
        assert_eq!(deserialized, scopes);
    }
}
