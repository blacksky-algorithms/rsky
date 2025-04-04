use crate::oauth_provider::account::account::Account;
use crate::oauth_provider::client::client::Client;
use crate::oauth_provider::client::client_info::ClientInfo;
use crate::oauth_types::{
    OAuthAuthorizationDetails, OAuthAuthorizationRequestParameters, OAuthClientId,
    OAuthClientMetadata,
};
use jsonwebtoken::jwk::JwkSet;

pub type OnClientInfo =
    Box<dyn Fn(OAuthClientId, OAuthClientMetadata, Option<JwkSet>) -> ClientInfo>;

pub type OnAuthorizationDetails =
    Box<dyn Fn(Client, OAuthAuthorizationRequestParameters, Account) -> OAuthAuthorizationDetails>;

pub struct OAuthHooks {
    /**
     * Use this to alter, override or validate the client metadata & jwks returned
     * by the client store.
     *
     * @throws {InvalidClientMetadataError} if the metadata is invalid
     * @see {@link InvalidClientMetadataError}
     */
    pub on_client_info: OnClientInfo,
    /**
     * Allows enriching the authorization details with additional information
     * when the tokens are issued.
     *
     * @see {@link https://datatracker.ietf.org/doc/html/rfc9396 | RFC 9396}
     */
    pub on_authorization_details: OnAuthorizationDetails,
}
