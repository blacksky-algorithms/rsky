use jsonwebtoken::jwk::{
    AlgorithmParameters, CommonParameters, EllipticCurve, EllipticCurveKeyParameters,
    EllipticCurveKeyType, Jwk, JwkSet, KeyAlgorithm, KeyOperations, PublicKeyUse,
};
use rsky_oauth::jwk::Keyset;
use rsky_oauth::oauth_provider::account::account_store::AccountStore;
use rsky_oauth::oauth_provider::client::client_store::ClientStore;
use rsky_oauth::oauth_provider::device::device_store::DeviceStore;
use rsky_oauth::oauth_provider::metadata::build_metadata::CustomMetadata;
use rsky_oauth::oauth_provider::oauth_provider::{OAuthProvider, OAuthProviderOptions};
use rsky_oauth::oauth_provider::replay::replay_store_memory::ReplayStoreMemory;
use rsky_oauth::oauth_provider::request::request_store_memory::RequestStoreMemory;
use rsky_oauth::oauth_provider::token::token_store::TokenStore;
use rsky_oauth::oauth_types::OAuthIssuerIdentifier;

pub fn build_oauth_provider() -> OAuthProvider {
    let issuer = OAuthIssuerIdentifier::new("https://rsky.com").expect("Valid Issuer");
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
    let keyset = Keyset::new(jwk_set);
    let custom_metadata = CustomMetadata {
        scopes_supported: Some(vec![
            "transition:generic".to_string(),
            "transition:chat.bsky".to_string(),
        ]),
        authorization_details_type_supported: None,
        protected_resources: None,
    };
    let oauth_options = OAuthProviderOptions {
        authentication_max_age: None,
        token_max_age: None,
        metadata: Some(custom_metadata),
        customization: None,
        safe_fetch: false,
        redis: "".to_string(),
        account_store: AccountStore {},
        device_store: DeviceStore {},
        client_store: ClientStore {},
        replay_store: ReplayStoreMemory::new(),
        request_store: RequestStoreMemory::new(),
        token_store: TokenStore {},
        client_jwks_cache: None,
        client_metadata_cache: None,
        loopback_metadata: "".to_string(),
        dpop_secret: None,
        dpop_step: None,
        issuer,
        keyset: Some(keyset),
        access_token_type: None,
    };
    OAuthProvider::new(oauth_options).unwrap()
}
