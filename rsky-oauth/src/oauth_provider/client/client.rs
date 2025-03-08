pub const AUTH_METHODS_SUPPORTED: [&str; 2] = ["none", "private_key_jwt"];

use crate::oauth_provider::client::client_auth::ClientAuth;
use crate::oauth_provider::client::client_id::ClientId;
use crate::oauth_provider::client::client_info::ClientInfo;
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_types::{
    OAuthAuthorizationRequestParameters, OAuthClientCredentials, OAuthClientMetadata,
    OAuthIssuerIdentifier,
};

#[derive(Clone)]
pub struct Client {
    pub id: ClientId,
    pub metadata: OAuthClientMetadata,
    pub jwks: String,
    pub info: ClientInfo,
}

impl Client {
    pub fn new(
        id: ClientId,
        metadata: OAuthClientMetadata,
        jwks: String,
        info: ClientInfo,
    ) -> Self {
        Client {
            id,
            metadata,
            jwks,
            info,
        }
    }

    pub async fn decode_request_object(
        &self,
        jar: &str,
    ) -> Result<OAuthAuthorizationRequestParameters, OAuthError> {
        unimplemented!()
    }

    async fn jwt_verify_unsecured(&self, token: String, options: String) {
        unimplemented!()
    }

    async fn jwt_verify(&self, token: String, options: Option<String>) {
        unimplemented!()
    }

    pub async fn verify_credentials(
        &self,
        input: OAuthClientCredentials,
        aud: &OAuthIssuerIdentifier,
    ) -> (ClientAuth, Option<String>) {
        unimplemented!()
    }

    pub async fn validate_client_auth(&self, client_auth: ClientAuth) -> bool {
        if client_auth.method == "none" {
            //TODO
        }

        unimplemented!()
    }

    pub fn validate_request(
        &self,
        parameters: OAuthAuthorizationRequestParameters,
    ) -> OAuthAuthorizationRequestParameters {
        unimplemented!()
    }
}
