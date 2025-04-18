use crate::oauth_provider::client::client::AUTH_METHODS_SUPPORTED;
use crate::oauth_provider::lib::util::crypto::VERIFY_ALGOS;
use crate::oauth_types::{
    OAuthAuthorizationServerMetadata, OAuthCodeChallengeMethod, OAuthGrantType,
    OAuthIssuerIdentifier, ValidUri, WebUri,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CustomMetadata {
    pub scopes_supported: Option<Vec<String>>,
    pub authorization_details_type_supported: Option<Vec<String>>,
    pub protected_resources: Option<Vec<WebUri>>,
}

/**
 * @see {@link https://datatracker.ietf.org/doc/html/rfc8414#section-2}
 * @see {@link https://openid.net/specs/openid-connect-discovery-1_0.html#ProviderMetadata}
 */
pub fn build_metadata(
    issuer: OAuthIssuerIdentifier,
    custom_metadata: Option<CustomMetadata>,
) -> OAuthAuthorizationServerMetadata {
    let mut authorization_details_types_supported = None;
    let mut protected_resources = None;

    let mut token_endpoint_auth_methods_supported = Vec::new();
    for method in AUTH_METHODS_SUPPORTED {
        token_endpoint_auth_methods_supported.push(method.to_string());
    }

    let mut token_endpoint_auth_signing_alg_values_supported = Vec::new();
    let mut dpop_signing_alg_values_supported = Vec::new();
    let mut request_object_signing_alg_values_supported = vec!["none".to_string()];
    for algo in VERIFY_ALGOS {
        request_object_signing_alg_values_supported.push(algo.to_string());
        dpop_signing_alg_values_supported.push(algo.to_string());
        token_endpoint_auth_signing_alg_values_supported.push(algo.to_string());
    }

    let _issuer = issuer.clone();
    let base = _issuer.into_inner();
    let jwks_uri = WebUri::validate((base.clone() + "/oauth/jwks").as_str()).unwrap();
    let authorization_endpoint =
        WebUri::validate((base.clone() + "/oauth/authorize").as_str()).unwrap();
    let token_endpoint = WebUri::validate((base.clone() + "/oauth/token").as_str()).unwrap();
    let revocation_endpoint = WebUri::validate((base.clone() + "/oauth/revoke").as_str()).unwrap();
    let introspection_endpoint =
        WebUri::validate((base.clone() + "/oauth/introspect").as_str()).unwrap();
    let pushed_authorization_request_endpoint =
        WebUri::validate((base.clone() + "/oauth/par").as_str()).unwrap();

    let mut scopes_supported = vec!["atproto".to_string()];
    if let Some(custom_metadata) = custom_metadata {
        authorization_details_types_supported =
            custom_metadata.authorization_details_type_supported;
        protected_resources = custom_metadata.protected_resources;

        if let Some(custom_scopes_supported) = custom_metadata.scopes_supported {
            for scope in custom_scopes_supported {
                scopes_supported.push(scope);
            }
        }
    }

    let subject_types_supported = vec![
        //
        "public".to_string(), // The same "sub" is returned for all clients
                              // 'pairwise', // A different "sub" is returned for each client
    ];

    let response_types_supported = vec![
        // OAuth
        "code".to_string(), // 'token',

                            // OpenID
                            // 'none',
                            // 'code id_token token',
                            // 'code id_token',
                            // 'code token',
                            // 'id_token token',
                            // 'id_token',
    ];

    let response_modes_supported = vec![
        // https://openid.net/specs/oauth-v2-multiple-response-types-1_0.html#ResponseModes
        "query".to_string(),
        "fragment".to_string(),
        // https://openid.net/specs/oauth-v2-form-post-response-mode-1_0.html#FormPostResponseMode
        "form_post".to_string(),
    ];

    let grant_types_supported = vec![
        OAuthGrantType::AuthorizationCode,
        OAuthGrantType::RefreshToken,
    ];

    let code_challenge_methods_supported = vec![
        // https://www.iana.org/assignments/oauth-parameters/oauth-parameters.xhtml#pkce-code-challenge-method
        OAuthCodeChallengeMethod::S256, // atproto does not allow "plain"
                                        // 'plain',
    ];

    let ui_locales_supported = vec!["en-US".to_string()];

    let display_values_supported =
        vec!["page".to_string(), "popup".to_string(), "touch".to_string()];

    OAuthAuthorizationServerMetadata {
        issuer,
        claims_supported: None,
        claims_locales_supported: None,
        claims_parameter_supported: None,

        scopes_supported: Some(scopes_supported),
        subject_types_supported: Some(subject_types_supported),
        response_types_supported: Some(response_types_supported),
        response_modes_supported: Some(response_modes_supported),
        grant_types_supported: Some(grant_types_supported),
        code_challenge_methods_supported: Some(code_challenge_methods_supported),
        ui_locales_supported: Some(ui_locales_supported),
        id_token_signing_alg_values_supported: None,
        display_values_supported: Some(display_values_supported),

        // https://datatracker.ietf.org/doc/html/rfc9207
        authorization_response_iss_parameter_supported: Some(true),

        // https://datatracker.ietf.org/doc/html/rfc9101#section-4
        request_object_signing_alg_values_supported: Some(
            request_object_signing_alg_values_supported,
        ),
        request_object_encryption_alg_values_supported: Some(vec![]),
        request_object_encryption_enc_values_supported: Some(vec![]),

        request_parameter_supported: Some(true),
        request_uri_parameter_supported: Some(true),
        require_request_uri_registration: Some(true),

        jwks_uri: Some(jwks_uri),

        authorization_endpoint,

        token_endpoint,
        token_endpoint_auth_methods_supported: Some(token_endpoint_auth_methods_supported),
        token_endpoint_auth_signing_alg_values_supported: Some(
            token_endpoint_auth_signing_alg_values_supported,
        ),

        revocation_endpoint: Some(revocation_endpoint),

        introspection_endpoint: Some(introspection_endpoint),

        // https://datatracker.ietf.org/doc/html/rfc9126#section-5
        pushed_authorization_request_endpoint: Some(pushed_authorization_request_endpoint),

        require_pushed_authorization_requests: Some(true),

        // https://datatracker.ietf.org/doc/html/rfc9449#section-5.1
        dpop_signing_alg_values_supported: Some(dpop_signing_alg_values_supported),

        // https://datatracker.ietf.org/doc/html/rfc9396#section-14.4
        authorization_details_types_supported,

        // https://datatracker.ietf.org/doc/html/draft-ietf-oauth-resource-metadata-05#section-4
        protected_resources,

        // https://drafts.aaronpk.com/draft-parecki-oauth-client-id-metadata-document/draft-parecki-oauth-client-id-metadata-document.html
        client_id_metadata_document_supported: Some(true),

        userinfo_endpoint: None,
        end_session_endpoint: None,
        registration_endpoint: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rocket::yansi::Paint;

    #[test]
    fn test_build_metadata() {
        let issuer = OAuthIssuerIdentifier::new("https://pds.ripperoni.com").unwrap();
        let custom_metadata: Option<CustomMetadata> = Some(CustomMetadata {
            scopes_supported: Some(vec![
                "transition:generic".to_string(),
                "transition:chat.bsky".to_string(),
            ]),
            authorization_details_type_supported: None,
            protected_resources: Some(vec![WebUri::validate(issuer.as_ref()).unwrap()]),
        });
        let result = build_metadata(issuer.clone(), custom_metadata);
        let expected = OAuthAuthorizationServerMetadata {
            issuer,
            claims_supported: None,
            claims_locales_supported: None,
            claims_parameter_supported: None,
            request_parameter_supported: Some(true),
            request_uri_parameter_supported: Some(true),
            require_request_uri_registration: Some(true),
            scopes_supported: Some(vec![
                "atproto".to_string(),
                "transition:generic".to_string(),
                "transition:chat.bsky".to_string(),
            ]),
            subject_types_supported: Some(vec!["public".to_string()]),
            response_types_supported: Some(vec!["code".to_string()]),
            response_modes_supported: Some(vec![
                "query".to_string(),
                "fragment".to_string(),
                "form_post".to_string(),
            ]),
            grant_types_supported: Some(vec![
                OAuthGrantType::AuthorizationCode,
                OAuthGrantType::RefreshToken,
            ]),
            code_challenge_methods_supported: Some(vec![OAuthCodeChallengeMethod::S256]),
            ui_locales_supported: Some(vec!["en-US".to_string()]),
            id_token_signing_alg_values_supported: None,
            display_values_supported: Some(vec![
                "page".to_string(),
                "popup".to_string(),
                "touch".to_string(),
            ]),
            request_object_signing_alg_values_supported: Some(vec![
                "none".to_string(),
                "RS256".to_string(),
                "RS384".to_string(),
                "RS512".to_string(),
                "PS256".to_string(),
                "PS384".to_string(),
                "PS512".to_string(),
                "ES256".to_string(),
                "ES256K".to_string(),
                "ES384".to_string(),
                "ES512".to_string(),
            ]),
            authorization_response_iss_parameter_supported: Some(true),
            authorization_details_types_supported: None,
            request_object_encryption_alg_values_supported: Some(vec![]),
            request_object_encryption_enc_values_supported: Some(vec![]),
            jwks_uri: Some(WebUri::validate("https://pds.ripperoni.com/oauth/jwks").unwrap()),
            authorization_endpoint: WebUri::validate("https://pds.ripperoni.com/oauth/authorize")
                .unwrap(),
            token_endpoint: WebUri::validate("https://pds.ripperoni.com/oauth/token").unwrap(),
            token_endpoint_auth_methods_supported: Some(vec![
                "none".to_string(),
                "private_key_jwt".to_string(),
            ]),
            token_endpoint_auth_signing_alg_values_supported: Some(vec![
                "RS256".to_string(),
                "RS384".to_string(),
                "RS512".to_string(),
                "PS256".to_string(),
                "PS384".to_string(),
                "PS512".to_string(),
                "ES256".to_string(),
                "ES256K".to_string(),
                "ES384".to_string(),
                "ES512".to_string(),
            ]),
            revocation_endpoint: Some(
                WebUri::validate("https://pds.ripperoni.com/oauth/revoke").unwrap(),
            ),
            introspection_endpoint: Some(
                WebUri::validate("https://pds.ripperoni.com/oauth/introspect").unwrap(),
            ),
            pushed_authorization_request_endpoint: Some(
                WebUri::validate("https://pds.ripperoni.com/oauth/par").unwrap(),
            ),
            require_pushed_authorization_requests: Some(true),
            userinfo_endpoint: None,
            end_session_endpoint: None,
            registration_endpoint: None,
            dpop_signing_alg_values_supported: Some(vec![
                "RS256".to_string(),
                "RS384".to_string(),
                "RS512".to_string(),
                "PS256".to_string(),
                "PS384".to_string(),
                "PS512".to_string(),
                "ES256".to_string(),
                "ES256K".to_string(),
                "ES384".to_string(),
                "ES512".to_string(),
            ]),
            protected_resources: Some(vec![WebUri::validate("https://pds.ripperoni.com").unwrap()]),
            client_id_metadata_document_supported: Some(true),
        };
        assert_eq!(result, expected)
    }
}
