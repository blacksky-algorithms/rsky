//! DID-document lookups: an account's commit signing key, the PDS that
//! hosts its repos, and, for a space authority, its space host.

use rsky_identity::did::atproto_data::{get_did_key_from_multibase, VerificationMaterial};
use rsky_identity::types::{DidDocument, IdentityResolverOpts, MemoryCache};
use rsky_identity::IdResolver;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::Instant;

use crate::engine::CommitKeyResolver;
use crate::error::{DaemonError, Result};

/// How long resolved endpoints are trusted before the DID document is
/// fetched again, so an account migration is picked up within this window.
pub const PDS_ENDPOINT_TTL: Duration = Duration::from_secs(300);

/// How long a failed lookup is remembered before it is retried.
pub const PDS_FAILURE_TTL: Duration = Duration::from_secs(30);

/// How long the last resolved endpoints keep serving while the document
/// cannot be fetched; after this a lookup fails instead.
pub const LAST_KNOWN_LIMIT: Duration = Duration::from_secs(3600);

#[derive(Debug, Clone, PartialEq, Eq)]
enum Endpoint {
    Named(String),
    Unnamed,
    Unusable,
}

impl Endpoint {
    fn into_url(self, did: &str, service: &str) -> Result<Option<String>> {
        match self {
            Self::Named(url) => Ok(Some(url)),
            Self::Unnamed => Ok(None),
            Self::Unusable => Err(DaemonError::KeyResolution(format!(
                "unusable {service} endpoint in the DID document for {did}"
            ))),
        }
    }
}

#[derive(Debug, Clone)]
struct Hosts {
    pds: Endpoint,
    space_host: Endpoint,
}

struct Entry {
    last: Option<(Hosts, Instant)>,
    expires: Instant,
}

pub struct DidResolver {
    resolver: IdResolver,
    ttl: Duration,
    allow_http_loopback: bool,
    hosts: Mutex<HashMap<String, Entry>>,
}

impl DidResolver {
    pub fn new(plc_url: Option<String>, ttl: Duration, allow_http_loopback: bool) -> Self {
        Self {
            resolver: IdResolver::new(IdentityResolverOpts {
                timeout: None,
                plc_url,
                did_cache: Some(Arc::new(MemoryCache::new(None, None))),
                backup_nameservers: None,
            }),
            ttl,
            allow_http_loopback,
            hosts: Mutex::new(HashMap::new()),
        }
    }

    async fn document(&self, did: &str, force_refresh: bool) -> Result<DidDocument> {
        self.resolver
            .did
            .ensure_resolve(&did.to_string(), Some(force_refresh))
            .await
            .map_err(|e| DaemonError::KeyResolution(e.to_string()))
    }

    async fn hosts(&self, did: &str) -> Result<Hosts> {
        let now = Instant::now();
        let last = match self.hosts.lock().unwrap().get(did) {
            Some(entry) if now < entry.expires => return serve(did, entry.last.clone(), now),
            Some(entry) => entry.last.clone(),
            None => None,
        };
        let (last, ttl) = match self.document(did, true).await {
            Ok(doc) => (Some((self.hosts_from(did, &doc), now)), self.ttl),
            Err(error) => {
                tracing::warn!(did, error = %error, "DID document resolution failed");
                (last, PDS_FAILURE_TTL.min(self.ttl))
            }
        };
        self.hosts.lock().unwrap().insert(
            did.to_string(),
            Entry {
                last: last.clone(),
                expires: now + ttl,
            },
        );
        serve(did, last, now)
    }

    fn hosts_from(&self, did: &str, doc: &DidDocument) -> Hosts {
        let find = |fragment, kind| endpoint(did, doc, fragment, kind, self.allow_http_loopback);
        // Matches how Auth and the PDS pick a space's host, so all three agree.
        let space_host = match find("atproto_space_host", None) {
            Endpoint::Unnamed => find("atproto_pds", None),
            named => named,
        };
        Hosts {
            pds: find("atproto_pds", Some("AtprotoPersonalDataServer")),
            space_host,
        }
    }

    /// The account's `#atproto_pds` endpoint, `None` when its document names
    /// none, or an error when the document cannot be resolved.
    pub async fn pds_endpoint(&self, did: &str) -> Result<Option<String>> {
        self.hosts(did).await?.pds.into_url(did, "#atproto_pds")
    }

    /// The space authority's `#atproto_space_host`, else its `#atproto_pds`.
    pub async fn space_host_endpoint(&self, did: &str) -> Result<Option<String>> {
        self.hosts(did)
            .await?
            .space_host
            .into_url(did, "space host")
    }
}

fn serve(did: &str, last: Option<(Hosts, Instant)>, now: Instant) -> Result<Hosts> {
    match last {
        Some((hosts, at)) if now.duration_since(at) < LAST_KNOWN_LIMIT => Ok(hosts),
        _ => Err(DaemonError::KeyResolution(format!(
            "the DID document for {did} could not be resolved"
        ))),
    }
}

fn endpoint(
    did: &str,
    doc: &DidDocument,
    fragment: &str,
    kind: Option<&str>,
    allow_http_loopback: bool,
) -> Endpoint {
    let Some(service) = doc.service.iter().flatten().find(|s| {
        (s.id == format!("#{fragment}") || s.id == format!("{did}#{fragment}"))
            && kind.is_none_or(|kind| s.r#type == kind)
    }) else {
        return Endpoint::Unnamed;
    };
    let Ok(url) = reqwest::Url::parse(&service.service_endpoint) else {
        return Endpoint::Unusable;
    };
    // The document decides where a credential is sent, so it must not be
    // able to point the daemon at a plain-HTTP internal address.
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1"));
    if url.scheme() == "https" || (allow_http_loopback && url.scheme() == "http" && loopback) {
        Endpoint::Named(service.service_endpoint.trim_end_matches('/').to_string())
    } else {
        Endpoint::Unusable
    }
}

/// Where an account's repos, and a space authority's spaces, are hosted.
/// `Ok(None)` means the document names no host; an error means it could not
/// be resolved, and nothing should be sent anywhere for that DID.
#[async_trait::async_trait]
pub trait PdsResolver: Send + Sync {
    async fn pds_url(&self, did: &str) -> Result<Option<String>>;
    async fn space_host_url(&self, did: &str) -> Result<Option<String>>;
}

#[async_trait::async_trait]
impl PdsResolver for DidResolver {
    async fn pds_url(&self, did: &str) -> Result<Option<String>> {
        self.pds_endpoint(did).await
    }

    async fn space_host_url(&self, did: &str) -> Result<Option<String>> {
        self.space_host_endpoint(did).await
    }
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
        DidResolver::new(Some(plc.uri()), ttl, true)
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
            resolver.pds_url("did:plc:alice").await.unwrap().as_deref(),
            Some("https://pds.one")
        );
        assert_eq!(resolver.pds_url("did:plc:bob").await.unwrap(), None);
        assert!(resolver.pds_url("did:plc:nobody").await.is_err());
    }

    #[test]
    fn only_https_or_opted_in_loopback_endpoints_are_used() {
        let endpoint_in = |url: &str, allow| {
            let doc: DidDocument = serde_json::from_value(doc("did:plc:a", Some(url))).unwrap();
            endpoint(
                "did:plc:a",
                &doc,
                "atproto_pds",
                Some("AtprotoPersonalDataServer"),
                allow,
            )
        };
        let named = |url: &str| Endpoint::Named(url.to_string());
        assert_eq!(
            endpoint_in("https://pds.one", false),
            named("https://pds.one")
        );
        assert_eq!(
            endpoint_in("http://127.0.0.1:2583", false),
            Endpoint::Unusable
        );
        assert_eq!(
            endpoint_in("http://localhost:2583", false),
            Endpoint::Unusable
        );
        assert_eq!(
            endpoint_in("http://127.0.0.1:2583", true),
            named("http://127.0.0.1:2583")
        );
        assert_eq!(
            endpoint_in("http://localhost:2583", true),
            named("http://localhost:2583")
        );
        assert_eq!(endpoint_in("http://10.0.0.7", true), Endpoint::Unusable);
        assert_eq!(endpoint_in("not a url", true), Endpoint::Unusable);
        assert!(Endpoint::Unusable.into_url("did:plc:a", "pds").is_err());
    }

    #[tokio::test]
    async fn the_space_host_prefers_atproto_space_host_then_any_atproto_pds() {
        let plc = MockServer::start().await;
        let service = |id: &str, kind: &str, endpoint: &str| serde_json::json!({"id": id, "type": kind, "serviceEndpoint": endpoint});
        for (suffix, services) in [
            (
                "both",
                vec![
                    service(
                        "#atproto_pds",
                        "AtprotoPersonalDataServer",
                        "https://pds.one",
                    ),
                    service(
                        "did:plc:both#atproto_space_host",
                        "Any",
                        "https://spaces.one/",
                    ),
                ],
            ),
            (
                "pdsonly",
                vec![service("#atproto_pds", "Other", "https://pds.two")],
            ),
            (
                "none",
                vec![service("#atproto_labeler", "AtprotoLabeler", "https://l")],
            ),
            (
                "bad",
                vec![service("#atproto_space_host", "Any", "http://10.0.0.7")],
            ),
        ] {
            Mock::given(method("GET"))
                .and(path_regex(format!("{suffix}$")))
                .respond_with(ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({"id": format!("did:plc:{suffix}"), "service": services}),
                ))
                .mount(&plc)
                .await;
        }
        let resolver = resolver(&plc, PDS_ENDPOINT_TTL);
        let space_host = |did: &'static str| resolver.space_host_url(did);
        assert_eq!(
            space_host("did:plc:both").await.unwrap().as_deref(),
            Some("https://spaces.one")
        );
        assert_eq!(
            resolver.pds_url("did:plc:both").await.unwrap().as_deref(),
            Some("https://pds.one")
        );
        assert_eq!(
            space_host("did:plc:pdsonly").await.unwrap().as_deref(),
            Some("https://pds.two")
        );
        assert_eq!(resolver.pds_url("did:plc:pdsonly").await.unwrap(), None);
        assert_eq!(space_host("did:plc:none").await.unwrap(), None);
        assert!(space_host("did:plc:bad").await.is_err());
    }

    #[tokio::test]
    async fn an_endpoint_is_cached_for_the_ttl() {
        let plc = plc_serving(&[("did:plc:alice", Some("https://pds.one"))]).await;
        let cached = resolver(&plc, PDS_ENDPOINT_TTL);
        cached.pds_url("did:plc:alice").await.unwrap();
        cached.space_host_url("did:plc:alice").await.unwrap();
        assert_eq!(plc.received_requests().await.unwrap().len(), 1);

        let expired = resolver(&plc, Duration::ZERO);
        expired.pds_url("did:plc:alice").await.unwrap();
        expired.pds_url("did:plc:alice").await.unwrap();
        assert_eq!(plc.received_requests().await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn a_failed_refresh_keeps_the_last_known_endpoint_for_a_bounded_time() {
        let plc = plc_serving(&[("did:plc:alice", Some("https://pds.one"))]).await;
        let resolver = resolver(&plc, Duration::ZERO);
        assert!(resolver.pds_url("did:plc:alice").await.unwrap().is_some());
        plc.reset().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&plc)
            .await;
        assert_eq!(
            resolver.pds_url("did:plc:alice").await.unwrap().as_deref(),
            Some("https://pds.one")
        );
        assert!(resolver.pds_url("did:plc:carol").await.is_err());

        let long_ago = Instant::now()
            .checked_sub(LAST_KNOWN_LIMIT)
            .expect("the clock is past the keep-last-known window");
        if let Some(entry) = resolver.hosts.lock().unwrap().get_mut("did:plc:alice") {
            entry.last.as_mut().unwrap().1 = long_ago;
        }
        assert!(resolver.pds_url("did:plc:alice").await.is_err());
    }

    #[tokio::test]
    async fn a_failed_first_lookup_errors_until_the_failure_ttl() {
        let plc = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&plc)
            .await;
        let resolver = resolver(&plc, PDS_ENDPOINT_TTL);
        assert!(resolver.pds_url("did:plc:alice").await.is_err());
        assert!(resolver.space_host_url("did:plc:alice").await.is_err());
        assert_eq!(plc.received_requests().await.unwrap().len(), 1);
        let expires = resolver.hosts.lock().unwrap()["did:plc:alice"].expires;
        assert!(expires <= Instant::now() + PDS_FAILURE_TTL);
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
