//! Client attestation verification (spec §Client attestation).
//!
//! An attestation is a `private_key_jwt`-shaped assertion signed by the
//! application's own client authentication key. The authority verifies it by
//! resolving `iss` (the `client_id`) to the client's published metadata,
//! fetching its JWKS, and checking the signature against the key named by the
//! attestation's `kid`. `jti` is single-use.

use async_trait::async_trait;
use rsky_space::credential::{self, ATTESTATION_TYP};
use rsky_space::jwk::{verify_es256, JwkSet};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use crate::error::{HostError, Result};
use crate::managing_app::require_https;

/// Maximum tolerated clock skew for a future `iat`.
pub const MAX_IAT_SKEW_SECS: u64 = 60;

/// Published OAuth client metadata, reduced to the attestation-relevant fields.
#[derive(Debug, Clone, Deserialize)]
pub struct ClientMetadata {
    pub client_id: String,
    #[serde(default)]
    pub jwks: Option<JwkSet>,
    #[serde(default)]
    pub jwks_uri: Option<String>,
}

/// Fetches client metadata and JWKS documents.
#[async_trait]
pub trait MetadataFetcher: Send + Sync {
    async fn client_metadata(&self, client_id: &str) -> Result<ClientMetadata>;
    async fn jwks(&self, url: &str) -> Result<JwkSet>;
}

/// Replay protection: `consume` returns true when `jti` was unseen (and records
/// it until `exp`), false when it was already used.
#[async_trait]
pub trait JtiStore: Send + Sync {
    async fn consume(&self, jti: &str, exp: u64) -> Result<bool>;
}

/// Entries expiring this long before an incoming token's `exp` are certainly
/// past their own expiry (attestations are short-lived) and safe to purge.
pub(crate) const JTI_PURGE_GRACE_SECS: u64 = 3600;

#[derive(Default)]
pub struct InMemoryJtiStore {
    seen: Mutex<HashMap<String, u64>>,
}

#[async_trait]
impl JtiStore for InMemoryJtiStore {
    async fn consume(&self, jti: &str, exp: u64) -> Result<bool> {
        let mut seen = self.seen.lock().unwrap();
        seen.retain(|_, e| e.saturating_add(JTI_PURGE_GRACE_SECS) > exp);
        Ok(seen.insert(jti.to_string(), exp).is_none())
    }
}

/// Verify a client attestation end-to-end and return the attested `client_id`.
pub async fn verify_client_attestation(
    jwt: &str,
    authority_did: &str,
    fetcher: &dyn MetadataFetcher,
    jti_store: &dyn JtiStore,
    now: u64,
) -> Result<String> {
    let decoded = credential::decode(jwt).map_err(|e| HostError::Attestation(e.to_string()))?;
    if decoded.header.typ != ATTESTATION_TYP {
        return Err(HostError::Attestation(format!(
            "typ {} != {ATTESTATION_TYP}",
            decoded.header.typ
        )));
    }
    if decoded.header.alg != "ES256" {
        return Err(HostError::Attestation(format!(
            "unsupported alg {}",
            decoded.header.alg
        )));
    }
    let kid = decoded
        .header
        .kid
        .as_deref()
        .ok_or_else(|| HostError::Attestation("missing kid".into()))?;
    let claims = &decoded.claims;
    if claims.iss != claims.sub {
        return Err(HostError::Attestation("iss != sub".into()));
    }
    if !claims.iss.starts_with("https://") {
        return Err(HostError::Attestation(
            "client_id is not an https url".into(),
        ));
    }
    let want_aud = format!("{authority_did}#atproto_space_host");
    if claims.aud.as_deref() != Some(want_aud.as_str()) {
        return Err(HostError::Attestation("aud != space host".into()));
    }
    if now >= claims.exp {
        return Err(HostError::Attestation("attestation expired".into()));
    }
    if claims.iat > now + MAX_IAT_SKEW_SECS {
        return Err(HostError::Attestation("iat too far in the future".into()));
    }

    let metadata = fetcher.client_metadata(&claims.iss).await?;
    if metadata.client_id != claims.iss {
        return Err(HostError::Attestation(
            "metadata client_id does not match iss".into(),
        ));
    }
    let jwks = match (metadata.jwks, metadata.jwks_uri) {
        (Some(jwks), _) => jwks,
        (None, Some(url)) => fetcher.jwks(&url).await?,
        (None, None) => {
            return Err(HostError::Attestation("client publishes no jwks".into()));
        }
    };
    let jwk = jwks
        .find(kid)
        .ok_or_else(|| HostError::Attestation(format!("no jwk with kid {kid}")))?;
    verify_es256(jwk, &decoded.signing_input, &decoded.signature)
        .map_err(|e| HostError::Attestation(e.to_string()))?;

    // Consume the nonce only after the signature proves the attestation genuine.
    if !jti_store.consume(&claims.jti, claims.exp).await? {
        return Err(HostError::Attestation("jti replayed".into()));
    }
    Ok(claims.iss.clone())
}

const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// HTTPS [`MetadataFetcher`] with a response-size cap.
pub struct HttpMetadataFetcher {
    http: reqwest::Client,
}

impl Default for HttpMetadataFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpMetadataFetcher {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .timeout(FETCH_TIMEOUT)
            .build()
            .expect("reqwest client");
        Self { http }
    }

    async fn fetch_capped(&self, url: &str) -> Result<Vec<u8>> {
        require_https(url).map_err(|_| HostError::Attestation(format!("url not https: {url}")))?;
        let mut response = self.http.get(url).send().await.map_err(transport_err)?;
        if !response.status().is_success() {
            return Err(HostError::Attestation(format!(
                "{url} returned {}",
                response.status()
            )));
        }
        let mut buf = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(transport_err)? {
            if buf.len() + chunk.len() > MAX_RESPONSE_BYTES {
                return Err(HostError::Attestation(format!("{url} response too large")));
            }
            buf.extend_from_slice(&chunk);
        }
        Ok(buf)
    }
}

fn transport_err(e: reqwest::Error) -> HostError {
    HostError::Attestation(e.to_string())
}

#[async_trait]
impl MetadataFetcher for HttpMetadataFetcher {
    async fn client_metadata(&self, client_id: &str) -> Result<ClientMetadata> {
        let bytes = self.fetch_capped(client_id).await?;
        serde_json::from_slice(&bytes)
            .map_err(|e| HostError::Attestation(format!("bad client metadata: {e}")))
    }

    async fn jwks(&self, url: &str) -> Result<JwkSet> {
        let bytes = self.fetch_capped(url).await?;
        serde_json::from_slice(&bytes).map_err(|e| HostError::Attestation(format!("bad jwks: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use p256::ecdsa::signature::hazmat::PrehashSigner;
    use p256::ecdsa::{Signature, SigningKey};
    use rsky_space::credential::{encode, JwtHeader, SpaceClaims};
    use rsky_space::jwk::EcJwk;
    use sha2::{Digest, Sha256};

    const AUTHORITY: &str = "did:plc:auth";
    const CLIENT_ID: &str = "https://app.example.com/client-metadata.json";

    fn client_key() -> SigningKey {
        SigningKey::from_slice(&[0x71u8; 32]).unwrap()
    }

    fn client_jwk(kid: &str) -> EcJwk {
        let point = client_key().verifying_key().to_encoded_point(false);
        let bytes = point.as_bytes();
        EcJwk {
            kty: "EC".to_string(),
            crv: "P-256".to_string(),
            x: URL_SAFE_NO_PAD.encode(&bytes[1..33]),
            y: URL_SAFE_NO_PAD.encode(&bytes[33..65]),
            kid: Some(kid.to_string()),
        }
    }

    struct Attestation {
        typ: String,
        alg: String,
        kid: Option<String>,
        iss: String,
        sub: String,
        aud: Option<String>,
        iat: u64,
        exp: u64,
        jti: String,
    }

    impl Default for Attestation {
        fn default() -> Self {
            Self {
                typ: ATTESTATION_TYP.to_string(),
                alg: "ES256".to_string(),
                kid: Some("key-1".to_string()),
                iss: CLIENT_ID.to_string(),
                sub: CLIENT_ID.to_string(),
                aud: Some(format!("{AUTHORITY}#atproto_space_host")),
                iat: 1000,
                exp: 1060,
                jti: "attest-jti".to_string(),
            }
        }
    }

    impl Attestation {
        fn jwt(self) -> String {
            let header = JwtHeader {
                typ: self.typ,
                alg: self.alg,
                kid: self.kid,
            };
            let claims = SpaceClaims {
                iss: self.iss,
                sub: self.sub,
                aud: self.aud,
                iat: self.iat,
                exp: self.exp,
                jti: self.jti,
            };
            encode(&header, &claims, |input| {
                let digest = Sha256::digest(input);
                let sig: Signature = client_key().sign_prehash(&digest).expect("p256 signs");
                let sig = sig.normalize_s().unwrap_or(sig);
                Ok(sig.to_vec())
            })
            .unwrap()
        }
    }

    struct MockFetcher {
        metadata: ClientMetadata,
        jwks: Option<JwkSet>,
    }

    impl MockFetcher {
        fn inline() -> Self {
            Self {
                metadata: ClientMetadata {
                    client_id: CLIENT_ID.to_string(),
                    jwks: Some(JwkSet {
                        keys: vec![client_jwk("key-1")],
                    }),
                    jwks_uri: None,
                },
                jwks: None,
            }
        }
    }

    #[async_trait]
    impl MetadataFetcher for MockFetcher {
        async fn client_metadata(&self, _client_id: &str) -> Result<ClientMetadata> {
            Ok(self.metadata.clone())
        }
        async fn jwks(&self, _url: &str) -> Result<JwkSet> {
            self.jwks
                .clone()
                .ok_or_else(|| HostError::Attestation("no jwks served".into()))
        }
    }

    async fn verify(jwt: &str, fetcher: &MockFetcher) -> Result<String> {
        let store = InMemoryJtiStore::default();
        verify_client_attestation(jwt, AUTHORITY, fetcher, &store, 1030).await
    }

    #[tokio::test]
    async fn valid_attestation_yields_client_id() {
        let got = verify(&Attestation::default().jwt(), &MockFetcher::inline())
            .await
            .unwrap();
        assert_eq!(got, CLIENT_ID);
    }

    #[tokio::test]
    async fn jwks_uri_path_is_used_when_no_inline_jwks() {
        let fetcher = MockFetcher {
            metadata: ClientMetadata {
                client_id: CLIENT_ID.to_string(),
                jwks: None,
                jwks_uri: Some("https://app.example.com/jwks.json".to_string()),
            },
            jwks: Some(JwkSet {
                keys: vec![client_jwk("key-1")],
            }),
        };
        assert_eq!(
            verify(&Attestation::default().jwt(), &fetcher)
                .await
                .unwrap(),
            CLIENT_ID
        );

        let no_jwks_anywhere = MockFetcher {
            metadata: ClientMetadata {
                client_id: CLIENT_ID.to_string(),
                jwks: None,
                jwks_uri: None,
            },
            jwks: None,
        };
        let err = verify(&Attestation::default().jwt(), &no_jwks_anywhere).await;
        assert!(matches!(err, Err(HostError::Attestation(m)) if m.contains("no jwks")));

        // A jwks_uri that fails to fetch propagates the fetch error.
        let broken_jwks_uri = MockFetcher {
            metadata: ClientMetadata {
                client_id: CLIENT_ID.to_string(),
                jwks: None,
                jwks_uri: Some("https://app.example.com/jwks.json".to_string()),
            },
            jwks: None,
        };
        let err = verify(&Attestation::default().jwt(), &broken_jwks_uri).await;
        assert!(matches!(err, Err(HostError::Attestation(m)) if m.contains("no jwks served")));
    }

    #[tokio::test]
    async fn header_and_claim_deviations_rejected() {
        let cases: Vec<(Attestation, &str)> = vec![
            (
                Attestation {
                    typ: "JWT".to_string(),
                    ..Default::default()
                },
                "typ",
            ),
            (
                Attestation {
                    alg: "ES256K".to_string(),
                    ..Default::default()
                },
                "alg",
            ),
            (
                Attestation {
                    kid: None,
                    ..Default::default()
                },
                "kid",
            ),
            (
                Attestation {
                    sub: "https://other.example.com/client".to_string(),
                    ..Default::default()
                },
                "iss != sub",
            ),
            (
                Attestation {
                    iss: "http://app.example.com/client".to_string(),
                    sub: "http://app.example.com/client".to_string(),
                    ..Default::default()
                },
                "https",
            ),
            (
                Attestation {
                    aud: Some("did:plc:other#atproto_space_host".to_string()),
                    ..Default::default()
                },
                "aud",
            ),
            (
                Attestation {
                    aud: None,
                    ..Default::default()
                },
                "aud",
            ),
            (
                Attestation {
                    exp: 1030,
                    ..Default::default()
                },
                "expired",
            ),
            (
                Attestation {
                    iat: 2000,
                    exp: 2060,
                    ..Default::default()
                },
                "iat",
            ),
        ];
        for (attestation, needle) in cases {
            let msg = verify(&attestation.jwt(), &MockFetcher::inline())
                .await
                .unwrap_err()
                .to_string();
            assert!(msg.starts_with("client attestation rejected"), "{msg}");
            assert!(msg.contains(needle), "expected {needle} in {msg}");
        }
    }

    #[tokio::test]
    async fn malformed_jwt_rejected() {
        let err = verify("not-a-jwt", &MockFetcher::inline()).await;
        assert!(matches!(err, Err(HostError::Attestation(_))));
    }

    #[tokio::test]
    async fn replayed_jti_rejected() {
        let store = InMemoryJtiStore::default();
        let fetcher = MockFetcher::inline();
        let jwt = Attestation::default().jwt();
        verify_client_attestation(&jwt, AUTHORITY, &fetcher, &store, 1030)
            .await
            .unwrap();
        let replay = verify_client_attestation(&jwt, AUTHORITY, &fetcher, &store, 1030).await;
        assert!(matches!(replay, Err(HostError::Attestation(m)) if m.contains("replayed")));
    }

    #[tokio::test]
    async fn unknown_kid_metadata_mismatch_and_tampered_sig_rejected() {
        let unknown_kid = Attestation {
            kid: Some("key-9".to_string()),
            ..Default::default()
        };
        let err = verify(&unknown_kid.jwt(), &MockFetcher::inline()).await;
        assert!(matches!(err, Err(HostError::Attestation(m)) if m.contains("key-9")));

        let mut fetcher = MockFetcher::inline();
        fetcher.metadata.client_id = "https://imposter.example.com".to_string();
        let err = verify(&Attestation::default().jwt(), &fetcher).await;
        assert!(matches!(err, Err(HostError::Attestation(m)) if m.contains("client_id")));

        let jwt = Attestation::default().jwt();
        let mut parts: Vec<String> = jwt.split('.').map(str::to_string).collect();
        let mut sig = URL_SAFE_NO_PAD.decode(&parts[2]).unwrap();
        sig[0] ^= 0xFF;
        parts[2] = URL_SAFE_NO_PAD.encode(sig);
        let err = verify(&parts.join("."), &MockFetcher::inline()).await;
        assert!(matches!(err, Err(HostError::Attestation(_))));
    }

    #[tokio::test]
    async fn jti_store_purges_expired_entries() {
        let store = InMemoryJtiStore::default();
        assert!(store.consume("a", 100).await.unwrap());
        assert!(!store.consume("a", 100).await.unwrap());
        // A much later token purges "a" (its exp is long past), freeing the slot.
        assert!(store
            .consume("b", 100 + JTI_PURGE_GRACE_SECS)
            .await
            .unwrap());
        assert!(store
            .consume("a", 100 + JTI_PURGE_GRACE_SECS)
            .await
            .unwrap());
    }

    mod http_fetcher {
        use super::*;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        #[tokio::test]
        async fn fetches_metadata_and_jwks() {
            let server = MockServer::start().await;
            let metadata = serde_json::json!({
                "client_id": CLIENT_ID,
                "jwks_uri": format!("{}/jwks.json", server.uri()),
                "unknown_field": {"ignored": true},
            });
            Mock::given(method("GET"))
                .and(path("/client-metadata.json"))
                .respond_with(ResponseTemplate::new(200).set_body_json(metadata))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/jwks.json"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(serde_json::json!({"keys": [client_jwk("key-1")]})),
                )
                .mount(&server)
                .await;

            let fetcher = HttpMetadataFetcher::default();
            let url = format!("{}/client-metadata.json", server.uri());
            let got = fetcher.client_metadata(&url).await.unwrap();
            assert_eq!(got.client_id, CLIENT_ID);
            let jwks = fetcher.jwks(&got.jwks_uri.unwrap()).await.unwrap();
            assert_eq!(jwks.find("key-1"), Some(&client_jwk("key-1")));
        }

        #[tokio::test]
        async fn rejects_http_urls_errors_and_oversized_bodies() {
            let fetcher = HttpMetadataFetcher::new();
            let err = fetcher.client_metadata("http://app.example.com/meta").await;
            assert!(matches!(err, Err(HostError::Attestation(m)) if m.contains("https")));

            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/missing"))
                .respond_with(ResponseTemplate::new(404))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/huge"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_bytes(vec![b'x'; MAX_RESPONSE_BYTES + 1]),
                )
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/notjson"))
                .respond_with(ResponseTemplate::new(200).set_body_string("nope"))
                .mount(&server)
                .await;

            let err = fetcher
                .client_metadata(&format!("{}/missing", server.uri()))
                .await;
            assert!(matches!(err, Err(HostError::Attestation(m)) if m.contains("404")));

            let err = fetcher.jwks(&format!("{}/huge", server.uri())).await;
            assert!(matches!(err, Err(HostError::Attestation(m)) if m.contains("too large")));

            let err = fetcher
                .client_metadata(&format!("{}/notjson", server.uri()))
                .await;
            assert!(matches!(err, Err(HostError::Attestation(m)) if m.contains("metadata")));
            let err = fetcher.jwks(&format!("{}/notjson", server.uri())).await;
            assert!(matches!(err, Err(HostError::Attestation(m)) if m.contains("jwks")));

            // Unreachable host: transport errors surface.
            let err = fetcher.client_metadata("http://127.0.0.1:1/meta").await;
            assert!(matches!(err, Err(HostError::Attestation(_))));
        }
    }
}
