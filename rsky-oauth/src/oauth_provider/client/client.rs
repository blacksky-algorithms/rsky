use crate::jwk::VerifyOptions;
use crate::jwk_jose::jose_key::JwkSet;
use crate::oauth_provider::client::client_auth::ClientAuth;
use crate::oauth_provider::client::client_info::ClientInfo;
use crate::oauth_provider::constants::{CLIENT_ASSERTION_MAX_AGE, JAR_MAX_AGE};
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_provider::lib::util::redirect_uri::compare_redirect_uri;
use crate::oauth_provider::now_as_secs;
use crate::oauth_provider::token::token_claims::TokenClaims;
use crate::oauth_provider::token::token_data::TokenData;
use crate::oauth_types::{
    OAuthAuthorizationRequestParameters, OAuthClientCredentials, OAuthClientId,
    OAuthClientMetadata, OAuthEndpointAuthMethod, OAuthGrantType, OAuthIssuerIdentifier,
    OAuthRedirectUri, OAuthResponseType, CLIENT_ASSERTION_TYPE_JWT_BEARER,
};
use biscuit::Validation;
use rocket::form::validate::Len;
use rocket::yansi::Paint;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::str::FromStr;

/**
 * @see {@link https://www.iana.org/assignments/oauth-parameters/oauth-parameters.xhtml#token-endpoint-auth-method}
 */
pub const AUTH_METHODS_SUPPORTED: [&str; 2] = ["none", "private_key_jwt"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Client {
    pub id: OAuthClientId,
    pub metadata: OAuthClientMetadata,
    pub jwks: Option<JwkSet>,
    pub info: ClientInfo,
}

impl Client {
    pub fn new(
        id: OAuthClientId,
        metadata: OAuthClientMetadata,
        jwks: Option<JwkSet>,
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
    ) -> Result<
        (
            OAuthAuthorizationRequestParameters,
            Option<(String, String, String)>,
        ),
        OAuthError,
    > {
        // match &self.metadata.request_object_signing_alg {
        //     None => {
        //         // https://openid.net/specs/openid-connect-registration-1_0.html#rfc.section.2
        //         // > The default, if omitted, is that any algorithm supported by the OP
        //         // > and the RP MAY be used.
        //         let mut validation = Validation::new(Algorithm::ES256);
        //         validation.reject_tokens_expiring_in_less_than =
        //             (JAR_MAX_AGE / 1000) + now_as_secs();
        //         let data = self.jwt_verify(jar.to_string(), validation).await;
        //         Ok((
        //             OAuthAuthorizationRequestParameters {
        //                 client_id: self.id.clone(),
        //                 state: None,
        //                 redirect_uri: None,
        //                 scope: None,
        //                 response_type: OAuthResponseType::Code,
        //                 code_challenge: None,
        //                 code_challenge_method: None,
        //                 dpop_jkt: None,
        //                 response_mode: None,
        //                 nonce: None,
        //                 max_age: None,
        //                 claims: None,
        //                 login_hint: None,
        //                 ui_locales: None,
        //                 id_token_hint: None,
        //                 display: None,
        //                 prompt: None,
        //                 authorization_details: None,
        //             },
        //             Some((
        //                 data.header.kid.unwrap(),
        //                 format!("{:?}", data.header.alg),
        //                 data.header.typ.unwrap(), //TODO
        //             )),
        //         ))
        //     }
        //     Some(request_object_signing_alg) => {
        //         if request_object_signing_alg == "none" {
        //             let mut validation = Validation::new(Algorithm::ES256);
        //             validation.reject_tokens_expiring_in_less_than =
        //                 (JAR_MAX_AGE / 1000) + now_as_secs();
        //             let data = self.jwt_verify(jar.to_string(), validation).await;
        //             Ok((
        //                 OAuthAuthorizationRequestParameters {
        //                     client_id: self.id.clone(),
        //                     state: None,
        //                     redirect_uri: None,
        //                     scope: None,
        //                     response_type: OAuthResponseType::Code,
        //                     code_challenge: None,
        //                     code_challenge_method: None,
        //                     dpop_jkt: None,
        //                     response_mode: None,
        //                     nonce: None,
        //                     max_age: None,
        //                     claims: None,
        //                     login_hint: None,
        //                     ui_locales: None,
        //                     id_token_hint: None,
        //                     display: None,
        //                     prompt: None,
        //                     authorization_details: None,
        //                 },
        //                 Some((
        //                     data.header.kid.unwrap(),
        //                     format!("{:?}", data.header.alg),
        //                     data.header.typ.unwrap(), //TODO
        //                 )),
        //             ))
        //         } else {
        //             let mut validation = Validation::new(Algorithm::ES256);
        //             validation.reject_tokens_expiring_in_less_than =
        //                 (JAR_MAX_AGE / 1000) + now_as_secs();
        //             validation
        //                 .algorithms
        //                 .push(Algorithm::from_str(request_object_signing_alg).unwrap());
        //             let data = self.jwt_verify(jar.to_string(), validation).await;
        //             Ok((
        //                 OAuthAuthorizationRequestParameters {
        //                     client_id: self.id.clone(),
        //                     state: None,
        //                     redirect_uri: None,
        //                     scope: None,
        //                     response_type: OAuthResponseType::Code,
        //                     code_challenge: None,
        //                     code_challenge_method: None,
        //                     dpop_jkt: None,
        //                     response_mode: None,
        //                     nonce: None,
        //                     max_age: None,
        //                     claims: None,
        //                     login_hint: None,
        //                     ui_locales: None,
        //                     id_token_hint: None,
        //                     display: None,
        //                     prompt: None,
        //                     authorization_details: None,
        //                 },
        //                 Some((
        //                     data.header.kid.unwrap(),
        //                     format!("{:?}", data.header.alg),
        //                     data.header.typ.unwrap(), //TODO
        //                 )),
        //             ))
        //         }
        //     }
        // }
        unimplemented!()
    }

    async fn jwt_verify_unsecured(&self, token: String, options: String) {
        unimplemented!()
    }

    async fn jwt_verify(&self, token: String, validation: VerifyOptions) -> TokenData {
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
        // let method = self.metadata.token_endpoint_auth_method.unwrap();
        //
        // /**
        //  * @see {@link https://www.iana.org/assignments/oauth-parameters/oauth-parameters.xhtml#token-endpoint-auth-method}
        //  */
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
        //     OAuthEndpointAuthMethod::PrivateKeyJwt => match input {
        //         OAuthClientCredentials::JwtBearer(credentials) => {
        //             let mut validation = Validation::new(Algorithm::HS256);
        //             validation.sub = Some(self.id.val());
        //             validation.reject_tokens_expiring_in_less_than =
        //                 (CLIENT_ASSERTION_MAX_AGE / 1000) + now_as_secs();
        //             let mut audience = HashSet::new();
        //             audience.insert(aud.to_string());
        //             validation.aud = Some(audience);
        //             let mut required_claims = HashSet::new();
        //             required_claims.insert(String::from("jti"));
        //             validation.required_spec_claims = required_claims;
        //             let result = self
        //                 .jwt_verify(credentials.client_assertion, validation)
        //                 .await;
        //
        //             let kid = match result.header.kid.clone() {
        //                 None => {
        //                     return Err(OAuthError::InvalidClientError(
        //                         "\"kid\" required in client_assertion".to_string(),
        //                     ))
        //                 }
        //                 Some(kid) => kid,
        //             };
        //
        //             let client_auth = ClientAuth {
        //                 method: CLIENT_ASSERTION_TYPE_JWT_BEARER.to_string(),
        //                 alg: format!("{:?}", result.header.alg),
        //                 kid,
        //                 jkt: "todo".to_string(),
        //             };
        //             let jti = result.claims.jti.unwrap();
        //             Ok((client_auth, Some(jti.val())))
        //         }
        //         _ => Err(OAuthError::InvalidRequestError(
        //             "client_assertion_type required for ".to_string(),
        //         )),
        //     },
        //     _ => Err(OAuthError::InvalidClientMetadataError(
        //         "Unsupported token_endpoint_auth_method".to_string(),
        //     )),
        // }
        unimplemented!()
    }

    /**
     * Ensures that a {@link ClientAuth} generated in the past is still valid wrt
     * the current client metadata & jwks. This is used to invalidate tokens when
     * the client stops advertising the key that it used to authenticate itself
     * during the initial token request.
     */
    pub async fn validate_client_auth(&self, client_auth: &ClientAuth) -> bool {
        if client_auth.method == "none" {
            return match self.metadata.token_endpoint_auth_method {
                None => false,
                Some(token_endpoint_auth_method) => match token_endpoint_auth_method {
                    OAuthEndpointAuthMethod::None => true,
                    _ => false,
                },
            };
        }

        if client_auth.method == CLIENT_ASSERTION_TYPE_JWT_BEARER {
            return match self.metadata.token_endpoint_auth_method.unwrap() {
                OAuthEndpointAuthMethod::PrivateKeyJwt => {
                    // let key;
                    // const key = await this.keyGetter(
                    //     {
                    //         kid: clientAuth.kid,
                    //         alg: clientAuth.alg,
                    //     },
                    //     { payload: '', signature: '' },
                    // )
                    //todo
                    // let jtk = auth_jwk_thumbprint(key).await;

                    unimplemented!()
                    // jtk == client_auth.jkt
                }
                _ => false,
            };
        }

        false
    }

    /**
     * Validates the request parameters against the client metadata.
     */
    pub fn validate_request(
        &self,
        parameters: OAuthAuthorizationRequestParameters,
    ) -> Result<OAuthAuthorizationRequestParameters, OAuthError> {
        if parameters.client_id != self.id {
            return Err(OAuthError::InvalidParametersError(parameters, "The client_id parameter field does not match the value used to authenticate the client".to_string()));
        }

        if let Some(scope) = parameters.scope.clone() {
            // Any scope requested by the client must be registered in the client
            // metadata.
            let declared_scopes: Vec<String> = scope.iter().map(|x| x.to_string()).collect();

            if declared_scopes.is_empty() {
                return Err(OAuthError::InvalidScopeError(
                    parameters,
                    "Client has no declared scopes in its metadata".to_string(),
                ));
            }

            for scope in parameters
                .scope
                .clone()
                .unwrap()
                .iter()
                .map(|val| val.to_string())
            {
                if !declared_scopes.contains(&scope) {
                    return Err(OAuthError::InvalidScopeError(
                        parameters,
                        format!("Scope \"{scope}\" is not declared in the client metadata"),
                    ));
                }
            }
        } else {
            return Err(OAuthError::InvalidScopeError(
                parameters,
                "Client has no declared scopes in its metadata".to_string(),
            ));
        }

        if !self
            .metadata
            .response_types
            .contains(&parameters.response_type)
        {
            return Err(OAuthError::InvalidParametersError(
                parameters,
                "Invalid response type requested by the client".to_string(),
            ));
        }

        if parameters.response_type.includes_code()
            && !self
                .metadata
                .grant_types
                .contains(&OAuthGrantType::AuthorizationCode)
        {
            return Err(OAuthError::InvalidParametersError(
                parameters,
                "The client is not allowed to use the authorization_code grant type".to_string(),
            ));
        }

        let redirect_uri = parameters.redirect_uri.clone();
        let mut parameters = parameters.clone();
        match redirect_uri {
            None => {
                match self.default_redirect_uri() {
                    None => {
                        // https://datatracker.ietf.org/doc/html/draft-ietf-oauth-v2-1-10#authorization-request
                        //
                        // > "redirect_uri": OPTIONAL if only one redirect URI is registered for
                        // > this client. REQUIRED if multiple redirect URIs are registered for this
                        // > client.
                        return Err(OAuthError::InvalidParametersError(
                            parameters,
                            "redirect_uri is required".to_string(),
                        ));
                    }
                    Some(redirect_uri) => {
                        parameters.redirect_uri = Some(redirect_uri);
                    }
                }
            }
            Some(redirect_uri) => {
                let mut invalid_redirect_uri = false;
                for metadata_redirect in &self.metadata.redirect_uris {
                    if !compare_redirect_uri(metadata_redirect.clone(), redirect_uri.clone()) {
                        invalid_redirect_uri = true;
                        break;
                    }
                }
                if invalid_redirect_uri {
                    return Err(OAuthError::InvalidParametersError(
                        parameters,
                        "Invalid redirect_uri".to_string(),
                    ));
                }
            }
        }

        if let Some(authorization_details) = parameters.authorization_details.clone() {
            match self.metadata.authorization_details_types.clone() {
                None => {
                    return Err(OAuthError::InvalidAuthorizationDetailsError(
                        parameters,
                        "Client Metadata does not declare any authorization_details".to_string(),
                    ))
                }
                Some(authorization_details_types) => {
                    for detail in authorization_details {
                        if !authorization_details_types.contains(&detail.type_().to_string()) {
                            return Err(OAuthError::InvalidAuthorizationDetailsError(parameters, "Client Metadata does not declare any authorization_details of type".to_string()));
                        }
                    }
                }
            }
        }
        Ok(parameters)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jwk_jose::jose_key::JwkSet;
    use crate::oauth_types::OAuthClientCredentialsNone;

    fn create_client() -> Client {
        let id = OAuthClientId::new("client123").unwrap();
        let metadata = OAuthClientMetadata {
            redirect_uris: vec![],
            response_types: vec![],
            grant_types: vec![],
            scope: None,
            token_endpoint_auth_method: None,
            token_endpoint_auth_signing_alg: None,
            userinfo_signed_response_alg: None,
            userinfo_encrypted_response_alg: None,
            jwks_uri: None,
            jwks: None,
            application_type: Default::default(),
            subject_type: None,
            request_object_signing_alg: None,
            id_token_signed_response_alg: None,
            authorization_signed_response_alg: "".to_string(),
            authorization_encrypted_response_enc: None,
            authorization_encrypted_response_alg: None,
            client_id: None,
            client_name: None,
            client_uri: None,
            policy_uri: None,
            tos_uri: None,
            logo_uri: None,
            default_max_age: None,
            require_auth_time: None,
            contacts: None,
            tls_client_certificate_bound_access_tokens: None,
            dpop_bound_access_tokens: None,
            authorization_details_types: None,
        };
        let jwks = JwkSet { keys: vec![] };
        let info = ClientInfo {
            is_first_party: false,
            is_trusted: false,
        };
        Client::new(id, metadata, None, info)
    }
    #[tokio::test]
    async fn test_decode_request_object() {
        let client = create_client();
        let jar = "{}";
        let res = client.decode_request_object(jar).await.unwrap();
        // let text = "rsky.com".to_string();
        // let result = validate_url(&text);
        // assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_verify_credentials() {
        let client = create_client();
        let input = OAuthClientCredentials::None(OAuthClientCredentialsNone::new(
            OAuthClientId::new("client123").unwrap(),
        ));
        let aud = OAuthIssuerIdentifier::new("https://rsky.com").unwrap();
        client.verify_credentials(input, &aud).await.unwrap();
    }

    #[test]
    fn test_validate_request() {
        let client = create_client();
        let client_id = OAuthClientId::new("client123").unwrap();
        let parameters = OAuthAuthorizationRequestParameters {
            client_id,
            state: None,
            redirect_uri: None,
            scope: None,
            response_type: OAuthResponseType::Code,
            code_challenge: None,
            code_challenge_method: None,
            dpop_jkt: None,
            response_mode: None,
            nonce: None,
            max_age: None,
            claims: None,
            login_hint: None,
            ui_locales: None,
            id_token_hint: None,
            display: None,
            prompt: None,
            authorization_details: None,
        };
        client.validate_request(parameters).unwrap();
    }

    #[tokio::test]
    async fn test_validate_client_auth() {
        let client = create_client();
        let client_auth = ClientAuth {
            method: "urn:ietf:params:oauth:client-assertion-type:jwt-bearer".to_string(),
            alg: "".to_string(),
            kid: "".to_string(),
            jkt: "".to_string(),
        };
        let result = client.validate_client_auth(&client_auth).await;
        assert_eq!(result, true)
    }
}
