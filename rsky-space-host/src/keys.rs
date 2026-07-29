//! DID-document resolution: signing keys and service endpoints.
//!
//! Spec §Space authority: the `#atproto_space` verification method is preferred
//! and the account's `#atproto` signing key is the fallback.

use async_trait::async_trait;
use rsky_identity::did::atproto_data::{get_did_key_from_multibase, VerificationMaterial};
use rsky_identity::did::did_resolver::DidResolver;
use rsky_identity::types::DidDocument;
use std::sync::Arc;

use crate::authority::KeyResolver;
use crate::error::{HostError, Result};

/// Fetches DID documents. Production wraps [`DidResolver`]; tests hand-build docs.
#[async_trait]
pub trait DocSource: Send + Sync {
    async fn did_document(&self, did: &str) -> Result<DidDocument>;
}

/// [`DocSource`] backed by rsky-identity's plc/web resolver.
pub struct ResolverDocSource {
    resolver: tokio::sync::Mutex<DidResolver>,
}

impl ResolverDocSource {
    pub fn new(resolver: DidResolver) -> Self {
        Self {
            resolver: tokio::sync::Mutex::new(resolver),
        }
    }
}

#[async_trait]
impl DocSource for ResolverDocSource {
    async fn did_document(&self, did: &str) -> Result<DidDocument> {
        let mut resolver = self.resolver.lock().await;
        resolver
            .ensure_resolve(&did.to_string(), None)
            .await
            .map_err(|e| HostError::Resolution(e.to_string()))
    }
}

fn fragment_of(id: &str) -> Option<&str> {
    id.rsplit_once('#').map(|(_, frag)| frag)
}

/// The `did:key` for a doc's signing key: prefer `#atproto_space`, fall back to
/// `#atproto`.
pub fn signing_did_key_from_doc(doc: &DidDocument) -> Result<String> {
    let methods = doc
        .verification_method
        .as_deref()
        .ok_or_else(|| HostError::Resolution(format!("{}: no verification methods", doc.id)))?;
    let method = ["atproto_space", "atproto"]
        .iter()
        .find_map(|frag| methods.iter().find(|m| fragment_of(&m.id) == Some(frag)))
        .ok_or_else(|| HostError::Resolution(format!("{}: no atproto signing key", doc.id)))?;
    let multibase = method
        .public_key_multibase
        .clone()
        .ok_or_else(|| HostError::Resolution(format!("{}: key has no multibase", doc.id)))?;
    let material = VerificationMaterial {
        r#type: method.r#type.clone(),
        public_key_multibase: multibase,
    };
    get_did_key_from_multibase(material)
        .map_err(|e| HostError::Resolution(e.to_string()))?
        .ok_or_else(|| HostError::Resolution(format!("{}: unsupported key type", doc.id)))
}

/// The endpoint of the doc's service entry named by `fragment`.
pub fn service_endpoint_from_doc(doc: &DidDocument, fragment: &str) -> Result<String> {
    doc.service
        .as_deref()
        .and_then(|services| {
            services
                .iter()
                .find(|s| fragment_of(&s.id) == Some(fragment) || s.id == fragment)
        })
        .map(|s| s.service_endpoint.clone())
        .ok_or_else(|| HostError::Resolution(format!("{}: no #{fragment} service", doc.id)))
}

/// Production [`KeyResolver`]: resolve the DID doc, extract the signing key.
pub struct DocKeyResolver {
    docs: Arc<dyn DocSource>,
}

impl DocKeyResolver {
    pub fn new(docs: Arc<dyn DocSource>) -> Self {
        Self { docs }
    }
}

#[async_trait]
impl KeyResolver for DocKeyResolver {
    async fn signing_key(&self, did: &str) -> Result<String> {
        let doc = self.docs.did_document(did).await?;
        signing_did_key_from_doc(&doc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsky_identity::types::{Service, VerificationMethod};

    fn doc(methods: Vec<VerificationMethod>, services: Vec<Service>) -> DidDocument {
        DidDocument {
            context: None,
            id: "did:plc:subject".to_string(),
            also_known_as: None,
            verification_method: if methods.is_empty() {
                None
            } else {
                Some(methods)
            },
            service: if services.is_empty() {
                None
            } else {
                Some(services)
            },
        }
    }

    fn multikey_method(id: &str, secret: [u8; 32]) -> (VerificationMethod, String) {
        let sk = secp256k1::SecretKey::from_slice(&secret).unwrap();
        let pk = secp256k1::PublicKey::from_secret_key(&secp256k1::Secp256k1::new(), &sk);
        let did_key = rsky_crypto::utils::encode_did_key(&pk);
        // did:key strips to a bare multikey in DID documents.
        let multibase = did_key.strip_prefix("did:key:").unwrap().to_string();
        (
            VerificationMethod {
                id: id.to_string(),
                r#type: "Multikey".to_string(),
                controller: "did:plc:subject".to_string(),
                public_key_multibase: Some(multibase),
            },
            did_key,
        )
    }

    #[test]
    fn prefers_atproto_space_over_atproto() {
        let (space_m, space_key) = multikey_method("did:plc:subject#atproto_space", [0x41; 32]);
        let (atp_m, atp_key) = multikey_method("#atproto", [0x42; 32]);
        let d = doc(vec![atp_m.clone(), space_m], vec![]);
        assert_eq!(signing_did_key_from_doc(&d).unwrap(), space_key);

        let d = doc(vec![atp_m], vec![]);
        assert_eq!(signing_did_key_from_doc(&d).unwrap(), atp_key);
    }

    #[test]
    fn missing_or_unusable_keys_are_errors() {
        assert!(matches!(
            signing_did_key_from_doc(&doc(vec![], vec![])),
            Err(HostError::Resolution(_))
        ));
        let (other, _) = multikey_method("#unrelated", [0x43; 32]);
        assert!(signing_did_key_from_doc(&doc(vec![other], vec![])).is_err());

        let no_multibase = VerificationMethod {
            id: "#atproto".to_string(),
            r#type: "Multikey".to_string(),
            controller: "did:plc:subject".to_string(),
            public_key_multibase: None,
        };
        assert!(signing_did_key_from_doc(&doc(vec![no_multibase], vec![])).is_err());

        let bad_multibase = VerificationMethod {
            id: "#atproto".to_string(),
            r#type: "Multikey".to_string(),
            controller: "did:plc:subject".to_string(),
            public_key_multibase: Some("!!!".to_string()),
        };
        assert!(signing_did_key_from_doc(&doc(vec![bad_multibase], vec![])).is_err());

        let unknown_type = VerificationMethod {
            id: "#atproto".to_string(),
            r#type: "Ed25519VerificationKey2020".to_string(),
            controller: "did:plc:subject".to_string(),
            public_key_multibase: Some("zunknown".to_string()),
        };
        assert!(signing_did_key_from_doc(&doc(vec![unknown_type], vec![])).is_err());
    }

    #[test]
    fn service_endpoint_lookup() {
        let svc = Service {
            id: "did:plc:subject#managing_app".to_string(),
            r#type: "ManagingApp".to_string(),
            service_endpoint: "https://app.example.com".to_string(),
        };
        let d = doc(vec![], vec![svc]);
        assert_eq!(
            service_endpoint_from_doc(&d, "managing_app").unwrap(),
            "https://app.example.com"
        );
        assert!(service_endpoint_from_doc(&d, "missing").is_err());
        assert!(service_endpoint_from_doc(&doc(vec![], vec![]), "managing_app").is_err());
    }

    struct FixedDoc(DidDocument);
    #[async_trait]
    impl DocSource for FixedDoc {
        async fn did_document(&self, _did: &str) -> Result<DidDocument> {
            Ok(self.0.clone())
        }
    }

    #[tokio::test]
    async fn doc_key_resolver_resolves_signing_key() {
        let (m, key) = multikey_method("#atproto", [0x44; 32]);
        let resolver = DocKeyResolver::new(Arc::new(FixedDoc(doc(vec![m], vec![]))));
        assert_eq!(resolver.signing_key("did:plc:subject").await.unwrap(), key);
    }

    #[tokio::test]
    async fn resolver_doc_source_fetches_via_did_web() {
        use rsky_identity::types::{DidResolverOpts, MemoryCache};
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let port = server.address().port();
        // did:web percent-encodes the port; rsky-identity rewrites localhost to http.
        let did = format!("did:web:localhost%3A{port}");
        let (m, key) = multikey_method("#atproto", [0x45; 32]);
        let doc_json = serde_json::json!({
            "id": did,
            "verificationMethod": [{
                "id": m.id,
                "type": m.r#type,
                "controller": did,
                "publicKeyMultibase": m.public_key_multibase,
            }],
        });
        Mock::given(method("GET"))
            .and(path("/.well-known/did.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(doc_json))
            .mount(&server)
            .await;

        let source = ResolverDocSource::new(DidResolver::new(DidResolverOpts {
            timeout: None,
            plc_url: None,
            did_cache: Arc::new(MemoryCache::new(None, None)),
        }));
        let got = source.did_document(&did).await.unwrap();
        assert_eq!(signing_did_key_from_doc(&got).unwrap(), key);

        // Unresolvable DIDs surface as resolution errors.
        let missing = source.did_document("did:web:localhost%3A1").await;
        assert!(matches!(missing, Err(HostError::Resolution(_))));
    }
}
