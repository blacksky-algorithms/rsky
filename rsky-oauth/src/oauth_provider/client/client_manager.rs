use crate::jwk::Keyset;
use crate::oauth_provider::client::client::Client;
use crate::oauth_provider::client::client_info::ClientInfo;
use crate::oauth_provider::client::client_store::ClientStore;
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_types::{
    ApplicationType, OAuthAuthorizationServerMetadata, OAuthClientId, OAuthClientIdDiscoverable,
    OAuthClientIdLoopback, OAuthClientMetadata, OAuthEndpointAuthMethod, OAuthGrantType,
    OAuthResponseType, SubjectType,
};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub type ClientManagerCreator =
    Box<dyn Fn(Arc<RwLock<dyn ClientStore>>, Arc<RwLock<Keyset>>) -> ClientManager + Send + Sync>;

pub struct ClientManager {
    // jwks: BTreeMap<String, Jwk>,
    // metadata_getter: BTreeMap<String, OAuthClientMetadata>,
    server_metadata: OAuthAuthorizationServerMetadata,
    keyset: Arc<RwLock<Keyset>>,
    store: Arc<RwLock<dyn ClientStore>>,
    // loopback_metadata: Option<LoopbackMetadataGetter>,
}

impl ClientManager {
    pub fn creator(metadata: OAuthAuthorizationServerMetadata) -> ClientManagerCreator {
        Box::new(
            move |store: Arc<RwLock<dyn ClientStore>>,
                  keyset: Arc<RwLock<Keyset>>|
                  -> ClientManager { ClientManager::new(store, keyset, metadata) },
        )
    }

    pub fn new(
        store: Arc<RwLock<dyn ClientStore>>,
        keyset: Arc<RwLock<Keyset>>,
        server_metadata: OAuthAuthorizationServerMetadata,
    ) -> Self {
        Self {
            server_metadata,
            keyset,
            store,
        }
    }

    /**
     * @see {@link https://openid.net/specs/openid-connect-registration-1_0.html#rfc.section.2 OIDC Client Registration}
     */
    pub async fn get_client(&self, client_id: &OAuthClientId) -> Result<Client, OAuthError> {
        unimplemented!()
        // let metadata = self.get_client_metadata(client_id).await?;
        //
        // let jwks = match metadata.jwks_uri {
        //     None => {
        //         None
        //     }
        //     Some(jwks_uri) => {
        //         unimplemented!()
        //     }
        // };
        //
        // let partial_info: ClientInfo;
        //
        // Ok(Client::new(client_id.clone(), metadata, jwks, partial_info))
    }

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
            return self.get_stored_client_metadata(client_id).await;
        }
    }

    async fn get_loopback_client_metadata(
        &self,
        client_id: OAuthClientIdLoopback,
    ) -> Result<OAuthClientMetadata, OAuthError> {
        // if  {  }
        unimplemented!()
    }

    async fn get_discoverable_client_metadata(
        &self,
        client_id: &OAuthClientIdDiscoverable,
    ) -> Result<OAuthClientMetadata, OAuthError> {
        unimplemented!()
        // let metadata_url = client_id.as_url();

        // let metadata = self.metadata_getter.get(metadata_url.as_str()).await;

        // Note: we do *not* re-validate the metadata here, as the metadata is
        // validated within the getter. This is to avoid double validation.
        //
        // return this.validateClientMetadata(metadataUrl.href, metadata)
        // Ok(metadata)
    }

    async fn get_stored_client_metadata(
        &self,
        client_id: &OAuthClientId,
    ) -> Result<OAuthClientMetadata, OAuthError> {
        let metadata = self.store.blocking_read().find_client(client_id.clone())?;
        self.validate_client_metadata(&client_id, metadata).await
    }

    /**
     * This method will ensure that the client metadata is valid w.r.t. the OAuth
     * and OIDC specifications. It will also ensure that the metadata is
     * compatible with the implementation of this library, and ATPROTO's
     * requirements.
     */
    async fn validate_client_metadata(
        &self,
        client_id: &OAuthClientId,
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
                "Unsupported parameter".to_string(),
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
                "Missing atproto scope".to_string(),
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
            if metadata_client_id != client_id.clone() {
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
        // match method {
        //     OAuthEndpointAuthMethod::None => {
        //         if metadata.token_endpoint_auth_signing_alg.is_some() {
        //             return Err(OAuthError::InvalidClientMetadataError("token_endpoint_auth_method none must not have token_endpoint_auth_signing_alg".to_string()));
        //         }
        //     }
        //     OAuthEndpointAuthMethod::PrivateKeyJwt => {
        //         if metadata.jwks.is_none() && metadata.jwks_uri.is_none() {
        //             return Err(OAuthError::InvalidClientMetadataError("private_key_jwt auth method requires jwks or jwks_uri".to_string()));
        //         }
        //
        //         if let Some(jwks) = metadata.jwks {
        //
        //         }
        //     }
        //     _ => {}
        // }

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
            for redirect_uri in metadata.redirect_uris.clone() {}
        }

        if let Ok(client_loopback_id) = OAuthClientIdLoopback::new(client_id.val()) {
            self.validate_loopback_client_metadata(client_id, metadata)
        } else if let Ok(discoverable_client_id) = OAuthClientIdDiscoverable::new(client_id.val()) {
            return self.validate_discoverable_client_metadata(client_id, metadata);
        } else {
            Ok(metadata)
        }
    }

    fn validate_loopback_client_metadata(
        &self,
        client_id: &OAuthClientId,
        metadata: OAuthClientMetadata,
    ) -> Result<OAuthClientMetadata, OAuthError> {
        if metadata.client_uri.is_some() {
            return Err(OAuthError::InvalidClientMetadataError(
                "client_uri is not allowed for loopback clients".to_string(),
            ));
        }

        if metadata.application_type == ApplicationType::Native {
            return Err(OAuthError::InvalidClientMetadataError(
                "Loopback clients must have application_type \"native\"".to_string(),
            ));
        }

        if let Some(method) = metadata.token_endpoint_auth_method {
            return Err(OAuthError::InvalidClientMetadataError(
                "Loopback clients are not allowed to use \"token_endpoint_auth_method\""
                    .to_string(),
            ));
        }

        for redirect_uri in metadata.redirect_uris.clone() {
            //TODO
        }

        Ok(metadata)
    }

    fn validate_discoverable_client_metadata(
        &self,
        client_id: &OAuthClientId,
        metadata: OAuthClientMetadata,
    ) -> Result<OAuthClientMetadata, OAuthError> {
        if metadata.client_id.is_none() {
            // https://drafts.aaronpk.com/draft-parecki-oauth-client-id-metadata-document/draft-parecki-oauth-client-id-metadata-document.html
            return Err(OAuthError::InvalidClientMetadataError(
                "client_id is required for discoverable clients".to_string(),
            ));
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
            //TODO
        }

        Ok(metadata)
    }
}
