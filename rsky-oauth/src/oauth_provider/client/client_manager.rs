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
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct ClientManager {
    // jwks: (String, JwkBase),
    // metadata_getter: (String, OAuthClientMetadata),
    server_metadata: OAuthAuthorizationServerMetadata,
    keyset: Keyset,
    store: Arc<RwLock<dyn ClientStore>>,
}

impl ClientManager {
    pub fn new(
        server_metadata: OAuthAuthorizationServerMetadata,
        keyset: Keyset,
        store: Arc<RwLock<dyn ClientStore>>,
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
        } else {
            return self.get_stored_client_metadata(client_id).await;
        }
    }

    async fn get_loopback_client_metadata(
        &self,
        client_id: OAuthClientId,
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
        client_id: ClientId,
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
        client_id: &ClientId,
        metadata: OAuthClientMetadata,
    ) -> Result<OAuthClientMetadata, OAuthError> {
        unimplemented!()
    }
}
