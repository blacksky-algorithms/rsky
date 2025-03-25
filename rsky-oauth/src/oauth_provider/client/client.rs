pub const AUTH_METHODS_SUPPORTED: [&str; 2] = ["none", "private_key_jwt"];

use rocket::form::validate::Contains;
use crate::oauth_provider::client::client_auth::ClientAuth;
use crate::oauth_provider::client::client_id::ClientId;
use crate::oauth_provider::client::client_info::ClientInfo;
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_types::{OAuthAuthorizationRequestParameters, OAuthClientCredentials, OAuthClientMetadata, OAuthGrantType, OAuthIssuerIdentifier, OAuthRedirectUri};

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

    /**
     * Validates the request parameters against the client metadata.
     */
    pub fn validate_request(
        &self,
        parameters: OAuthAuthorizationRequestParameters,
    ) -> Result<OAuthAuthorizationRequestParameters, OAuthError> {
        if parameters.client_id != self.id {
            return Err(OAuthError::InvalidParametersError("The client_id parameter field does not match the value used to authenticate the client".to_string()));
        }

        if let Some(scope) = parameters.scope {
            // Any scope requested by the client must be registered in the client
            // metadata.
            return Err(OAuthError::InvalidScopeError("Client has no declared scopes in its metadata".to_string()));
            //todo
        }

        if !self.metadata.response_types.contains(parameters.response_type) {
            return Err(OAuthError::InvalidParametersError("Invalid response type requested by the client".to_string()));
        }

        if parameters.response_type.includes_code() {
            if !self.metadata.grant_types.contains(OAuthGrantType::AuthorizationCode) {
                return Err(OAuthError::InvalidParametersError("The client is not allowed to use the authorization_code grant type".to_string()));
            }
        }

        let redirect_uri = parameters.redirect_uri;
        match redirect_uri {
            None => {
                let default_redirect_uri =
            }
            Some(redirect_uri) => {
                //todo
                // if self.metadata.redirect_uris
            }
        }
    }
}
