use crate::oauth_provider::client::client_id::ClientId;
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_types::OAuthClientMetadata;

pub trait ClientStore: Send + Sync {
    fn find_client(&self, client_id: ClientId) -> Result<OAuthClientMetadata, OAuthError>;
}
