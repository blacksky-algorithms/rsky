use crate::oauth_provider::client::client_id::ClientId;
use crate::oauth_types::OAuthClientMetadata;

pub struct ClientStore {}

impl ClientStore {
    pub async fn find_client(client_id: ClientId) -> OAuthClientMetadata {
        unimplemented!()
    }
}
