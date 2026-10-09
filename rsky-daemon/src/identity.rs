//! DID-document lookups: an account's commit signing key, and the PDS that
//! hosts its repos (and, for a space authority, its spaces).

use rsky_identity::did::atproto_data::{get_did_key_from_multibase, VerificationMaterial};
use rsky_identity::types::{DidDocument, IdentityResolverOpts, MemoryCache};
use rsky_identity::IdResolver;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Instant;

use crate::engine::CommitKeyResolver;
use crate::error::{DaemonError, Result};

/// How long a resolved PDS endpoint is trusted before the DID document is
/// fetched again, so an account migration is picked up within this window.
pub const PDS_ENDPOINT_TTL: Duration = Duration::from_secs(300);

pub struct DidResolver {
    resolver: Mutex<IdResolver>,
    pds_ttl: Duration,
    pds: Mutex<HashMap<String, (Option<String>, Instant)>>,
}

impl DidResolver {
    pub fn new(plc_url: Option<String>, pds_ttl: Duration) -> Self {
        Self {
            resolver: Mutex::new(IdResolver::new(IdentityResolverOpts {
                timeout: None,
                plc_url,
                did_cache: Some(Arc::new(MemoryCache::new(None, None))),
                backup_nameservers: None,
            })),
            pds_ttl,
            pds: Mutex::new(HashMap::new()),
        }
    }

    async fn document(&self, did: &str, force_refresh: bool) -> Result<DidDocument> {
        self.resolver
            .lock()
            .await
            .did
            .ensure_resolve(&did.to_string(), Some(force_refresh))
            .await
            .map_err(|e| DaemonError::KeyResolution(e.to_string()))
    }

    /// The account's `#atproto_pds` endpoint, or `None` when the document
    /// names none. A failed lookup keeps serving the last known endpoint.
    pub async fn pds_endpoint(&self, did: &str) -> Option<String> {
        let now = Instant::now();
        let previous = match self.pds.lock().await.get(did) {
            Some((endpoint, at)) if now.duration_since(*at) < self.pds_ttl => {
                return endpoint.clone()
            }
            Some((endpoint, _)) => endpoint.clone(),
            None => None,
        };
        let endpoint = match self.document(did, true).await {
            Ok(doc) => pds_from_document(did, &doc),
            Err(error) => {
                tracing::warn!(did, error = %error, "PDS endpoint resolution failed");
                previous
            }
        };
        self.pds
            .lock()
            .await
            .insert(did.to_string(), (endpoint.clone(), now));
        endpoint
    }
}

fn pds_from_document(did: &str, doc: &DidDocument) -> Option<String> {
    let service = doc.service.as_ref()?.iter().find(|s| {
        (s.id == "#atproto_pds" || s.id == format!("{did}#atproto_pds"))
            && s.r#type == "AtprotoPersonalDataServer"
    })?;
    let url = reqwest::Url::parse(&service.service_endpoint).ok()?;
    // A member's document decides where the space credential is sent, so it
    // must not be able to point the daemon at a plain-HTTP internal address.
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1"));
    (url.scheme() == "https" || (url.scheme() == "http" && loopback))
        .then(|| service.service_endpoint.trim_end_matches('/').to_string())
}

#[async_trait::async_trait]
impl CommitKeyResolver for DidResolver {
    async fn signing_key(&self, did: &str) -> Result<String> {
        let doc = self.document(did, false).await?;
        let method = doc
            .verification_method
            .unwrap_or_default()
            .into_iter()
            .find(|m| m.id == format!("{did}#atproto") || m.id == "#atproto")
            .ok_or_else(|| DaemonError::KeyResolution(format!("no #atproto key for {did}")))?;
        let multibase = method.public_key_multibase.ok_or_else(|| {
            DaemonError::KeyResolution(format!("no publicKeyMultibase for {did}"))
        })?;
        get_did_key_from_multibase(VerificationMaterial {
            r#type: method.r#type,
            public_key_multibase: multibase,
        })
        .map_err(|e| DaemonError::KeyResolution(e.to_string()))?
        .ok_or_else(|| DaemonError::KeyResolution(format!("unsupported key type for {did}")))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use secp256k1::{PublicKey, Secp256k1, SecretKey};
    use wiremock::matchers::{method, path_regex};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    pub(crate) fn doc(did: &str, pds: Option<&str>) -> serde_json::Value {
        let service = pds.map(|endpoint| {
            vec![serde_json::json!({
                "id": "#atproto_pds",
                "type": "AtprotoPersonalDataServer",
                "serviceEndpoint": endpoint,
            })]
        });
        serde_json::json!({"id": did, "service": service})
    }

    pub(crate) async fn plc_serving(docs: &[(&str, Option<&str>)]) -> MockServer {
        let plc = MockServer::start().await;
        for (did, pds) in docs {
            Mock::given(method("GET"))
                .and(path_regex(format!("{}$", did.rsplit(':').next().unwrap())))
                .respond_with(ResponseTemplate::new(200).set_body_json(doc(did, *pds)))
                .mount(&plc)
                .await;
        }
        plc
    }

    fn resolver(plc: &MockServer, ttl: Duration) -> DidResolver {
        DidResolver::new(Some(plc.uri()), ttl)
    }

    #[tokio::test]
    async fn the_pds_endpoint_comes_from_the_document() {
        let plc = plc_serving(&[
            ("did:plc:alice", Some("https://pds.one/")),
            ("did:plc:bob", None),
        ])
        .await;
        let resolver = resolver(&plc, PDS_ENDPOINT_TTL);
        assert_eq!(
            resolver.pds_endpoint("did:plc:alice").await.as_deref(),
            Some("https://pds.one")
        );
        assert_eq!(resolver.pds_endpoint("did:plc:bob").await, None);
        assert_eq!(resolver.pds_endpoint("did:plc:nobody").await, None);
    }

    #[test]
    fn only_https_or_loopback_endpoints_are_used() {
        let endpoint = |url: &str| {
            let doc: DidDocument = serde_json::from_value(doc("did:plc:a", Some(url))).unwrap();
            pds_from_document("did:plc:a", &doc)
        };
        assert_eq!(
            endpoint("https://pds.one").as_deref(),
            Some("https://pds.one")
        );
        assert_eq!(
            endpoint("http://127.0.0.1:2583").as_deref(),
            Some("http://127.0.0.1:2583")
        );
        assert_eq!(
            endpoint("http://localhost:2583").as_deref(),
            Some("http://localhost:2583")
        );
        assert_eq!(endpoint("http://10.0.0.7"), None);
        assert_eq!(endpoint("not a url"), None);

        let qualified: DidDocument = serde_json::from_value(serde_json::json!({
            "id": "did:plc:a",
            "service": [
                {"id": "#atproto_labeler", "type": "AtprotoLabeler", "serviceEndpoint": "https://l"},
                {"id": "#atproto_pds", "type": "Other", "serviceEndpoint": "https://wrong"},
                {"id": "did:plc:a#atproto_pds", "type": "AtprotoPersonalDataServer", "serviceEndpoint": "https://pds.two"}
            ]
        }))
        .unwrap();
        assert_eq!(
            pds_from_document("did:plc:a", &qualified).as_deref(),
            Some("https://pds.two")
        );
    }

    #[tokio::test]
    async fn an_endpoint_is_cached_for_the_ttl() {
        let plc = plc_serving(&[("did:plc:alice", Some("https://pds.one"))]).await;
        let cached = resolver(&plc, PDS_ENDPOINT_TTL);
        cached.pds_endpoint("did:plc:alice").await;
        cached.pds_endpoint("did:plc:alice").await;
        assert_eq!(plc.received_requests().await.unwrap().len(), 1);

        let expired = resolver(&plc, Duration::ZERO);
        expired.pds_endpoint("did:plc:alice").await;
        expired.pds_endpoint("did:plc:alice").await;
        assert_eq!(plc.received_requests().await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn a_failed_refresh_keeps_the_last_known_endpoint() {
        let plc = plc_serving(&[("did:plc:alice", Some("https://pds.one"))]).await;
        let resolver = resolver(&plc, Duration::ZERO);
        assert!(resolver.pds_endpoint("did:plc:alice").await.is_some());
        plc.reset().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&plc)
            .await;
        assert_eq!(
            resolver.pds_endpoint("did:plc:alice").await.as_deref(),
            Some("https://pds.one")
        );
        assert_eq!(resolver.pds_endpoint("did:plc:carol").await, None);
    }

    #[tokio::test]
    async fn the_signing_key_comes_from_the_atproto_verification_method() {
        let pubkey = PublicKey::from_secret_key(
            &Secp256k1::new(),
            &SecretKey::from_slice(&[0x33u8; 32]).unwrap(),
        );
        let did_key = rsky_crypto::utils::encode_did_key(&pubkey);
        let multibase = did_key.trim_start_matches("did:key:");
        let method_doc = |did: &str, id: &str, kind: &str, key: Option<&str>| {
            serde_json::json!({
                "id": did,
                "verificationMethod": [{
                    "id": id, "type": kind, "controller": did, "publicKeyMultibase": key
                }]
            })
        };
        let plc = MockServer::start().await;
        for (suffix, body) in [
            (
                "good",
                method_doc("did:plc:good", "#atproto", "Multikey", Some(multibase)),
            ),
            (
                "nokey",
                method_doc("did:plc:nokey", "#other", "Multikey", Some(multibase)),
            ),
            (
                "nomb",
                method_doc("did:plc:nomb", "did:plc:nomb#atproto", "Multikey", None),
            ),
            (
                "odd",
                method_doc("did:plc:odd", "#atproto", "Unknown", Some(multibase)),
            ),
            (
                "bad",
                method_doc("did:plc:bad", "#atproto", "Multikey", Some("!")),
            ),
        ] {
            Mock::given(method("GET"))
                .and(path_regex(format!("{suffix}$")))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .mount(&plc)
                .await;
        }
        let resolver = resolver(&plc, PDS_ENDPOINT_TTL);
        assert_eq!(resolver.signing_key("did:plc:good").await.unwrap(), did_key);
        for did in [
            "did:plc:nokey",
            "did:plc:nomb",
            "did:plc:odd",
            "did:plc:bad",
            "did:plc:missing",
        ] {
            assert!(matches!(
                resolver.signing_key(did).await,
                Err(DaemonError::KeyResolution(_))
            ));
        }
    }
}
