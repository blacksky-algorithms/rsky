use crate::oauth_provider::errors::OAuthError;
use crate::oauth_types::{OAuthClientId, OAuthClientMetadata};

pub trait ClientStore: Send + Sync {
    fn find_client(&self, client_id: OAuthClientId) -> Result<OAuthClientMetadata, OAuthError>;
}
