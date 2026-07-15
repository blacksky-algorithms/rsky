use crate::error::OAuthError;
use crate::jwk::JwkSet;
use crate::jwt;
use crate::types::*;
use std::collections::HashSet;
use std::sync::Arc;
use url::{Host, Url};

pub const LOOPBACK_CLIENT_ID_ORIGIN: &str = "http://localhost";
/// Maximum age of a `private_key_jwt` client assertion, in seconds.
pub const CLIENT_ASSERTION_MAX_AGE: u64 = 60;
pub const DEFAULT_LOOPBACK_REDIRECT_URIS: [&str; 2] = ["http://127.0.0.1/", "http://[::1]/"];

/// Fetches client metadata documents and JWK sets over HTTPS. The host
/// application injects an SSRF-hardened HTTP client.
#[async_trait::async_trait]
pub trait ClientMetadataFetcher: Send + Sync {
    async fn fetch_client_metadata(&self, url: &str) -> Result<OAuthClientMetadata, OAuthError>;
    async fn fetch_jwks(&self, url: &str) -> Result<JwkSet, OAuthError>;
}

/// A resolved and validated OAuth client.
#[derive(Debug, Clone, PartialEq)]
pub struct Client {
    pub id: String,
    pub metadata: OAuthClientMetadata,
    /// Resolved key set for `private_key_jwt` clients.
    pub jwks: Option<JwkSet>,
}

impl Client {
    /// Authenticates the client per its registered
    /// `token_endpoint_auth_method`.
    pub fn authenticate(
        &self,
        client_assertion_type: Option<&str>,
        client_assertion: Option<&str>,
        issuer: &str,
        now: u64,
    ) -> Result<ClientAuth, OAuthError> {
        if !self.metadata.is_confidential() {
            return Ok(ClientAuth::None);
        }
        match client_assertion_type {
            Some(CLIENT_ASSERTION_TYPE_JWT_BEARER) => {}
            _ => {
                return Err(OAuthError::InvalidClient(format!(
                    "client_assertion_type must be \"{CLIENT_ASSERTION_TYPE_JWT_BEARER}\""
                )))
            }
        }
        let Some(assertion) = client_assertion else {
            return Err(OAuthError::InvalidClient(
                "client_assertion is required".to_string(),
            ));
        };
        let decoded = jwt::decode(assertion)
            .map_err(|e| OAuthError::InvalidClient(format!("invalid client assertion: {e}")))?;
        let Some(kid) = decoded.header.kid.clone() else {
            return Err(OAuthError::InvalidClient(
                "client assertion missing \"kid\" header".to_string(),
            ));
        };
        let Some(key) = self.jwks.as_ref().and_then(|jwks| jwks.find_by_kid(&kid)) else {
            return Err(OAuthError::InvalidClient(format!(
                "no key found for kid \"{kid}\""
            )));
        };
        jwt::verify_signature(&decoded, key)
            .map_err(|e| OAuthError::InvalidClient(format!("invalid client assertion: {e}")))?;
        let claims = &decoded.claims;
        claims
            .validate_iss(&self.id)
            .and_then(|()| claims.validate_aud(issuer))
            .map_err(|e| OAuthError::InvalidClient(format!("invalid client assertion: {e}")))?;
        if claims.sub.as_deref() != Some(self.id.as_str()) {
            return Err(OAuthError::InvalidClient(
                "client assertion \"sub\" must be the client_id".to_string(),
            ));
        }
        if claims.jti.as_deref().unwrap_or_default().is_empty() {
            return Err(OAuthError::InvalidClient(
                "client assertion missing \"jti\"".to_string(),
            ));
        }
        let Some(iat) = claims.iat else {
            return Err(OAuthError::InvalidClient(
                "client assertion missing \"iat\"".to_string(),
            ));
        };
        if iat > now || now - iat > CLIENT_ASSERTION_MAX_AGE {
            return Err(OAuthError::InvalidClient(
                "client assertion expired".to_string(),
            ));
        }
        Ok(ClientAuth::PrivateKeyJwt {
            alg: decoded.header.alg.clone(),
            kid,
            jkt: key.thumbprint(),
        })
    }

    /// Validates authorization request parameters against the client's
    /// registered metadata, filling in a defaulted `redirect_uri`.
    pub fn validate_request(
        &self,
        parameters: &ParRequest,
    ) -> Result<AuthorizationRequestParameters, OAuthError> {
        if parameters.client_id != self.id {
            return Err(OAuthError::InvalidRequest(
                "client_id does not match".to_string(),
            ));
        }
        if parameters.response_type != RESPONSE_TYPE_CODE {
            return Err(OAuthError::InvalidRequest(format!(
                "unsupported response_type \"{}\"",
                parameters.response_type
            )));
        }
        if !self
            .metadata
            .response_types
            .iter()
            .any(|rt| rt == RESPONSE_TYPE_CODE)
        {
            return Err(OAuthError::InvalidRequest(
                "client metadata does not declare the \"code\" response type".to_string(),
            ));
        }
        let scope = validate_requested_scope(parameters.scope.as_deref(), &self.metadata)?;
        let (code_challenge, code_challenge_method) = match (
            parameters.code_challenge.as_deref(),
            parameters.code_challenge_method.as_deref(),
        ) {
            (Some(challenge), Some(CODE_CHALLENGE_METHOD_S256)) if !challenge.is_empty() => (
                challenge.to_string(),
                CODE_CHALLENGE_METHOD_S256.to_string(),
            ),
            (Some(_), Some(other)) => {
                return Err(OAuthError::InvalidRequest(format!(
                    "unsupported code_challenge_method \"{other}\""
                )))
            }
            (Some(_), None) => {
                return Err(OAuthError::InvalidRequest(
                    "code_challenge_method is required".to_string(),
                ))
            }
            _ => {
                return Err(OAuthError::InvalidRequest(
                    "use of PKCE is required".to_string(),
                ))
            }
        };
        let redirect_uri = match parameters.redirect_uri.as_deref() {
            Some(redirect_uri) => {
                if !self
                    .metadata
                    .redirect_uris
                    .iter()
                    .any(|registered| compare_redirect_uri(registered, redirect_uri))
                {
                    return Err(OAuthError::InvalidRequest(
                        "invalid redirect_uri".to_string(),
                    ));
                }
                redirect_uri.to_string()
            }
            None => match self.metadata.redirect_uris.as_slice() {
                [single] => single.clone(),
                _ => {
                    return Err(OAuthError::InvalidRequest(
                        "redirect_uri is required".to_string(),
                    ))
                }
            },
        };
        // Public clients may not silently sign on; force consent otherwise.
        let prompt = match parameters.prompt.as_deref() {
            Some("none") if !self.metadata.is_confidential() => {
                return Err(OAuthError::InvalidRequest(
                    "public clients are not allowed to use silent sign-on".to_string(),
                ))
            }
            Some("create") => Some("create".to_string()),
            _ => Some("consent".to_string()),
        };
        Ok(AuthorizationRequestParameters {
            client_id: self.id.clone(),
            response_type: RESPONSE_TYPE_CODE.to_string(),
            redirect_uri,
            scope,
            state: parameters.state.clone(),
            code_challenge,
            code_challenge_method,
            login_hint: parameters
                .login_hint
                .as_deref()
                .map(|hint| hint.to_ascii_lowercase()),
            prompt,
            dpop_jkt: None,
        })
    }
}

/// The raw form parameters of a pushed authorization request.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ParRequest {
    pub client_id: String,
    #[serde(default)]
    pub response_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redirect_uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_challenge: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_challenge_method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub login_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

fn validate_requested_scope(
    requested: Option<&str>,
    metadata: &OAuthClientMetadata,
) -> Result<String, OAuthError> {
    let allowed: HashSet<&str> = metadata.allowed_scopes().into_iter().collect();
    let mut seen: Vec<&str> = Vec::new();
    for token in requested.unwrap_or_default().split_ascii_whitespace() {
        if token == "openid" {
            return Err(OAuthError::InvalidRequest(
                "openid scope is not supported".to_string(),
            ));
        }
        if !allowed.contains(token) {
            return Err(OAuthError::InvalidRequest(format!(
                "scope \"{token}\" is not registered in the client metadata"
            )));
        }
        if !seen.contains(&token) {
            seen.push(token);
        }
    }
    if !seen.contains(&SCOPE_ATPROTO) {
        return Err(OAuthError::InvalidRequest(format!(
            "the \"{SCOPE_ATPROTO}\" scope is required"
        )));
    }
    Ok(seen.join(" "))
}

/// RFC 8252 section 7.3: for loopback-IP redirect URIs registered without
/// a port, any port matches. Everything else is an exact string match.
pub fn compare_redirect_uri(registered: &str, requested: &str) -> bool {
    if registered == requested {
        return true;
    }
    let (Ok(registered), Ok(requested)) = (Url::parse(registered), Url::parse(requested)) else {
        return false;
    };
    let loopback_ip = matches!(registered.host_str(), Some("127.0.0.1") | Some("[::1]"));
    loopback_ip
        && registered.port().is_none()
        && registered.scheme() == requested.scheme()
        && registered.host_str() == requested.host_str()
        && registered.path() == requested.path()
        && registered.query() == requested.query()
}

/// Resolves `client_id` values into validated [`Client`]s: loopback ids
/// get virtual metadata, https ids are fetched via the injected fetcher.
pub struct ClientManager {
    fetcher: Arc<dyn ClientMetadataFetcher>,
}

impl ClientManager {
    pub fn new(fetcher: Arc<dyn ClientMetadataFetcher>) -> Self {
        Self { fetcher }
    }

    pub async fn get_client(&self, client_id: &str) -> Result<Client, OAuthError> {
        let metadata = if client_id.starts_with(LOOPBACK_CLIENT_ID_ORIGIN) {
            loopback_client_metadata(client_id)?
        } else {
            let metadata = self.fetcher.fetch_client_metadata(client_id).await?;
            validate_discoverable_client_id(client_id)?;
            metadata
        };
        validate_client_metadata(client_id, &metadata)?;
        let jwks = match &metadata.jwks_uri {
            Some(jwks_uri) => Some(self.fetcher.fetch_jwks(jwks_uri).await?),
            None => metadata.jwks.clone(),
        };
        Ok(Client {
            id: client_id.to_string(),
            metadata,
            jwks,
        })
    }
}

/// Derives virtual client metadata for a development loopback client id
/// (`http://localhost` with optional `scope` and `redirect_uri` query
/// parameters).
pub fn loopback_client_metadata(client_id: &str) -> Result<OAuthClientMetadata, OAuthError> {
    let rest = &client_id[LOOPBACK_CLIENT_ID_ORIGIN.len()..];
    if rest.contains('#') {
        return Err(OAuthError::InvalidClient(
            "loopback client_id must not contain a fragment".to_string(),
        ));
    }
    let (path, query) = match rest.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (rest, None),
    };
    if !path.is_empty() && path != "/" {
        return Err(OAuthError::InvalidClient(
            "loopback client_id must not contain a path component".to_string(),
        ));
    }
    let mut scope: Option<String> = None;
    let mut redirect_uris: Vec<String> = Vec::new();
    if let Some(query) = query {
        for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
            match key.as_ref() {
                "scope" => {
                    if scope.replace(value.into_owned()).is_some() {
                        return Err(OAuthError::InvalidClient(
                            "duplicate \"scope\" query parameter".to_string(),
                        ));
                    }
                }
                "redirect_uri" => redirect_uris.push(value.into_owned()),
                other => {
                    return Err(OAuthError::InvalidClient(format!(
                        "unexpected query parameter \"{other}\""
                    )))
                }
            }
        }
    }
    if redirect_uris.is_empty() {
        redirect_uris = DEFAULT_LOOPBACK_REDIRECT_URIS
            .iter()
            .map(|uri| uri.to_string())
            .collect();
    }
    let mut metadata = OAuthClientMetadata::new(client_id);
    metadata.scope = Some(scope.unwrap_or_else(|| SCOPE_ATPROTO.to_string()));
    metadata.redirect_uris = redirect_uris;
    metadata.grant_types = vec![
        GRANT_AUTHORIZATION_CODE.to_string(),
        GRANT_REFRESH_TOKEN.to_string(),
    ];
    metadata.token_endpoint_auth_method = Some(AUTH_METHOD_NONE.to_string());
    metadata.application_type = APPLICATION_TYPE_NATIVE.to_string();
    metadata.dpop_bound_access_tokens = true;
    Ok(metadata)
}

fn is_hostname_ip(url: &Url) -> bool {
    matches!(url.host(), Some(Host::Ipv4(_)) | Some(Host::Ipv6(_)))
}

fn is_local_hostname(host: &str) -> bool {
    let parts: Vec<&str> = host.split('.').collect();
    parts.len() < 2
        || matches!(
            *parts.last().expect("split always yields one part"),
            "test" | "local" | "localhost" | "invalid" | "example"
        )
}

fn is_private_use_scheme(url: &Url) -> bool {
    url.scheme().contains('.')
}

fn reverse_domain(domain: &str) -> String {
    domain.split('.').rev().collect::<Vec<&str>>().join(".")
}

pub fn validate_discoverable_client_id(client_id: &str) -> Result<Url, OAuthError> {
    let invalid = |reason: &str| OAuthError::InvalidClient(format!("invalid client_id: {reason}"));
    let url = Url::parse(client_id).map_err(|_| invalid("not a valid URL"))?;
    if url.scheme() != "https" {
        return Err(invalid("must use the https scheme"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(invalid("must not contain credentials"));
    }
    if url.fragment().is_some() {
        return Err(invalid("must not contain a fragment"));
    }
    if url.path() == "/" {
        return Err(invalid("must contain a path component"));
    }
    if url.path().ends_with('/') {
        return Err(invalid("must not end with a trailing slash"));
    }
    if is_hostname_ip(&url) {
        return Err(invalid("hostname must not be an IP address"));
    }
    let host = url.host_str().unwrap_or_default();
    if is_local_hostname(host) {
        return Err(invalid("hostname must not be a local hostname"));
    }
    if url.as_str() != client_id {
        return Err(invalid("must be in canonical form"));
    }
    Ok(url)
}

/// Applies the full upstream client metadata validation rules.
pub fn validate_client_metadata(
    client_id: &str,
    metadata: &OAuthClientMetadata,
) -> Result<(), OAuthError> {
    let invalid = |reason: String| OAuthError::InvalidClient(reason);
    if metadata.client_id != client_id {
        return Err(invalid("client_id does not match".to_string()));
    }
    if metadata.jwks.is_some() && metadata.jwks_uri.is_some() {
        return Err(invalid(
            "jwks and jwks_uri are mutually exclusive".to_string(),
        ));
    }
    let scopes = metadata.allowed_scopes();
    if scopes.is_empty() {
        return Err(invalid("missing scope property".to_string()));
    }
    let mut seen_scopes: HashSet<&str> = HashSet::new();
    for scope in &scopes {
        if !seen_scopes.insert(scope) {
            return Err(invalid(format!("duplicate scope \"{scope}\"")));
        }
    }
    if !seen_scopes.contains(SCOPE_ATPROTO) {
        return Err(invalid(format!("missing \"{SCOPE_ATPROTO}\" scope")));
    }
    let mut seen_grants: HashSet<&str> = HashSet::new();
    for grant in &metadata.grant_types {
        if !seen_grants.insert(grant) {
            return Err(invalid(format!("duplicate grant type \"{grant}\"")));
        }
        match grant.as_str() {
            GRANT_AUTHORIZATION_CODE | GRANT_REFRESH_TOKEN => {}
            "implicit" => {
                return Err(invalid(
                    "grant type \"implicit\" is not allowed".to_string(),
                ))
            }
            other => return Err(invalid(format!("grant type \"{other}\" is not supported"))),
        }
    }
    match metadata.auth_method() {
        AUTH_METHOD_NONE => {
            if metadata.token_endpoint_auth_signing_alg.is_some() {
                return Err(invalid(
                    "token_endpoint_auth_signing_alg is not allowed with method \"none\""
                        .to_string(),
                ));
            }
        }
        AUTH_METHOD_PRIVATE_KEY_JWT => {
            match (&metadata.jwks, &metadata.jwks_uri) {
                (Some(jwks), _) if jwks.keys.is_empty() => {
                    return Err(invalid("jwks must contain at least one key".to_string()))
                }
                (None, None) => {
                    return Err(invalid(
                        "private_key_jwt requires jwks or jwks_uri".to_string(),
                    ))
                }
                _ => {}
            }
            if metadata.token_endpoint_auth_signing_alg.is_none() {
                return Err(invalid(
                    "missing token_endpoint_auth_signing_alg".to_string(),
                ));
            }
        }
        other => {
            return Err(invalid(format!(
                "token_endpoint_auth_method \"{other}\" is not supported"
            )))
        }
    }
    if !metadata.dpop_bound_access_tokens {
        return Err(invalid(
            "\"dpop_bound_access_tokens\" must be true".to_string(),
        ));
    }
    if !metadata
        .response_types
        .iter()
        .any(|rt| rt == RESPONSE_TYPE_CODE)
    {
        return Err(invalid("response_types must include \"code\"".to_string()));
    }
    if !metadata
        .grant_types
        .iter()
        .any(|grant| grant == GRANT_AUTHORIZATION_CODE)
    {
        return Err(invalid(format!(
            "grant_types must include \"{GRANT_AUTHORIZATION_CODE}\""
        )));
    }
    if metadata.redirect_uris.is_empty() {
        return Err(invalid("at least one redirect_uri is required".to_string()));
    }
    let native = metadata.application_type == APPLICATION_TYPE_NATIVE;
    if native && metadata.auth_method() != AUTH_METHOD_NONE {
        return Err(invalid(
            "native clients must authenticate using the \"none\" method".to_string(),
        ));
    }
    for redirect_uri in &metadata.redirect_uris {
        validate_redirect_uri(redirect_uri, native)?;
    }
    if client_id.starts_with(LOOPBACK_CLIENT_ID_ORIGIN) {
        if metadata.client_uri.is_some() {
            return Err(invalid(
                "client_uri is not allowed for loopback clients".to_string(),
            ));
        }
        if !native {
            return Err(invalid(
                "loopback clients must have application_type \"native\"".to_string(),
            ));
        }
    } else {
        let client_id_url = validate_discoverable_client_id(client_id)?;
        validate_discoverable_client_metadata(&client_id_url, metadata)?;
    }
    Ok(())
}

fn validate_redirect_uri(redirect_uri: &str, native: bool) -> Result<(), OAuthError> {
    let invalid = |reason: String| {
        OAuthError::InvalidClient(format!("invalid redirect_uri \"{redirect_uri}\": {reason}"))
    };
    let url = Url::parse(redirect_uri).map_err(|_| invalid("not a valid URL".to_string()))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(invalid("must not contain credentials".to_string()));
    }
    let host = url.host_str().unwrap_or_default();
    if host == "localhost" {
        return Err(invalid(
            "\"localhost\" is not allowed, use an explicit loopback IP".to_string(),
        ));
    }
    match url.scheme() {
        "http" if matches!(host, "127.0.0.1" | "[::1]") => {
            if !native {
                return Err(invalid(
                    "loopback redirect URIs are only allowed for native apps".to_string(),
                ));
            }
            Ok(())
        }
        "http" => Err(invalid(
            "only loopback redirect URIs may use the \"http\" scheme".to_string(),
        )),
        "https" => {
            if is_local_hostname(host) {
                return Err(invalid("must not use a local hostname".to_string()));
            }
            Ok(())
        }
        _ if is_private_use_scheme(&url) => {
            if !native {
                return Err(invalid(
                    "private-use schemes are only allowed for native apps".to_string(),
                ));
            }
            Ok(())
        }
        _ => Err(invalid("invalid scheme".to_string())),
    }
}

fn validate_discoverable_client_metadata(
    client_id_url: &Url,
    metadata: &OAuthClientMetadata,
) -> Result<(), OAuthError> {
    let client_host = client_id_url.host_str().unwrap_or_default();
    if let Some(client_uri) = &metadata.client_uri {
        let url = Url::parse(client_uri)
            .map_err(|_| OAuthError::InvalidClient("invalid client_uri".to_string()))?;
        if url.origin() != client_id_url.origin() {
            return Err(OAuthError::InvalidClient(
                "client_uri must have the same origin as the client_id".to_string(),
            ));
        }
        let mut parent = url.path().to_string();
        if !parent.ends_with('/') {
            parent.push('/');
        }
        if !client_id_url.path().starts_with(&parent) {
            return Err(OAuthError::InvalidClient(
                "client_uri must be a parent URL of the client_id".to_string(),
            ));
        }
    }
    let expected_scheme = reverse_domain(client_host);
    // redirect URIs were already parse-validated by validate_redirect_uri
    for url in metadata
        .redirect_uris
        .iter()
        .filter_map(|uri| Url::parse(uri).ok())
    {
        if is_private_use_scheme(&url) && url.scheme() != expected_scheme {
            return Err(OAuthError::InvalidClient(format!(
                "private-use redirect scheme must be \"{expected_scheme}:\""
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jwk::{EcCurve, Jwk};
    use crate::jwt::{JwtClaims, JwtHeader};
    use serde_json::json;

    const NOW: u64 = 1_700_000_000;
    const CLIENT_ID: &str = "https://app.example.com/oauth/client-metadata.json";
    const ISSUER: &str = "https://pds.example.com";

    fn base_metadata() -> OAuthClientMetadata {
        let mut metadata = OAuthClientMetadata::new(CLIENT_ID);
        metadata.redirect_uris = vec!["https://app.example.com/callback".to_string()];
        metadata.grant_types = vec![
            GRANT_AUTHORIZATION_CODE.to_string(),
            GRANT_REFRESH_TOKEN.to_string(),
        ];
        metadata.scope = Some("atproto transition:generic".to_string());
        metadata.dpop_bound_access_tokens = true;
        metadata
    }

    fn assert_invalid(metadata: &OAuthClientMetadata, fragment: &str) {
        let err = validate_client_metadata(&metadata.client_id, metadata).unwrap_err();
        let desc = err.error_description().to_string();
        assert!(desc.contains(fragment), "expected {fragment:?} in {desc:?}");
    }

    fn assert_desc(err: &OAuthError, fragment: &str) {
        let desc = err.error_description().to_string();
        assert!(desc.contains(fragment), "expected {fragment:?} in {desc:?}");
    }

    #[test]
    fn valid_public_web_client() {
        validate_client_metadata(CLIENT_ID, &base_metadata()).unwrap();
    }

    #[test]
    fn valid_confidential_client() {
        let mut metadata = base_metadata();
        metadata.token_endpoint_auth_method = Some(AUTH_METHOD_PRIVATE_KEY_JWT.to_string());
        metadata.token_endpoint_auth_signing_alg = Some("ES256".to_string());
        metadata.jwks = Some(JwkSet {
            keys: vec![Jwk::from_private_key_bytes(EcCurve::P256, &[0x42u8; 32])
                .unwrap()
                .to_public()],
        });
        validate_client_metadata(CLIENT_ID, &metadata).unwrap();
        metadata.jwks = None;
        metadata.jwks_uri = Some("https://app.example.com/jwks.json".to_string());
        validate_client_metadata(CLIENT_ID, &metadata).unwrap();
    }

    #[test]
    fn generic_metadata_failures() {
        let err =
            validate_client_metadata("https://other.example.com/x", &base_metadata()).unwrap_err();
        assert!(err.error_description().contains("does not match"));

        let mut metadata = base_metadata();
        metadata.jwks = Some(JwkSet { keys: vec![] });
        metadata.jwks_uri = Some("https://app.example.com/jwks.json".to_string());
        assert_invalid(&metadata, "mutually exclusive");

        let mut metadata = base_metadata();
        metadata.scope = None;
        assert_invalid(&metadata, "missing scope");

        let mut metadata = base_metadata();
        metadata.scope = Some("atproto atproto".to_string());
        assert_invalid(&metadata, "duplicate scope");

        let mut metadata = base_metadata();
        metadata.scope = Some("transition:generic".to_string());
        assert_invalid(&metadata, "missing \"atproto\" scope");

        let mut metadata = base_metadata();
        metadata.grant_types = vec![
            GRANT_AUTHORIZATION_CODE.to_string(),
            GRANT_AUTHORIZATION_CODE.to_string(),
        ];
        assert_invalid(&metadata, "duplicate grant type");

        let mut metadata = base_metadata();
        metadata.grant_types = vec![GRANT_AUTHORIZATION_CODE.to_string(), "implicit".to_string()];
        assert_invalid(&metadata, "not allowed");

        let mut metadata = base_metadata();
        metadata.grant_types = vec![
            GRANT_AUTHORIZATION_CODE.to_string(),
            "client_credentials".to_string(),
        ];
        assert_invalid(&metadata, "not supported");

        let mut metadata = base_metadata();
        metadata.grant_types = vec![GRANT_REFRESH_TOKEN.to_string()];
        assert_invalid(&metadata, "must include \"authorization_code\"");
    }

    #[test]
    fn auth_method_failures() {
        let mut metadata = base_metadata();
        metadata.token_endpoint_auth_signing_alg = Some("ES256".to_string());
        assert_invalid(&metadata, "not allowed with method \"none\"");

        let mut metadata = base_metadata();
        metadata.token_endpoint_auth_method = Some(AUTH_METHOD_PRIVATE_KEY_JWT.to_string());
        metadata.token_endpoint_auth_signing_alg = Some("ES256".to_string());
        assert_invalid(&metadata, "requires jwks or jwks_uri");

        metadata.jwks = Some(JwkSet { keys: vec![] });
        assert_invalid(&metadata, "at least one key");

        metadata.jwks = Some(JwkSet {
            keys: vec![Jwk::from_private_key_bytes(EcCurve::P256, &[0x42u8; 32])
                .unwrap()
                .to_public()],
        });
        metadata.token_endpoint_auth_signing_alg = None;
        assert_invalid(&metadata, "missing token_endpoint_auth_signing_alg");

        let mut metadata = base_metadata();
        metadata.token_endpoint_auth_method = Some("client_secret_basic".to_string());
        assert_invalid(&metadata, "is not supported");
    }

    #[test]
    fn structural_failures() {
        let mut metadata = base_metadata();
        metadata.dpop_bound_access_tokens = false;
        assert_invalid(&metadata, "dpop_bound_access_tokens");

        let mut metadata = base_metadata();
        metadata.response_types = vec!["token".to_string()];
        assert_invalid(&metadata, "response_types must include");

        let mut metadata = base_metadata();
        metadata.redirect_uris = vec![];
        assert_invalid(&metadata, "at least one redirect_uri");

        let mut metadata = base_metadata();
        metadata.application_type = APPLICATION_TYPE_NATIVE.to_string();
        metadata.token_endpoint_auth_method = Some(AUTH_METHOD_PRIVATE_KEY_JWT.to_string());
        metadata.token_endpoint_auth_signing_alg = Some("ES256".to_string());
        metadata.jwks_uri = Some("https://app.example.com/jwks.json".to_string());
        assert_invalid(&metadata, "native clients must authenticate");
    }

    #[test]
    fn redirect_uri_rules() {
        let cases: [(&str, bool, &str); 10] = [
            ("not a url", false, "not a valid URL"),
            ("https://user:pw@app.example.com/cb", false, "credentials"),
            ("http://localhost/cb", false, "explicit loopback IP"),
            ("http://127.0.0.1:8080/cb", false, "only allowed for native"),
            ("http://evil.example.com/cb", false, "http\" scheme"),
            ("https://app.test/cb", false, "local hostname"),
            ("com.example.app:/cb", false, "only allowed for native"),
            ("ftp://app.example.com/cb", false, "invalid scheme"),
            ("https://app.example.com/cb", true, ""),
            ("mailto:foo", false, "invalid scheme"),
        ];
        for (uri, ok, fragment) in cases {
            let result = validate_redirect_uri(uri, false);
            if ok {
                result.unwrap();
            } else {
                let err = result.unwrap_err();
                assert_desc(&err, fragment);
            }
        }
        validate_redirect_uri("http://127.0.0.1:8080/cb", true).unwrap();
        validate_redirect_uri("http://[::1]/cb", true).unwrap();
        validate_redirect_uri("com.example.app:/cb", true).unwrap();
    }

    #[test]
    fn discoverable_client_id_rules() {
        validate_discoverable_client_id(CLIENT_ID).unwrap();
        let cases: [(&str, &str); 9] = [
            ("not a url", "not a valid URL"),
            ("http://app.example.com/client", "https scheme"),
            ("https://user@app.example.com/client", "credentials"),
            ("https://app.example.com/client#frag", "fragment"),
            ("https://app.example.com/", "must contain a path"),
            ("https://app.example.com/client/", "trailing slash"),
            ("https://127.0.0.1/client", "IP address"),
            ("https://app.test/client", "local hostname"),
            ("https://app.example.com/a/../client", "canonical form"),
        ];
        for (client_id, fragment) in cases {
            let err = validate_discoverable_client_id(client_id).unwrap_err();
            assert_desc(&err, fragment);
        }
    }

    #[test]
    fn discoverable_metadata_rules() {
        let mut metadata = base_metadata();
        metadata.client_uri = Some("https://app.example.com/oauth".to_string());
        validate_client_metadata(CLIENT_ID, &metadata).unwrap();

        metadata.client_uri = Some("https://other.example.com".to_string());
        assert_invalid(&metadata, "same origin");

        metadata.client_uri = Some("https://app.example.com/elsewhere".to_string());
        assert_invalid(&metadata, "parent URL");

        metadata.client_uri = Some("::invalid::".to_string());
        assert_invalid(&metadata, "invalid client_uri");

        let mut metadata = base_metadata();
        metadata.application_type = APPLICATION_TYPE_NATIVE.to_string();
        metadata.redirect_uris = vec!["app.wrong.scheme:/cb".to_string()];
        assert_invalid(&metadata, "com.example.app");

        metadata.redirect_uris = vec!["com.example.app:/cb".to_string()];
        validate_client_metadata(CLIENT_ID, &metadata).unwrap();
    }

    #[test]
    fn loopback_metadata_derivation() {
        let metadata = loopback_client_metadata("http://localhost").unwrap();
        assert_eq!(metadata.scope.as_deref(), Some(SCOPE_ATPROTO));
        assert_eq!(metadata.redirect_uris, DEFAULT_LOOPBACK_REDIRECT_URIS);
        assert_eq!(metadata.application_type, APPLICATION_TYPE_NATIVE);
        assert_eq!(metadata.auth_method(), AUTH_METHOD_NONE);
        assert!(metadata.dpop_bound_access_tokens);
        validate_client_metadata("http://localhost", &metadata).unwrap();

        let metadata = loopback_client_metadata(
            "http://localhost/?scope=atproto+transition%3Ageneric&redirect_uri=http%3A%2F%2F127.0.0.1%3A8080%2Fcb",
        )
        .unwrap();
        assert_eq!(
            metadata.scope.as_deref(),
            Some("atproto transition:generic")
        );
        assert_eq!(metadata.redirect_uris, vec!["http://127.0.0.1:8080/cb"]);

        for (client_id, fragment) in [
            ("http://localhost#frag", "fragment"),
            ("http://localhost/path", "path component"),
            ("http://localhost?scope=a&scope=b", "duplicate \"scope\""),
            ("http://localhost?other=x", "unexpected query parameter"),
        ] {
            let err = loopback_client_metadata(client_id).unwrap_err();
            assert_desc(&err, fragment);
        }
    }

    #[test]
    fn loopback_specific_validation() {
        let mut metadata = loopback_client_metadata("http://localhost").unwrap();
        metadata.client_uri = Some("https://app.example.com".to_string());
        assert_invalid(&metadata, "client_uri is not allowed");

        let mut metadata = loopback_client_metadata("http://localhost").unwrap();
        metadata.application_type = APPLICATION_TYPE_WEB.to_string();
        metadata.redirect_uris = vec!["https://app.example.com/cb".to_string()];
        assert_invalid(&metadata, "application_type \"native\"");
    }

    #[test]
    fn redirect_uri_comparison() {
        assert!(compare_redirect_uri(
            "https://app.example.com/cb",
            "https://app.example.com/cb"
        ));
        assert!(!compare_redirect_uri(
            "https://app.example.com/cb",
            "https://app.example.com/other"
        ));
        // registered loopback without port matches any port
        assert!(compare_redirect_uri(
            "http://127.0.0.1/cb",
            "http://127.0.0.1:49152/cb"
        ));
        assert!(compare_redirect_uri("http://[::1]/", "http://[::1]:8080/"));
        // registered with explicit port requires exact match
        assert!(!compare_redirect_uri(
            "http://127.0.0.1:8080/cb",
            "http://127.0.0.1:9090/cb"
        ));
        // non-loopback hosts never get the port wildcard
        assert!(!compare_redirect_uri(
            "https://app.example.com/cb",
            "https://app.example.com:8443/cb"
        ));
        assert!(!compare_redirect_uri("not-a-url", "not-a-url-either"));
        assert!(!compare_redirect_uri(
            "http://127.0.0.1/cb",
            "http://127.0.0.1:8080/cb?extra=1"
        ));
    }

    // client authentication

    fn client_key() -> Jwk {
        let mut key = Jwk::from_private_key_bytes(EcCurve::P256, &[0x42u8; 32]).unwrap();
        key.kid = Some("key-1".to_string());
        key
    }

    fn confidential_client() -> Client {
        let mut metadata = base_metadata();
        metadata.token_endpoint_auth_method = Some(AUTH_METHOD_PRIVATE_KEY_JWT.to_string());
        metadata.token_endpoint_auth_signing_alg = Some("ES256".to_string());
        Client {
            id: CLIENT_ID.to_string(),
            metadata,
            jwks: Some(JwkSet {
                keys: vec![client_key().to_public()],
            }),
        }
    }

    fn assertion(key: &Jwk, mutate: impl FnOnce(&mut JwtClaims)) -> String {
        let mut header = JwtHeader::new("ES256");
        header.kid = key.kid.clone();
        let mut claims = JwtClaims {
            iss: Some(CLIENT_ID.to_string()),
            sub: Some(CLIENT_ID.to_string()),
            aud: Some(json!(ISSUER)),
            iat: Some(NOW),
            exp: Some(NOW + 60),
            jti: Some("assert-1".to_string()),
            ..Default::default()
        };
        mutate(&mut claims);
        crate::jwt::sign(&header, &claims, key).unwrap()
    }

    #[test]
    fn public_client_authenticates_as_none() {
        let client = Client {
            id: CLIENT_ID.to_string(),
            metadata: base_metadata(),
            jwks: None,
        };
        assert_eq!(
            client.authenticate(None, None, ISSUER, NOW).unwrap(),
            ClientAuth::None
        );
    }

    #[test]
    fn confidential_client_authenticates() {
        let client = confidential_client();
        let auth = client
            .authenticate(
                Some(CLIENT_ASSERTION_TYPE_JWT_BEARER),
                Some(&assertion(&client_key(), |_| {})),
                ISSUER,
                NOW,
            )
            .unwrap();
        assert_eq!(
            auth,
            ClientAuth::PrivateKeyJwt {
                alg: "ES256".to_string(),
                kid: "key-1".to_string(),
                jkt: client_key().thumbprint(),
            }
        );
    }

    #[test]
    fn confidential_client_failures() {
        let client = confidential_client();
        let ok = assertion(&client_key(), |_| {});
        let check = |assertion_type: Option<&str>, assertion: Option<&str>, fragment: &str| {
            let err = client
                .authenticate(assertion_type, assertion, ISSUER, NOW)
                .unwrap_err();
            assert_desc(&err, fragment);
        };
        check(None, Some(&ok), "client_assertion_type");
        check(Some("urn:wrong"), Some(&ok), "client_assertion_type");
        check(
            Some(CLIENT_ASSERTION_TYPE_JWT_BEARER),
            None,
            "client_assertion is required",
        );
        check(
            Some(CLIENT_ASSERTION_TYPE_JWT_BEARER),
            Some("garbage"),
            "invalid client assertion",
        );

        // missing kid
        let mut header = JwtHeader::new("ES256");
        header.kid = None;
        let no_kid = crate::jwt::sign(&header, &JwtClaims::default(), &client_key()).unwrap();
        check(
            Some(CLIENT_ASSERTION_TYPE_JWT_BEARER),
            Some(&no_kid),
            "missing \"kid\"",
        );

        // unknown kid
        let mut other_key = client_key();
        other_key.kid = Some("key-2".to_string());
        let mut header = JwtHeader::new("ES256");
        header.kid = Some("key-2".to_string());
        let unknown_kid = crate::jwt::sign(&header, &JwtClaims::default(), &other_key).unwrap();
        check(
            Some(CLIENT_ASSERTION_TYPE_JWT_BEARER),
            Some(&unknown_kid),
            "no key found",
        );

        // signed by a different key under the registered kid
        let mut wrong_key = Jwk::from_private_key_bytes(EcCurve::P256, &[0x43u8; 32]).unwrap();
        wrong_key.kid = Some("key-1".to_string());
        check(
            Some(CLIENT_ASSERTION_TYPE_JWT_BEARER),
            Some(&assertion(&wrong_key, |_| {})),
            "invalid client assertion",
        );

        type ClaimMutation = Box<dyn FnOnce(&mut JwtClaims)>;
        let claim_cases: [(ClaimMutation, &str); 7] = [
            (
                Box::new(|claims: &mut JwtClaims| claims.iss = Some("https://evil".to_string())),
                "invalid client assertion",
            ),
            (
                Box::new(|claims: &mut JwtClaims| {
                    claims.aud = Some(json!("https://other-as.example.com"))
                }),
                "invalid client assertion",
            ),
            (
                Box::new(|claims: &mut JwtClaims| claims.sub = Some("someone-else".to_string())),
                "\"sub\" must be the client_id",
            ),
            (
                Box::new(|claims: &mut JwtClaims| claims.jti = None),
                "missing \"jti\"",
            ),
            (
                Box::new(|claims: &mut JwtClaims| claims.iat = None),
                "missing \"iat\"",
            ),
            (
                Box::new(|claims: &mut JwtClaims| claims.iat = Some(NOW - 61)),
                "expired",
            ),
            (
                Box::new(|claims: &mut JwtClaims| {
                    claims.iat = Some(NOW + 10);
                    claims.exp = Some(NOW + 100);
                }),
                "expired",
            ),
        ];
        for (mutate, fragment) in claim_cases {
            check(
                Some(CLIENT_ASSERTION_TYPE_JWT_BEARER),
                Some(&assertion(&client_key(), mutate)),
                fragment,
            );
        }
    }

    // request validation

    fn par_request() -> ParRequest {
        ParRequest {
            client_id: CLIENT_ID.to_string(),
            response_type: RESPONSE_TYPE_CODE.to_string(),
            redirect_uri: Some("https://app.example.com/callback".to_string()),
            scope: Some("atproto transition:generic".to_string()),
            state: Some("state-1".to_string()),
            code_challenge: Some("E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM".to_string()),
            code_challenge_method: Some(CODE_CHALLENGE_METHOD_S256.to_string()),
            login_hint: Some("Alice.Example.Com".to_string()),
            prompt: None,
        }
    }

    fn public_client() -> Client {
        Client {
            id: CLIENT_ID.to_string(),
            metadata: base_metadata(),
            jwks: None,
        }
    }

    #[test]
    fn validate_request_success() {
        let client = public_client();
        let parameters = client.validate_request(&par_request()).unwrap();
        assert_eq!(parameters.scope, "atproto transition:generic");
        assert_eq!(parameters.redirect_uri, "https://app.example.com/callback");
        assert_eq!(parameters.login_hint.as_deref(), Some("alice.example.com"));
        assert_eq!(parameters.prompt.as_deref(), Some("consent"));
        assert!(parameters.dpop_jkt.is_none());
    }

    #[test]
    fn validate_request_defaults_single_redirect_uri() {
        let client = public_client();
        let mut request = par_request();
        request.redirect_uri = None;
        let parameters = client.validate_request(&request).unwrap();
        assert_eq!(parameters.redirect_uri, "https://app.example.com/callback");

        let mut multi = public_client();
        multi
            .metadata
            .redirect_uris
            .push("https://app.example.com/callback2".to_string());
        let err = multi.validate_request(&request).unwrap_err();
        assert!(err.error_description().contains("redirect_uri is required"));
    }

    #[test]
    fn validate_request_failures() {
        let client = public_client();
        let check = |mutate: Box<dyn FnOnce(&mut ParRequest)>, fragment: &str| {
            let mut request = par_request();
            mutate(&mut request);
            let err = client.validate_request(&request).unwrap_err();
            assert_desc(&err, fragment);
        };
        check(
            Box::new(|request| request.client_id = "https://evil".to_string()),
            "client_id does not match",
        );
        check(
            Box::new(|request| request.response_type = "token".to_string()),
            "unsupported response_type",
        );
        check(
            Box::new(|request| request.scope = Some("atproto openid".to_string())),
            "openid",
        );
        check(
            Box::new(|request| request.scope = Some("atproto transition:email".to_string())),
            "not registered",
        );
        check(
            Box::new(|request| request.scope = Some("transition:generic".to_string())),
            "\"atproto\" scope is required",
        );
        check(Box::new(|request| request.scope = None), "atproto");
        check(Box::new(|request| request.code_challenge = None), "PKCE");
        check(
            Box::new(|request| {
                request.code_challenge_method = Some("plain".to_string());
            }),
            "unsupported code_challenge_method",
        );
        check(
            Box::new(|request| request.code_challenge_method = None),
            "code_challenge_method is required",
        );
        check(
            Box::new(|request| {
                request.redirect_uri = Some("https://evil.example.com/cb".to_string())
            }),
            "invalid redirect_uri",
        );
        check(
            Box::new(|request| request.prompt = Some("none".to_string())),
            "silent sign-on",
        );
    }

    #[test]
    fn validate_request_prompt_handling() {
        let client = confidential_client();
        let mut request = par_request();
        request.prompt = Some("none".to_string());
        // confidential clients may request silent sign-on; we still record
        // consent as the effective prompt
        let parameters = client.validate_request(&request).unwrap();
        assert_eq!(parameters.prompt.as_deref(), Some("consent"));

        request.prompt = Some("create".to_string());
        let parameters = client.validate_request(&request).unwrap();
        assert_eq!(parameters.prompt.as_deref(), Some("create"));
    }

    #[test]
    fn validate_request_requires_code_response_type_in_metadata() {
        let mut client = public_client();
        client.metadata.response_types = vec!["token".to_string()];
        let err = client.validate_request(&par_request()).unwrap_err();
        assert!(err
            .error_description()
            .contains("does not declare the \"code\" response type"));
    }

    // client manager

    struct StubFetcher {
        metadata: OAuthClientMetadata,
        jwks: Option<JwkSet>,
    }

    #[async_trait::async_trait]
    impl ClientMetadataFetcher for StubFetcher {
        async fn fetch_client_metadata(
            &self,
            url: &str,
        ) -> Result<OAuthClientMetadata, OAuthError> {
            if url == self.metadata.client_id {
                Ok(self.metadata.clone())
            } else {
                Err(OAuthError::InvalidClient("fetch failed".to_string()))
            }
        }

        async fn fetch_jwks(&self, _url: &str) -> Result<JwkSet, OAuthError> {
            self.jwks
                .clone()
                .ok_or_else(|| OAuthError::InvalidClient("jwks fetch failed".to_string()))
        }
    }

    #[tokio::test]
    async fn manager_resolves_discoverable_client() {
        let manager = ClientManager::new(Arc::new(StubFetcher {
            metadata: base_metadata(),
            jwks: None,
        }));
        let client = manager.get_client(CLIENT_ID).await.unwrap();
        assert_eq!(client.id, CLIENT_ID);
        assert!(client.jwks.is_none());
        assert!(manager
            .get_client("https://unknown.example.com/x")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn manager_resolves_loopback_client() {
        let manager = ClientManager::new(Arc::new(StubFetcher {
            metadata: base_metadata(),
            jwks: None,
        }));
        let client = manager.get_client("http://localhost").await.unwrap();
        assert_eq!(client.metadata.application_type, APPLICATION_TYPE_NATIVE);
    }

    #[tokio::test]
    async fn manager_fetches_jwks_uri() {
        let mut metadata = base_metadata();
        metadata.token_endpoint_auth_method = Some(AUTH_METHOD_PRIVATE_KEY_JWT.to_string());
        metadata.token_endpoint_auth_signing_alg = Some("ES256".to_string());
        metadata.jwks_uri = Some("https://app.example.com/jwks.json".to_string());
        let jwks = JwkSet {
            keys: vec![client_key().to_public()],
        };
        let manager = ClientManager::new(Arc::new(StubFetcher {
            metadata: metadata.clone(),
            jwks: Some(jwks.clone()),
        }));
        let client = manager.get_client(CLIENT_ID).await.unwrap();
        assert_eq!(client.jwks, Some(jwks));

        let manager = ClientManager::new(Arc::new(StubFetcher {
            metadata,
            jwks: None,
        }));
        assert!(manager.get_client(CLIENT_ID).await.is_err());
    }

    #[tokio::test]
    async fn manager_uses_inline_jwks() {
        let mut metadata = base_metadata();
        metadata.token_endpoint_auth_method = Some(AUTH_METHOD_PRIVATE_KEY_JWT.to_string());
        metadata.token_endpoint_auth_signing_alg = Some("ES256".to_string());
        metadata.jwks = Some(JwkSet {
            keys: vec![client_key().to_public()],
        });
        let manager = ClientManager::new(Arc::new(StubFetcher {
            metadata: metadata.clone(),
            jwks: None,
        }));
        let client = manager.get_client(CLIENT_ID).await.unwrap();
        assert_eq!(client.jwks, metadata.jwks);
    }
}
