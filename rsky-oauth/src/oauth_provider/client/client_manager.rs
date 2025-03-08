use crate::jwk::{JwkBase, Keyset};
use crate::oauth_provider::client::client::Client;
use crate::oauth_provider::client::client_id::ClientId;
use crate::oauth_provider::client::client_store::ClientStore;
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_provider::errors::OAuthError::{
    InvalidClientMetadataError, InvalidRedirectUriError,
};
use crate::oauth_types::{
    is_oauth_client_id_discoverable, is_oauth_client_id_loopback, OAuthAuthorizationServerMetadata,
    OAuthClientId, OAuthClientMetadata,
};
use serde_json::Value;

pub struct ClientManager {
    // jwks: (String, JwkBase),
    // metadata_getter: (String, OAuthClientMetadata),
    // server_metadata: OAuthAuthorizationServerMetadata,
    // keyset: Keyset,
    store: Option<ClientStore>,
}

impl ClientManager {
    pub fn new() -> Self {
        Self { store: None }
    }

    /**
     * @see {@link https://openid.net/specs/openid-connect-registration-1_0.html#rfc.section.2 OIDC Client Registration}
     */
    pub async fn get_client(&self, client_id: &OAuthClientId) -> Result<Client, OAuthError> {
        let metadata = self.get_client_metadata(client_id).await?;

        let jwks = metadata.jwks;

        unimplemented!()
    }

    async fn get_client_metadata(
        &self,
        client_id: &OAuthClientId,
    ) -> Result<OAuthClientMetadata, OAuthError> {
        if is_oauth_client_id_loopback(client_id) {
            self.get_loopback_client_metadata(client_id).await
        } else if is_oauth_client_id_discoverable(client_id) {
            return self.get_discoverable_client_metadata(client_id).await;
        } else if self.store.is_some() {
            return self.get_stored_client_metadata(client_id).await;
        } else {
            return Err(OAuthError::InvalidRequestError("test".to_string()));
        }
    }

    async fn get_loopback_client_metadata(
        &self,
        client_id: &OAuthClientId,
    ) -> Result<OAuthClientMetadata, OAuthError> {
        unimplemented!()
    }

    async fn get_discoverable_client_metadata(
        &self,
        client_id: &OAuthClientId,
    ) -> Result<OAuthClientMetadata, OAuthError> {
        unimplemented!()
    }

    async fn get_stored_client_metadata(
        &self,
        client_id: &OAuthClientId,
    ) -> Result<OAuthClientMetadata, OAuthError> {
        unimplemented!()
    }

    async fn validate_client_metadata(
        &self,
        client_id: &str,
        metadata: OAuthClientMetadata,
    ) -> Result<OAuthClientMetadata, OAuthError> {
        unimplemented!()
    }
}
