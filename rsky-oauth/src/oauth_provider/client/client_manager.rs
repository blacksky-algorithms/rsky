use crate::cached_getter::{CachedGetter, GetCachedOptions, Getter};
use crate::jwk::Keyset;
use crate::oauth_provider::client::client::Client;
use crate::oauth_provider::client::client_info::ClientInfo;
use crate::oauth_provider::client::client_store::ClientStore;
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_provider::oauth_hooks::OAuthHooks;
use crate::oauth_types::{
    ApplicationType, OAuthAuthorizationServerMetadata, OAuthClientId, OAuthClientIdDiscoverable,
    OAuthClientIdLoopback, OAuthClientMetadata, OAuthEndpointAuthMethod, OAuthGrantType,
    OAuthRedirectUri, OAuthResponseType, SubjectType,
};
use crate::simple_store_memory::SimpleStoreMemory;
use biscuit::jwk::JWKSet;
use biscuit::Empty;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::RwLock;
use url::Url;

pub type LoopbackMetadataGetter =
    Box<dyn Fn(OAuthClientIdLoopback) -> OAuthClientMetadata + Send + Sync>;

pub struct ClientManager {
    jwks: CachedGetter<String, JWKSet<Empty>>,
    server_metadata: OAuthAuthorizationServerMetadata,
    keyset: Arc<RwLock<Keyset>>,
    store: Arc<RwLock<dyn ClientStore>>,
    metadata_getter: CachedGetter<String, OAuthClientMetadata>,
    loopback_metadata: Option<LoopbackMetadataGetter>,
    hooks: Arc<OAuthHooks>,
}

pub struct JwkGetter {}

impl Getter<String, JWKSet<Empty>> for JwkGetter {
    fn get<'a>(
        &'a self,
        key: String,
        options: Option<GetCachedOptions>,
        stored_value: Option<JWKSet<Empty>>,
    ) -> Pin<Box<dyn Future<Output = JWKSet<Empty>> + Send + Sync + 'a>> {
        Box::pin(async move { fetch_jwks_handler(key, options).await })
    }
}

pub struct OAuthClientMetadataGetter {}

impl Getter<String, OAuthClientMetadata> for OAuthClientMetadataGetter {
    fn get<'a>(
        &'a self,
        key: String,
        options: Option<GetCachedOptions>,
        stored_value: Option<OAuthClientMetadata>,
    ) -> Pin<Box<dyn Future<Output = OAuthClientMetadata> + Send + Sync + 'a>> {
        Box::pin(async move { fetch_metadata_handler(key, options).await })
    }
}

impl ClientManager {
    pub fn new(
        server_metadata: OAuthAuthorizationServerMetadata,
        keyset: Arc<RwLock<Keyset>>,
        hooks: Arc<OAuthHooks>,
        store: Arc<RwLock<dyn ClientStore>>,
        loopback_metadata: Option<LoopbackMetadataGetter>,
        client_jwks_cache: Arc<RwLock<SimpleStoreMemory<String, JWKSet<Empty>>>>,
        client_metadata_cache: Arc<RwLock<SimpleStoreMemory<String, OAuthClientMetadata>>>,
    ) -> Self {
        let jwks: CachedGetter<String, JWKSet<Empty>> =
            CachedGetter::new(Arc::new(RwLock::new(JwkGetter {})), client_jwks_cache, None);
        let metadata_getter: CachedGetter<String, OAuthClientMetadata> = CachedGetter::new(
            Arc::new(RwLock::new(OAuthClientMetadataGetter {})),
            client_metadata_cache,
            None,
        );
        Self {
            jwks,
            server_metadata,
            keyset,
            store,
            metadata_getter,
            loopback_metadata,
            hooks,
        }
    }

    /**
     * @see {@link https://openid.net/specs/openid-connect-registration-1_0.html#rfc.section.2 OIDC Client Registration}
     */
    pub async fn get_client(&self, client_id: &OAuthClientId) -> Result<Client, OAuthError> {
        let metadata = self.get_client_metadata(client_id).await?;

        let jwks = match &metadata.jwks_uri {
            None => None,
            Some(jwks_uri) => Some(self.jwks.get(&jwks_uri.to_string(), None).await),
        };

        let partial_info = match &self.hooks.on_client_info {
            None => None,
            Some(on_client_info) => Some(on_client_info(client_id.clone(), metadata.clone(), None)),
        };
        let is_first_party;
        let is_trusted;
        match partial_info {
            None => {
                is_first_party = false;
                is_trusted = is_first_party;
            }
            Some(partial_info) => {
                is_first_party = partial_info.is_first_party;
                is_trusted = partial_info.is_trusted;
            }
        }
        let partial_info = ClientInfo {
            is_first_party,
            is_trusted,
        };

        Ok(Client::new(client_id.clone(), metadata, jwks, partial_info))
    }

    #[tracing::instrument(skip_all)]
    async fn get_client_metadata(
        &self,
        client_id: &OAuthClientId,
    ) -> Result<OAuthClientMetadata, OAuthError> {
        if let Ok(loopback_client_id) = OAuthClientIdLoopback::new(client_id.val()) {
            self.get_loopback_client_metadata(loopback_client_id).await
        } else if let Ok(discoverable_client_id) = OAuthClientIdDiscoverable::new(client_id.val()) {
            return self
                .get_discoverable_client_metadata(&discoverable_client_id)
                .await;
        } else {
            println!("stored client");
            return self.get_stored_client_metadata(client_id).await;
        }
    }

    #[tracing::instrument(skip_all)]
    async fn get_loopback_client_metadata(
        &self,
        client_id: OAuthClientIdLoopback,
    ) -> Result<OAuthClientMetadata, OAuthError> {
        if let Some(loopback_metadata) = &self.loopback_metadata {
            let metadata = loopback_metadata(client_id.clone());
            self.validate_client_metadata(client_id.as_str(), metadata)
                .await
        } else {
            Err(OAuthError::InvalidClientMetadataError(
                "Loopback clients are not allowed".to_string(),
            ))
        }
    }

    async fn get_discoverable_client_metadata(
        &self,
        client_id: &OAuthClientIdDiscoverable,
    ) -> Result<OAuthClientMetadata, OAuthError> {
        let metadata_url = client_id.as_url();

        let metadata = self
            .metadata_getter
            .get(&metadata_url.to_string(), None)
            .await;

        // Note: we do *not* re-validate the metadata here, as the metadata is
        // validated within the getter. This is to avoid double validation.
        //
        // return this.validateClientMetadata(metadataUrl.href, metadata)
        Ok(metadata)
    }

    async fn get_stored_client_metadata(
        &self,
        client_id: &OAuthClientId,
    ) -> Result<OAuthClientMetadata, OAuthError> {
        let store = self.store.read().await;
        let metadata = store.find_client(client_id.clone())?;
        self.validate_client_metadata(client_id.as_str(), metadata)
            .await
    }

    /**
     * This method will ensure that the client metadata is valid w.r.t. the OAuth
     * and OIDC specifications. It will also ensure that the metadata is
     * compatible with the implementation of this library, and ATPROTO's
     * requirements.
     */
    async fn validate_client_metadata(
        &self,
        client_id: &str,
        metadata: OAuthClientMetadata,
    ) -> Result<OAuthClientMetadata, OAuthError> {
        if metadata.jwks.is_some() && metadata.jwks_uri.is_some() {
            return Err(OAuthError::InvalidClientMetadataError(
                "jwks_uri and jwks are mutually exclusive".to_string(),
            ));
        }

        // Known OIDC specific parameters
        if metadata.default_max_age.is_some()
            || metadata.userinfo_signed_response_alg.is_some()
            || metadata.id_token_signed_response_alg.is_some()
            || metadata.userinfo_encrypted_response_alg.is_some()
        {
            return Err(OAuthError::InvalidClientMetadataError(
                "Unsupported client metadata parameter".to_string(),
            ));
        }

        let client_uri_url = metadata.client_uri.clone();
        let client_uri_domain = match &client_uri_url {
            None => None,
            Some(web_uri) => Some(web_uri.domain()),
        };

        if client_uri_url.is_some() && client_uri_domain.is_none() {
            return Err(OAuthError::InvalidClientMetadataError(
                "client_uri hostname is invalid".to_string(),
            ));
        }

        let oauth_scope = match &metadata.scope {
            None => {
                return Err(OAuthError::InvalidClientMetadataError(
                    "Missing scope property".to_string(),
                ))
            }
            Some(scope) => scope.clone(),
        };

        let mut scopes: Vec<String> = oauth_scope.iter().map(|x| x.to_string()).collect();

        if !scopes.contains(&"atproto".to_string()) {
            return Err(OAuthError::InvalidClientMetadataError(
                "Missing \"atproto\" scope".to_string(),
            ));
        }

        scopes.sort();
        let x = scopes.len();
        scopes.dedup();
        if x != scopes.len() {
            return Err(OAuthError::InvalidClientMetadataError(
                "Duplicate scope".to_string(),
            ));
        }

        for scope in scopes {
            // Note, once we have dynamic scopes, this check will need to be
            // updated to check against the server's supported scopes.
            if let Some(scopes_supported) = &self.server_metadata.scopes_supported {
                if !scopes_supported.contains(&scope) {
                    return Err(OAuthError::InvalidClientMetadataError(
                        "Unsupported scope".to_string(),
                    ));
                }
            }
        }

        let mut grant_types = metadata.grant_types.clone();
        grant_types.sort();
        let x = grant_types.len();
        grant_types.dedup();
        if x != grant_types.len() {
            return Err(OAuthError::InvalidClientMetadataError(
                "Duplicate grant type".to_string(),
            ));
        }

        for grant_type in grant_types {
            match grant_type {
                OAuthGrantType::AuthorizationCode => {
                    if let Some(grant_types_supported) = &self.server_metadata.grant_types_supported
                    {
                        if !grant_types_supported.contains(&grant_type) {
                            return Err(OAuthError::InvalidClientMetadataError(
                                "Unsupported grant type".to_string(),
                            ));
                        }
                    }
                }
                OAuthGrantType::Implicit => {
                    // Never allowed (unsafe)
                    return Err(OAuthError::InvalidClientMetadataError(
                        "Grant type is not allowed".to_string(),
                    ));
                }
                OAuthGrantType::RefreshToken => {
                    if let Some(grant_types_supported) = &self.server_metadata.grant_types_supported
                    {
                        if !grant_types_supported.contains(&grant_type) {
                            return Err(OAuthError::InvalidClientMetadataError(
                                "Unsupported grant type".to_string(),
                            ));
                        }
                    }
                }
                _ => {
                    return Err(OAuthError::InvalidClientMetadataError(
                        "Grant type is not supported".to_string(),
                    ))
                }
            }
        }

        if let Some(metadata_client_id) = metadata.client_id.clone() {
            if metadata_client_id != client_id {
                return Err(OAuthError::InvalidClientMetadataError(
                    "client_id does not match".to_string(),
                ));
            }
        }

        if let Some(metadata_subject_type) = &metadata.subject_type {
            match metadata_subject_type {
                SubjectType::Public => {}
                SubjectType::Pairwise => {
                    return Err(OAuthError::InvalidClientMetadataError(
                        "Only public subject_type is supported".to_string(),
                    ))
                }
            }
        }

        let method = match &metadata.token_endpoint_auth_method {
            None => {
                return Err(OAuthError::InvalidClientMetadataError(
                    "Missing token_endpoint_auth_method client metadata".to_string(),
                ))
            }
            Some(method) => method.clone(),
        };
        match method {
            OAuthEndpointAuthMethod::None => {
                if metadata.token_endpoint_auth_signing_alg.is_some() {
                    return Err(OAuthError::InvalidClientMetadataError("token_endpoint_auth_method \"none\" must not have token_endpoint_auth_signing_alg".to_string()));
                }
            }
            OAuthEndpointAuthMethod::PrivateKeyJwt => {
                if metadata.jwks.is_none() && metadata.jwks_uri.is_none() {
                    return Err(OAuthError::InvalidClientMetadataError(
                        "private_key_jwt auth method requires jwks or jwks_uri".to_string(),
                    ));
                }
                if let Some(jwks) = &metadata.jwks {
                    if jwks.keys.is_empty() {
                        return Err(OAuthError::InvalidClientMetadataError(
                            "private_key_jwt auth method requires at least one key in jwks"
                                .to_string(),
                        ));
                    }
                } else {
                    return Err(OAuthError::InvalidClientMetadataError(
                        "private_key_jwt auth method requires at least one key in jwks".to_string(),
                    ));
                }
                if metadata.token_endpoint_auth_signing_alg.is_none() {
                    return Err(OAuthError::InvalidClientMetadataError(
                        "Missing token_endpoint_auth_signing_alg client metadata".to_string(),
                    ));
                }
            }
            _ => {
                let method = method.as_str();
                return Err(OAuthError::InvalidClientMetadataError(
                    format!("{method} is not a supported \"token_endpoint_auth_method\". Use \"private_key_jwt\" or \"none\"."),
                ));
            }
        }

        if metadata.authorization_encrypted_response_enc.is_some() {
            return Err(OAuthError::InvalidClientMetadataError(
                "Encrypted authorization response is not supported".to_string(),
            ));
        }

        if metadata
            .tls_client_certificate_bound_access_tokens
            .is_some()
        {
            return Err(OAuthError::InvalidClientMetadataError(
                "Mutual-TLS bound access tokens are not supported".to_string(),
            ));
        }

        if metadata.authorization_encrypted_response_enc.is_some()
            && metadata.authorization_encrypted_response_alg.is_none()
        {
            return Err(OAuthError::InvalidClientMetadataError("authorization_encrypted_response_enc requires authorization_encrypted_response_alg".to_string()));
        }

        // ATPROTO spec requires the use of DPoP (OAuth spec defaults to false)
        if metadata.dpop_bound_access_tokens.is_none() {
            return Err(OAuthError::InvalidClientMetadataError(
                "\"dpop_bound_access_tokens\" must be true".to_string(),
            ));
        }

        // ATPROTO spec requires the use of PKCE, does not support OIDC
        if !metadata.response_types.contains(&OAuthResponseType::Code) {
            return Err(OAuthError::InvalidClientMetadataError(
                "response_types must include \"code\"".to_string(),
            ));
        } else if !metadata
            .grant_types
            .contains(&OAuthGrantType::AuthorizationCode)
        {
            return Err(OAuthError::InvalidClientMetadataError("The \"code\" response type requires that \"grant_types\" contains \"authorization_code\"".to_string()));
        }

        if metadata.redirect_uris.is_empty() {
            // ATPROTO spec requires that at least one redirect URI is provided
            return Err(OAuthError::InvalidClientMetadataError(
                "At least one redirect_uri is required".to_string(),
            ));
        }

        if metadata.application_type == ApplicationType::Web
            && metadata.grant_types.contains(&OAuthGrantType::Implicit)
        {
            // https://openid.net/specs/openid-connect-registration-1_0.html#rfc.section.2
            //
            // > Web Clients [as defined by "application_type"] using the OAuth
            // > Implicit Grant Type MUST only register URLs using the https
            // > scheme as redirect_uris; they MUST NOT use localhost as the
            // > hostname.
            for redirect_uri in metadata.redirect_uris.clone() {
                match redirect_uri {
                    OAuthRedirectUri::Https(redirect_uri) => {
                        let url = Url::parse(redirect_uri.as_str()).unwrap();
                        if url.host_str().unwrap() == "localhost" {
                            return Err(OAuthError::InvalidClientMetadataError(
                                "Web clients must not use localhost as the hostname".to_string(),
                            ));
                        }
                    }
                    _ => {
                        return Err(OAuthError::InvalidClientMetadataError(
                            "Web clients must use HTTPS redirect URIs".to_string(),
                        ));
                    }
                }
            }
        }

        for redirect_uri in metadata.redirect_uris.clone() {
            let url = Url::parse(redirect_uri.as_str()).unwrap();

            if !url.username().is_empty() || url.password().is_some() {
                // Is this a valid concern? Should we allow credentials in the URI?
                return Err(OAuthError::InvalidRedirectUriError(format!(
                    "Redirect URI {url} must not contain credentials"
                )));
            }

            let host = url.host_str().unwrap();

            // FIRST: Loopback redirect URI exception (only for native apps)
            if host == "localhost" {
                // https://datatracker.ietf.org/doc/html/rfc8252#section-8.3
                //
                // > While redirect URIs using localhost (i.e.,
                // > "http://localhost:{port}/{path}") function similarly to loopback IP
                // > redirects described in Section 7.3, the use of localhost is NOT
                // > RECOMMENDED. Specifying a redirect URI with the loopback IP literal
                // > rather than localhost avoids inadvertently listening on network
                // > interfaces other than the loopback interface. It is also less
                // > susceptible to client-side firewalls and misconfigured host name
                // > resolution on the user's device.
                return Err(OAuthError::InvalidRedirectUriError(format!(
                    "Loopback redirect URI {url} is not allowed (use explicit IPs instead)"
                )));
            }

            if host == "127.0.0.1" || host == "[::1]" {
                // Only allowed for native apps
                if metadata.application_type != ApplicationType::Native {
                    return Err(OAuthError::InvalidRedirectUriError(
                        "Loopback redirect URIs are only allowed for native apps".to_string(),
                    ));
                }

                if redirect_uri.is_https() {
                    // https://datatracker.ietf.org/doc/html/rfc8252#section-7.3
                    //
                    // > Loopback redirect URIs use the "http" scheme and are constructed
                    // > with the loopback IP literal and whatever port the client is
                    // > listening on. That is, "http://127.0.0.1:{port}/{path}" for IPv4,
                    // > and "http://[::1]:{port}/{path}" for IPv6.
                    return Err(OAuthError::InvalidRedirectUriError(format!(
                        "Loopback redirect URI {url} must use HTTP"
                    )));
                }
            }
        }

        if let Ok(_) = OAuthClientIdLoopback::new(client_id) {
            self.validate_loopback_client_metadata(metadata)
        } else if let Ok(client_id) = OAuthClientIdDiscoverable::new(client_id) {
            return self.validate_discoverable_client_metadata(&client_id, metadata);
        } else {
            Ok(metadata)
        }
    }

    fn validate_loopback_client_metadata(
        &self,
        metadata: OAuthClientMetadata,
    ) -> Result<OAuthClientMetadata, OAuthError> {
        if metadata.client_uri.is_some() {
            return Err(OAuthError::InvalidClientMetadataError(
                "client_uri is not allowed for loopback clients".to_string(),
            ));
        }

        if metadata.application_type != ApplicationType::Native {
            return Err(OAuthError::InvalidClientMetadataError(
                "Loopback clients must have application_type \"native\"".to_string(),
            ));
        }

        if let Some(method) = metadata.token_endpoint_auth_method {
            let method = method.as_str();
            if method != "none" {
                return Err(OAuthError::InvalidClientMetadataError(
                    format!("Loopback clients are not allowed to use \"token_endpoint_auth_method\" {method}"),
                ));
            }
        }

        for redirect_uri in metadata.redirect_uris.clone() {
            if !redirect_uri.is_loopback() {
                return Err(OAuthError::InvalidClientMetadataError(
                    "Loopback clients must use loopback direct URIS".to_string(),
                ));
            }
        }

        Ok(metadata)
    }

    fn validate_discoverable_client_metadata(
        &self,
        client_id: &OAuthClientIdDiscoverable,
        metadata: OAuthClientMetadata,
    ) -> Result<OAuthClientMetadata, OAuthError> {
        if metadata.client_id.is_none() {
            // https://drafts.aaronpk.com/draft-parecki-oauth-client-id-metadata-document/draft-parecki-oauth-client-id-metadata-document.html
            return Err(OAuthError::InvalidClientMetadataError(
                "client_id is required for discoverable clients".to_string(),
            ));
        }

        let client_id_url = Url::parse(client_id.clone().to_string().as_str()).unwrap();

        if let Some(client_uri) = metadata.client_uri.clone() {
            // https://drafts.aaronpk.com/draft-parecki-oauth-client-id-metadata-document/draft-parecki-oauth-client-id-metadata-document.html
            //
            // The client_uri must be a parent of the client_id URL. This might be
            // relaxed in the future.
            let client_uri_url = Url::parse(client_uri.clone().to_string().as_str()).unwrap();

            if client_uri_url.origin() != client_id_url.origin() {
                return Err(OAuthError::InvalidClientMetadataError(
                    "client_uri must have the same origin as the client_id".to_string(),
                ));
            }

            if client_id_url.path() != client_uri_url.path() {
                let path = if client_uri_url.path().ends_with("/") {
                    client_uri_url.path().to_string()
                } else {
                    client_uri_url.path().to_string() + "/"
                };
                if !client_id_url.path().starts_with(path.as_str()) {
                    return Err(OAuthError::InvalidClientMetadataError(
                        "client_uri must be a parent URL of the client_id".to_string(),
                    ));
                }
            }
        }

        if let Some(method) = metadata.token_endpoint_auth_method {
            match method {
                OAuthEndpointAuthMethod::ClientSecretBasic => {
                    return Err(OAuthError::InvalidClientMetadataError(
                        "Client authentication method is not allowed for discoverable clients"
                            .to_string(),
                    ));
                }
                OAuthEndpointAuthMethod::ClientSecretJwt => {
                    return Err(OAuthError::InvalidClientMetadataError(
                        "Client authentication method is not allowed for discoverable clients"
                            .to_string(),
                    ));
                }
                OAuthEndpointAuthMethod::ClientSecretPost => {
                    return Err(OAuthError::InvalidClientMetadataError(
                        "Client authentication method is not allowed for discoverable clients"
                            .to_string(),
                    ));
                }
                OAuthEndpointAuthMethod::None => {}
                OAuthEndpointAuthMethod::PrivateKeyJwt => {}
                _ => {
                    return Err(OAuthError::InvalidClientMetadataError(
                        "Unsupported client authentication method ".to_string(),
                    ));
                }
            }
        }

        for redirect_uri in metadata.redirect_uris.clone() {
            match redirect_uri {
                OAuthRedirectUri::Https(redirect_uri) => {
                    // https://datatracker.ietf.org/doc/html/rfc8252#section-8.4
                    //
                    // > In addition to the collision-resistant properties, requiring a
                    // > URI scheme based on a domain name that is under the control of
                    // > the app can help to prove ownership in the event of a dispute
                    // > where two apps claim the same private-use URI scheme (where one
                    // > app is acting maliciously).
                    //
                    // Although this only applies to "native" clients (extract being from
                    // rfc8252), we apply this rule to "web" clients as well.
                    let url = Url::parse(redirect_uri.as_str()).unwrap();
                    if url.host_str().unwrap() != client_id_url.host_str().unwrap() {
                        let client_uri = metadata.client_uri.unwrap();
                        return Err(OAuthError::InvalidRedirectUriError(
                            format!("Redirect URI {url} must be under the same domain as client_id {client_uri}"),
                        ));
                    }
                }
                OAuthRedirectUri::PrivateUse(redirect_uri) => {
                    // https://datatracker.ietf.org/doc/html/rfc8252#section-8.4
                    //
                    // > In addition to the collision-resistant properties, requiring a
                    // > URI scheme based on a domain name that is under the control of
                    // > the app can help to prove ownership in the event of a dispute
                    // > where two apps claim the same private-use URI scheme (where one
                    // > app is acting maliciously).

                    // https://drafts.aaronpk.com/draft-parecki-oauth-client-id-metadata-document/draft-parecki-oauth-client-id-metadata-document.html
                    //
                    // Fully qualified domain name (FQDN) of the client_id, in reverse
                    // order. This could be relaxed to allow same apex domain names, or
                    // parent domains, but for now we require an exact match.
                    //TODO
                }
                _ => {}
            }
        }

        Ok(metadata)
    }
}

pub async fn fetch_jwks_handler(uri: String, options: Option<GetCachedOptions>) -> JWKSet<Empty> {
    let client = reqwest::Client::new();
    let response = client
        .get(uri)
        .header("accept", "application/json")
        .send()
        .await
        .unwrap();
    let jwks = response.json::<JWKSet<Empty>>().await.unwrap();
    jwks
}

pub async fn fetch_metadata_handler(
    uri: String,
    options: Option<GetCachedOptions>,
) -> OAuthClientMetadata {
    let client = reqwest::Client::new();
    let response = client
        .get(uri)
        .header("accept", "application/json")
        .send()
        .await
        .unwrap();
    let metadata = response.json::<OAuthClientMetadata>().await.unwrap();
    metadata
}

#[cfg(test)]
mod tests {
    use crate::jwk::Keyset;
    use crate::jwk_jose::jose_key::JoseKey;
    use crate::oauth_provider::client::client::Client;
    use crate::oauth_provider::client::client_info::ClientInfo;
    use crate::oauth_provider::client::client_manager::{ClientManager, LoopbackMetadataGetter};
    use crate::oauth_provider::client::client_store::ClientStore;
    use crate::oauth_provider::errors::OAuthError;
    use crate::oauth_provider::oauth_hooks::OAuthHooks;
    use crate::oauth_types::{
        ApplicationType, OAuthAuthorizationServerMetadata, OAuthClientId, OAuthClientIdLoopback,
        OAuthClientMetadata, OAuthEndpointAuthMethod, OAuthGrantType, OAuthIssuerIdentifier,
        OAuthRedirectUri, OAuthResponseType, OAuthScope, ValidUri, WebUri,
    };
    use crate::simple_store_memory::SimpleStoreMemory;
    use biscuit::jwa::Algorithm;
    use biscuit::jwk::{
        AlgorithmParameters, CommonParameters, EllipticCurveKeyParameters, JWKSet, PublicKeyUse,
        RSAKeyParameters, JWK,
    };
    use biscuit::{jwa, Empty};
    use num_bigint::BigUint;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    struct TestClientStore {}

    impl ClientStore for TestClientStore {
        fn find_client(&self, client_id: OAuthClientId) -> Result<OAuthClientMetadata, OAuthError> {
            unimplemented!()
        }
    }

    fn create_server_metadata() -> OAuthAuthorizationServerMetadata {
        OAuthAuthorizationServerMetadata::new(
            OAuthIssuerIdentifier::new("https://pds.ripperoni.com").unwrap(),
            WebUri::validate("https://pds.ripperoni.com/oauth/authorize").unwrap(),
            WebUri::validate("https://pds.ripperoni.com/oauth/token").unwrap(),
        )
    }

    async fn create_keyset() -> Keyset {
        let jwk = JWK {
            common: CommonParameters {
                algorithm: Some(Algorithm::Signature(jwa::SignatureAlgorithm::RS256)),
                key_id: Some("2011-04-29".to_string()),
                ..Default::default()
            },
            algorithm: AlgorithmParameters::RSA(RSAKeyParameters {
                n: BigUint::new(vec![
                    2661337731, 446995658, 1209332140, 183172752, 955894533, 3140848734, 581365968,
                    3217299938, 3520742369, 1559833632, 1548159735, 2303031139, 1726816051,
                    92775838, 37272772, 1817499268, 2876656510, 1328166076, 2779910671, 4258539214,
                    2834014041, 3172137349, 4008354576, 121660540, 1941402830, 1620936445,
                    993798294, 47616683, 272681116, 983097263, 225284287, 3494334405, 4005126248,
                    1126447551, 2189379704, 4098746126, 3730484719, 3232696701, 2583545877,
                    428738419, 2533069420, 2922211325, 2227907999, 4154608099, 679827337,
                    1165541732, 2407118218, 3485541440, 799756961, 1854157941, 3062830172,
                    3270332715, 1431293619, 3068067851, 2238478449, 2704523019, 2826966453,
                    1548381401, 3719104923, 2605577849, 2293389158, 273345423, 169765991,
                    3539762026,
                ]),
                e: BigUint::new(vec![65537]),
                ..Default::default()
            }),
            additional: Default::default(),
        };
        let jose_key = JoseKey::from_jwk(jwk, None).await;
        let issuer = OAuthIssuerIdentifier::new("http://pds.ripperoni.com").unwrap();
        Keyset::new(vec![Box::new(jose_key)])
    }

    fn create_hooks() -> OAuthHooks {
        OAuthHooks {
            on_client_info: Some(Box::new(
                |client_id: OAuthClientId,
                 oauth_client_metadata: OAuthClientMetadata,
                 jwks: Option<JWKSet<Empty>>|
                 -> ClientInfo {
                    ClientInfo {
                        is_first_party: client_id
                            == OAuthClientId::new(
                                "https://cleanfollow-bsky.pages.dev/client-metadata.json",
                            )
                            .unwrap(),
                        is_trusted: false,
                    }
                },
            )),
            on_authorization_details: None,
        }
    }

    fn create_loopback_metadata() -> LoopbackMetadataGetter {
        unimplemented!()
    }

    fn create_client_jwks_cache() -> SimpleStoreMemory<String, JWKSet<Empty>> {
        SimpleStoreMemory::default()
    }

    fn create_client_metadata_cache() -> SimpleStoreMemory<String, OAuthClientMetadata> {
        SimpleStoreMemory::default()
    }

    async fn create_client_manager() -> ClientManager {
        let server_metadata = create_server_metadata();
        let keyset = Arc::new(RwLock::new(create_keyset().await));
        let hooks = Arc::new(create_hooks());
        let store = Arc::new(RwLock::new(TestClientStore {}));
        let loopback_metadata =
            Box::new(
                move |client_id: OAuthClientIdLoopback| OAuthClientMetadata {
                    redirect_uris: vec![
                        OAuthRedirectUri::new("http://127.0.0.1/").unwrap(),
                        OAuthRedirectUri::new("http://[::1]/").unwrap(),
                    ],
                    response_types: vec![OAuthResponseType::Code],
                    grant_types: vec![
                        OAuthGrantType::AuthorizationCode,
                        OAuthGrantType::RefreshToken,
                    ],
                    scope: Some(OAuthScope::new("atproto").unwrap()),
                    token_endpoint_auth_method: Some(OAuthEndpointAuthMethod::None),
                    token_endpoint_auth_signing_alg: None,
                    userinfo_signed_response_alg: None,
                    userinfo_encrypted_response_alg: None,
                    jwks_uri: None,
                    jwks: None,
                    application_type: ApplicationType::Native,
                    subject_type: None,
                    request_object_signing_alg: None,
                    id_token_signed_response_alg: None,
                    authorization_signed_response_alg: "".to_string(),
                    authorization_encrypted_response_enc: None,
                    authorization_encrypted_response_alg: None,
                    client_id: Some(client_id.as_str().to_string()),
                    client_name: None,
                    client_uri: None,
                    policy_uri: None,
                    tos_uri: None,
                    logo_uri: None,
                    default_max_age: None,
                    require_auth_time: None,
                    contacts: None,
                    tls_client_certificate_bound_access_tokens: None,
                    dpop_bound_access_tokens: Some(true),
                    authorization_details_types: None,
                },
            );
        let client_jwks_cache = Arc::new(RwLock::new(create_client_jwks_cache()));
        let client_metadata_cache = Arc::new(RwLock::new(create_client_metadata_cache()));
        ClientManager::new(
            server_metadata,
            keyset,
            hooks,
            store,
            Some(loopback_metadata),
            client_jwks_cache,
            client_metadata_cache,
        )
    }

    #[tokio::test]
    async fn test_get_loopback_client() {
        let client_manager = create_client_manager().await;
        let client_id = OAuthClientId::new("http://localhost/").unwrap();
        let result = client_manager.get_client(&client_id).await.unwrap();
        let expected = Client {
            id: OAuthClientId::new("http://localhost/").unwrap(),
            metadata: OAuthClientMetadata {
                redirect_uris: vec![
                    OAuthRedirectUri::new("http://127.0.0.1/").unwrap(),
                    OAuthRedirectUri::new("http://[::1]/").unwrap(),
                ],
                response_types: vec![OAuthResponseType::Code],
                grant_types: vec![
                    OAuthGrantType::AuthorizationCode,
                    OAuthGrantType::RefreshToken,
                ],
                scope: Some(OAuthScope::new("atproto").unwrap()),
                token_endpoint_auth_method: Some(OAuthEndpointAuthMethod::None),
                token_endpoint_auth_signing_alg: None,
                userinfo_signed_response_alg: None,
                userinfo_encrypted_response_alg: None,
                jwks_uri: None,
                jwks: None,
                application_type: ApplicationType::Native,
                subject_type: None,
                request_object_signing_alg: None,
                id_token_signed_response_alg: None,
                authorization_signed_response_alg: "".to_string(),
                authorization_encrypted_response_enc: None,
                authorization_encrypted_response_alg: None,
                client_id: Some(client_id.as_str().to_string()),
                client_name: None,
                client_uri: None,
                policy_uri: None,
                tos_uri: None,
                logo_uri: None,
                default_max_age: None,
                require_auth_time: None,
                contacts: None,
                tls_client_certificate_bound_access_tokens: None,
                dpop_bound_access_tokens: Some(true),
                authorization_details_types: None,
            },
            jwks: None,
            info: ClientInfo {
                is_first_party: false,
                is_trusted: false,
            },
        };
        assert_eq!(result, expected);
    }
}
