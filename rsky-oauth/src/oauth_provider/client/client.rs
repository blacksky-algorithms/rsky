pub const AUTH_METHODS_SUPPORTED: [&str; 2] = ["none", "private_key_jwt"];

use crate::oauth_provider::client::client_auth::ClientAuth;
use crate::oauth_provider::client::client_info::ClientInfo;
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_types::{
    OAuthAuthorizationRequestParameters, OAuthClientCredentials, OAuthClientId,
    OAuthClientMetadata, OAuthIssuerIdentifier, OAuthRedirectUri,
};

#[derive(Clone)]
pub struct Client {
    pub id: OAuthClientId,
    pub metadata: OAuthClientMetadata,
    pub jwks: String,
    pub info: ClientInfo,
}

impl Client {
    pub fn new(
        id: OAuthClientId,
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

    /**
     * @see {@link https://datatracker.ietf.org/doc/html/rfc6749#section-2.3.1}
     * @see {@link https://datatracker.ietf.org/doc/html/rfc7523#section-3}
     * @see {@link https://www.iana.org/assignments/oauth-parameters/oauth-parameters.xhtml#token-endpoint-auth-method}
     */
    pub async fn verify_credentials(
        &self,
        input: OAuthClientCredentials,
        aud: &OAuthIssuerIdentifier,
    ) -> Result<(ClientAuth, Option<String>), OAuthError> {
        unimplemented!()
        // let method = self.metadata.token_endpoint_auth_method;
        //
        // match method {
        //     OAuthEndpointAuthMethod::None => {
        //         let client_auth = ClientAuth {
        //             method: "none".to_string(),
        //             alg: "".to_string(),
        //             kid: "".to_string(),
        //             jkt: "".to_string(),
        //         };
        //         Ok((client_auth, None))
        //     }
        //     OAuthEndpointAuthMethod::PrivateKeyJwt => {
        //         match input {
        //             OAuthClientCredentials::JwtBearer(credentials) => {
        //                 unimplemented!();
        //                 // let result = self.jwt_verify(credentials.client_assertion, todo!()).await;
        //             }
        //             _ => Err(OAuthError::InvalidRequestError(
        //                 "client_assertion_type required for ".to_string(),
        //             )),
        //         }
        //     }
        //     _ => Err(OAuthError::InvalidClientMetadataError(
        //         "Unsupported token_endpoint_auth_method".to_string(),
        //     )),
        // }
    }

    /**
     * Ensures that a {@link ClientAuth} generated in the past is still valid wrt
     * the current client metadata & jwks. This is used to invalidate tokens when
     * the client stops advertising the key that it used to authenticate itself
     * during the initial token request.
     */
    pub async fn validate_client_auth(&self, client_auth: &ClientAuth) -> bool {
        unimplemented!()
        // if client_auth.method == "none" {
        //     return match self.metadata.token_endpoint_auth_method {
        //         OAuthEndpointAuthMethod::None => true,
        //         _ => false,
        //     };
        // }
        //
        // if client_auth.method == CLIENT_ASSERTION_TYPE_JWT_BEARER {
        //     return match self.metadata.token_endpoint_auth_method {
        //         OAuthEndpointAuthMethod::PrivateKeyJwt => {
        //             let key;
        //             // const key = await this.keyGetter(
        //             //     {
        //             //         kid: clientAuth.kid,
        //             //         alg: clientAuth.alg,
        //             //     },
        //             //     { payload: '', signature: '' },
        //             // )
        //             //todo
        //             let jtk = auth_jwk_thumbprint(key).await;
        //
        //             jtk == client_auth.jkt
        //         }
        //         _ => { false }
        //     }
        // }
        //
        // false
    }

    /**
     * Validates the request parameters against the client metadata.
     */
    pub fn validate_request(
        &self,
        parameters: OAuthAuthorizationRequestParameters,
    ) -> Result<OAuthAuthorizationRequestParameters, OAuthError> {
        unimplemented!()
        // if parameters.client_id != self.id {
        //     return Err(OAuthError::InvalidParametersError("The client_id parameter field does not match the value used to authenticate the client".to_string()));
        // }
        //
        // if let Some(scope) = parameters.scope {
        //     // Any scope requested by the client must be registered in the client
        //     // metadata.
        //     return Err(OAuthError::InvalidScopeError(
        //         "Client has no declared scopes in its metadata".to_string(),
        //     ));
        //     //todo
        // }
        //
        // if !self
        //     .metadata
        //     .response_types
        //     .contains(parameters.response_type)
        // {
        //     return Err(OAuthError::InvalidParametersError(
        //         "Invalid response type requested by the client".to_string(),
        //     ));
        // }
        //
        // if parameters.response_type.includes_code() {
        //     if !self
        //         .metadata
        //         .grant_types
        //         .contains(OAuthGrantType::AuthorizationCode)
        //     {
        //         return Err(OAuthError::InvalidParametersError(
        //             "The client is not allowed to use the authorization_code grant type"
        //                 .to_string(),
        //         ));
        //     }
        // }
        //
        // let redirect_uri = parameters.redirect_uri.clone();
        // let mut parameters = parameters.clone();
        // match redirect_uri {
        //     None => {
        //         match self.default_redirect_uri() {
        //             None => {
        //                 // https://datatracker.ietf.org/doc/html/draft-ietf-oauth-v2-1-10#authorization-request
        //                 //
        //                 // > "redirect_uri": OPTIONAL if only one redirect URI is registered for
        //                 // > this client. REQUIRED if multiple redirect URIs are registered for this
        //                 // > client.
        //                 return Err(OAuthError::InvalidParametersError(
        //                     "redirect_uri is required".to_string(),
        //                 ));
        //             }
        //             Some(redirect_uri) => {
        //                 parameters.redirect_uri = Some(redirect_uri);
        //             }
        //         }
        //     }
        //     Some(redirect_uri) => {
        //         let mut invalid_redirect_uri = false;
        //         for metadata_redirect in &self.metadata.redirect_uris {
        //             if !compare_redirect_uri(metadata_redirect.clone(), redirect_uri.clone()) {
        //                 invalid_redirect_uri = true;
        //                 break;
        //             }
        //         }
        //         if invalid_redirect_uri {
        //             return Err(OAuthError::InvalidParametersError(
        //                 "Invalid redirect_uri".to_string(),
        //             ));
        //         }
        //     }
        // }
        //
        // if let Some(authorization_details) = parameters.authorization_details.clone() {
        //     match self.metadata.authorization_details_types.clone() {
        //         None => {
        //             return Err(OAuthError::InvalidAuthorizationDetailsError(
        //                 "Client Metadata does not declare any authorization_details".to_string(),
        //             ))
        //         }
        //         Some(authorization_details_types) => {
        //             for detail in authorization_details {
        //                 if !authorization_details_types.contains(detail.type_()) {
        //                     return Err(OAuthError::InvalidAuthorizationDetailsError("Client Metadata does not declare any authorization_details of type".to_string()));
        //                 }
        //             }
        //         }
        //     }
        // }
        // Ok(parameters)
    }

    fn default_redirect_uri(&self) -> Option<OAuthRedirectUri> {
        let redirect_uris = &self.metadata.redirect_uris;
        if redirect_uris.len() == 1 {
            Some(redirect_uris.first().unwrap().clone())
        } else {
            None
        }
    }
}
