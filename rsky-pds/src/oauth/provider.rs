use crate::oauth::SharedOAuthProvider;
use jsonwebtoken::jwk::{
    AlgorithmParameters, CommonParameters, EllipticCurve, EllipticCurveKeyParameters,
    EllipticCurveKeyType, Jwk, JwkSet, KeyAlgorithm, KeyOperations, PublicKeyUse,
};
use rsky_oauth::jwk::Keyset;
use rsky_oauth::oauth_provider::metadata::build_metadata::CustomMetadata;
use rsky_oauth::oauth_provider::oauth_provider::{OAuthProvider, OAuthProviderCreatorParams};
use rsky_oauth::oauth_types::OAuthIssuerIdentifier;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct AuthProviderOptions {
    pub issuer: OAuthIssuerIdentifier,
}

pub fn build_oauth_provider(options: AuthProviderOptions) -> SharedOAuthProvider {
    let custom_metadata = build_custom_metadata();

    let keyset = build_keyset();

    let keyset = Arc::new(RwLock::new(keyset));
    SharedOAuthProvider {
        oauth_provider: Arc::new(RwLock::new(OAuthProvider::creator(
            OAuthProviderCreatorParams {
                authentication_max_age: None,
                token_max_age: None,
                metadata: Some(custom_metadata),
                customization: None,
                safe_fetch: false,
                redis: None,
                store: None,
                client_jwks_cache: None,
                client_metadata_cache: None,
                loopback_metadata: "".to_string(),
                dpop_secret: None,
                dpop_step: None,
                issuer: options.issuer,
                keyset: Some(keyset.clone()),
                access_token_type: None,
            },
        ))),
        keyset: keyset.clone(),
    }
}

fn build_keyset() -> Keyset {
    let mut keys = Vec::new();
    let key = Jwk {
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
    keys.push(key);
    let jwk_set = JwkSet { keys };
    Keyset::new(jwk_set)
}

fn build_custom_metadata() -> CustomMetadata {
    CustomMetadata {
        scopes_supported: Some(vec![
            "transition:generic".to_string(),
            "transition:chat.bsky".to_string(),
        ]),
        authorization_details_type_supported: None,
        protected_resources: None,
    }
}
