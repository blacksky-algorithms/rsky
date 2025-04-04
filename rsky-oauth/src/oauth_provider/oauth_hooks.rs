use crate::oauth_provider::client::client_info::ClientInfo;
use crate::oauth_types::{OAuthClientId, OAuthClientMetadata};
use jsonwebtoken::jwk::JwkSet;

pub type OnClientInfo =
    Box<dyn Fn(OAuthClientId, OAuthClientMetadata, Option<JwkSet>) -> ClientInfo>;

pub type On = Box<dyn Fn(OAuthClientId, OAuthClientMetadata, Option<JwkSet>) -> ClientInfo>;

pub struct OAuthHooks {
    /**
     * Use this to alter, override or validate the client metadata & jwks returned
     * by the client store.
     *
     * @throws {InvalidClientMetadataError} if the metadata is invalid
     * @see {@link InvalidClientMetadataError}
     */
    pub on_client_info: OnClientInfo,
}
