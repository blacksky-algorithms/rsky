use crate::oauth::SharedOAuthProvider;
use jsonwebtoken::jwk::{
    AlgorithmParameters, CommonParameters, EllipticCurve, EllipticCurveKeyParameters,
    EllipticCurveKeyType, Jwk, JwkSet, KeyAlgorithm, KeyOperations, PublicKeyUse,
};
use rsky_oauth::jwk::{Key, Keyset};
use rsky_oauth::jwk_jose::jose_key::JoseKey;
use rsky_oauth::oauth_provider::access_token::access_token_type::AccessTokenType;
use rsky_oauth::oauth_provider::client::client_info::ClientInfo;
use rsky_oauth::oauth_provider::client::client_manager::LoopbackMetadataGetter;
use rsky_oauth::oauth_provider::dpop::dpop_nonce::DpopNonceInput;
use rsky_oauth::oauth_provider::metadata::build_metadata::CustomMetadata;
use rsky_oauth::oauth_provider::oauth_hooks::OAuthHooks;
use rsky_oauth::oauth_provider::oauth_provider::{OAuthProvider, OAuthProviderCreatorParams};
use rsky_oauth::oauth_provider::output::customization::Customization;
use rsky_oauth::oauth_types::{
    HttpsUri, OAuthClientId, OAuthClientIdLoopback, OAuthClientMetadata, OAuthIssuerIdentifier,
    ValidUri, WebUri,
};
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct AuthProviderOptions {
    pub issuer: OAuthIssuerIdentifier,
    pub dpop_secret: Option<DpopNonceInput>,
    pub customization: Option<Customization>,
    pub redis: Option<String>,
}

pub async fn build_oauth_provider(options: AuthProviderOptions) -> SharedOAuthProvider {
    let custom_metadata = build_custom_metadata(options.issuer.clone());
    let keyset = Arc::new(RwLock::new(build_keyset().await));
    let loopback_metadata: LoopbackMetadataGetter = Box::new(
        |oauth_client_id_loopback: OAuthClientIdLoopback| -> OAuthClientMetadata {
            OAuthClientMetadata {
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
            }
        },
    );
    let oauth_hooks = OAuthHooks {
        on_client_info: Some(Box::new(
            |client_id: OAuthClientId,
             oauth_client_metadata: OAuthClientMetadata,
             jwks: Option<JwkSet>|
             -> ClientInfo {
                ClientInfo {
                    is_first_party: client_id == OAuthClientId::new("https://bsky.app/").unwrap(),
                    // @TODO make client client list configurable:
                    is_trusted: false,
                }
            },
        )),
        on_authorization_details: None,
    };
    SharedOAuthProvider {
        oauth_provider: Arc::new(RwLock::new(OAuthProvider::creator(
            OAuthProviderCreatorParams {
                authentication_max_age: None,
                token_max_age: None,
                metadata: Some(custom_metadata),
                customization: options.customization,
                safe_fetch: false,
                redis: options.redis,
                client_jwks_cache: None,
                client_metadata_cache: None,
                loopback_metadata: Some(loopback_metadata),
                dpop_secret: options.dpop_secret,
                dpop_step: None,
                issuer: options.issuer,
                keyset: Some(keyset.clone()),
                // If the PDS is bosh an authorization server & resource server (no
                // entryway), there is no need to use JWTs as access tokens. Instead,
                // the PDS can use tokenId as access tokens. This allows the PDS to
                // always use up-to-date token data from the token store.
                access_token_type: Some(AccessTokenType::ID),
                oauth_hooks: Arc::new(oauth_hooks),
            },
        ))),
        keyset: keyset.clone(),
    }
}

async fn build_keyset() -> Keyset {
    let mut keys = Vec::new();
    let jwk = Jwk {
        common: CommonParameters {
            public_key_use: Some(PublicKeyUse::Signature),
            key_operations: Some(vec![KeyOperations::Sign]),
            key_algorithm: Some(KeyAlgorithm::PS256),
            key_id: Some("test".to_string()),
            x509_url: None,
            x509_chain: None,
            x509_sha1_fingerprint: None,
            x509_sha256_fingerprint: None,
        },
        algorithm: AlgorithmParameters::EllipticCurve(EllipticCurveKeyParameters {
            key_type: EllipticCurveKeyType::EC,
            curve: EllipticCurve::P256,
            x: "GgskXhf9OJFxYNovWiwq35akQopFXS6Tzuv0Y-B6q8I".to_string(),
            y: "Cv8TnJVvra7TmYsaO-_nwhpD2jpfdnRE_TAeuvxLgJE".to_string(),
        }),
    };
    let key = JoseKey::from_jwk(jwk, None).await;
    keys.push(Box::new(key) as Box<dyn Key>);
    Keyset::new(keys)
}

// PdsOAuthProvider is used when the PDS is both an authorization server
// & resource server, in which case the issuer origin is also the
// resource server uri.
fn build_custom_metadata(issuer: OAuthIssuerIdentifier) -> CustomMetadata {
    CustomMetadata {
        scopes_supported: Some(vec![
            "transition:generic".to_string(),
            "transition:chat.bsky".to_string(),
        ]),
        authorization_details_type_supported: None,
        protected_resources: Some(vec![WebUri::validate(issuer.as_ref()).unwrap()]),
    }
}
