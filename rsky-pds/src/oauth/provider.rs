use crate::account_manager::AccountManager;
use crate::actor_store::ActorStore;
use crate::oauth::detailed_account_store::DetailedAccountStore;
use jsonwebtoken::jwk::{
    AlgorithmParameters, CommonParameters, EllipticCurve, EllipticCurveKeyParameters,
    EllipticCurveKeyType, Jwk, JwkSet, KeyAlgorithm, KeyOperations, PublicKeyUse,
};
use rsky_oauth::jwk::Keyset;
use rsky_oauth::oauth_provider::client::client_store::ClientStore;
use rsky_oauth::oauth_provider::metadata::build_metadata::CustomMetadata;
use rsky_oauth::oauth_provider::oauth_provider::{OAuthProvider, OAuthProviderOptions};
use rsky_oauth::oauth_types::OAuthIssuerIdentifier;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct AuthProviderOptions {
    pub issuer: OAuthIssuerIdentifier,
    pub account_manager: AccountManager,
    pub actor_store: ActorStore,
}

pub fn build_oauth_provider(options: AuthProviderOptions) -> OAuthProvider {
    let account_manager_guard = Arc::new(RwLock::new(options.account_manager.clone()));
    let request_store = account_manager_guard.clone();
    let device_store = account_manager_guard.clone();
    let token_store = account_manager_guard.clone();

    let keyset = build_keyset();

    let custom_metadata = build_custom_metadata();

    let account_store = Arc::new(RwLock::new(DetailedAccountStore::new(
        options.account_manager,
        options.actor_store,
    )));

    let oauth_options = OAuthProviderOptions {
        authentication_max_age: None,
        token_max_age: None,
        metadata: Some(custom_metadata),
        customization: None,
        safe_fetch: false,
        redis: "".to_string(),
        store: None,
        account_store: Some(account_store),
        device_store: Some(device_store),
        client_store: None,
        replay_store: None,
        request_store: Some(request_store),
        token_store: Some(token_store),
        client_jwks_cache: None,
        client_metadata_cache: None,
        loopback_metadata: "".to_string(),
        dpop_secret: None,
        dpop_step: None,
        issuer: options.issuer,
        keyset: Some(keyset),
        access_token_type: None,
    };
    OAuthProvider::new(oauth_options).unwrap()
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
