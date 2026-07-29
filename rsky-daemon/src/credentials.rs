//! Space-credential lifecycle (proposal §Credential flow): mint a delegation
//! token from the syncer account's PDS, exchange it with the space authority
//! for a 2h space credential, and refresh before expiry.

use async_trait::async_trait;
use rsky_lexicon::com::atproto::space::GetDelegationTokenOutput;
use rsky_space::credential::{decode, CREDENTIAL_TTL_SECS};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::xrpc::{check, http_client, net_err, SpaceHostClient};

/// Seconds since the Unix epoch; the injectable-`now` boundary for tests.
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock after the epoch")
        .as_secs()
}

/// Mints delegation tokens for the syncer's own account.
#[async_trait]
pub trait DelegationSource: Send + Sync {
    async fn delegation_token(&self, space: &str) -> Result<String>;
}

/// Calls `com.atproto.space.getDelegationToken` on the syncer account's PDS.
pub struct PdsDelegationSource {
    pds_url: String,
    access_token: String,
    http: reqwest::Client,
}

impl PdsDelegationSource {
    pub fn new(pds_url: impl Into<String>, access_token: impl Into<String>) -> Self {
        Self {
            pds_url: pds_url.into().trim_end_matches('/').to_string(),
            access_token: access_token.into(),
            http: http_client(),
        }
    }
}

#[async_trait]
impl DelegationSource for PdsDelegationSource {
    async fn delegation_token(&self, space: &str) -> Result<String> {
        let resp = self
            .http
            .get(format!(
                "{}/xrpc/com.atproto.space.getDelegationToken",
                self.pds_url
            ))
            .bearer_auth(&self.access_token)
            .query(&[("space", space)])
            .send()
            .await
            .map_err(net_err)?;
        let out: GetDelegationTokenOutput = check(resp).await?.json().await.map_err(net_err)?;
        Ok(out.token)
    }
}

/// Yields a currently-valid space credential.
#[async_trait]
pub trait CredentialSource: Send + Sync {
    async fn credential(&self, now: u64) -> Result<String>;
}

/// Dev mode: a fixed, pre-issued space credential, bypassing minting entirely.
pub struct StaticCredential(pub String);

#[async_trait]
impl CredentialSource for StaticCredential {
    async fn credential(&self, _now: u64) -> Result<String> {
        Ok(self.0.clone())
    }
}

/// Mints and caches a space credential, re-minting once 80% of the credential
/// TTL has elapsed so the daemon never presents one near expiry.
pub struct CredentialProvider {
    space: String,
    source: Box<dyn DelegationSource>,
    host: Arc<dyn SpaceHostClient>,
    client_attestation: Option<String>,
    cached: Mutex<Option<(String, u64)>>,
}

impl CredentialProvider {
    pub fn new(
        space: impl Into<String>,
        source: Box<dyn DelegationSource>,
        host: Arc<dyn SpaceHostClient>,
    ) -> Self {
        Self {
            space: space.into(),
            source,
            host,
            client_attestation: None,
            cached: Mutex::new(None),
        }
    }
}

#[async_trait]
impl CredentialSource for CredentialProvider {
    async fn credential(&self, now: u64) -> Result<String> {
        let mut cached = self.cached.lock().await;
        if let Some((jwt, exp)) = cached.as_ref() {
            if now < exp.saturating_sub(CREDENTIAL_TTL_SECS / 5) {
                return Ok(jwt.clone());
            }
        }
        let token = self.source.delegation_token(&self.space).await?;
        let jwt = self
            .host
            .get_space_credential(&self.space, &token, self.client_attestation.as_deref())
            .await?;
        let exp = decode(&jwt)?.claims.exp;
        tracing::info!(space = %self.space, exp, "minted space credential");
        *cached = Some((jwt.clone(), exp));
        Ok(jwt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};
    use rsky_lexicon::com::atproto::space::ListReposOutput;
    use rsky_space::credential::{encode, JwtHeader, SpaceClaims, CREDENTIAL_TYP};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const SPACE: &str = "at://did:plc:authority/space/community.blacksky.feed/main";

    fn credential_jwt(iat: u64) -> String {
        let header = JwtHeader {
            typ: CREDENTIAL_TYP.to_string(),
            alg: "ES256K".to_string(),
            kid: None,
        };
        let claims = SpaceClaims {
            iss: "did:plc:authority".to_string(),
            sub: SPACE.to_string(),
            aud: None,
            iat,
            exp: iat + CREDENTIAL_TTL_SECS,
            jti: format!("jti-{iat}"),
        };
        encode(&header, &claims, |_| Ok(vec![0u8; 64])).unwrap()
    }

    struct FixedDelegation;
    #[async_trait]
    impl DelegationSource for FixedDelegation {
        async fn delegation_token(&self, space: &str) -> Result<String> {
            assert_eq!(space, SPACE);
            Ok("dt.jwt".to_string())
        }
    }

    /// Mints a fresh credential (iat advances per mint) and counts mints.
    struct MintingHost {
        mints: AtomicUsize,
        iats: Vec<u64>,
    }
    #[async_trait]
    impl SpaceHostClient for MintingHost {
        async fn get_space_credential(
            &self,
            space: &str,
            delegation_token: &str,
            client_attestation: Option<&str>,
        ) -> Result<String> {
            assert_eq!(space, SPACE);
            assert_eq!(delegation_token, "dt.jwt");
            assert_eq!(client_attestation, None);
            let i = self.mints.fetch_add(1, Ordering::SeqCst);
            Ok(credential_jwt(self.iats[i]))
        }
        async fn list_repos(
            &self,
            _space: &str,
            _credential: &str,
            _cursor: Option<&str>,
            _limit: Option<i64>,
        ) -> Result<ListReposOutput> {
            Ok(ListReposOutput {
                cursor: None,
                repos: vec![],
            })
        }
        async fn register_notify(
            &self,
            _space: &str,
            _credential: &str,
            _endpoint: &str,
        ) -> Result<DateTime<Utc>> {
            Ok(Utc::now())
        }
    }

    #[tokio::test]
    async fn caches_and_refreshes_past_80_percent_of_ttl() {
        let host = Arc::new(MintingHost {
            mints: AtomicUsize::new(0),
            iats: vec![1000, 7000],
        });
        let provider = CredentialProvider::new(SPACE, Box::new(FixedDelegation), host.clone());

        let first = provider.credential(1000).await.unwrap();
        assert_eq!(first, credential_jwt(1000));
        // Before 80% of the 2h TTL (1000 + 5760): served from cache.
        assert_eq!(provider.credential(6759).await.unwrap(), first);
        assert_eq!(host.mints.load(Ordering::SeqCst), 1);
        // At the refresh threshold: re-minted.
        let second = provider.credential(6760).await.unwrap();
        assert_eq!(second, credential_jwt(7000));
        assert_eq!(host.mints.load(Ordering::SeqCst), 2);
    }

    struct GarbageHost;
    #[async_trait]
    impl SpaceHostClient for GarbageHost {
        async fn get_space_credential(
            &self,
            _space: &str,
            _delegation_token: &str,
            _client_attestation: Option<&str>,
        ) -> Result<String> {
            Ok("not-a-jwt".to_string())
        }
        async fn list_repos(
            &self,
            _space: &str,
            _credential: &str,
            _cursor: Option<&str>,
            _limit: Option<i64>,
        ) -> Result<ListReposOutput> {
            Ok(ListReposOutput {
                cursor: None,
                repos: vec![],
            })
        }
        async fn register_notify(
            &self,
            _space: &str,
            _credential: &str,
            _endpoint: &str,
        ) -> Result<DateTime<Utc>> {
            Ok(Utc::now())
        }
    }

    #[tokio::test]
    async fn undecodable_minted_credential_is_an_error() {
        let provider =
            CredentialProvider::new(SPACE, Box::new(FixedDelegation), Arc::new(GarbageHost));
        let err = provider.credential(1000).await.unwrap_err();
        assert!(matches!(
            err,
            crate::error::DaemonError::Space(rsky_space::SpaceError::MalformedJwt(_))
        ));
        // GarbageHost's unused trait methods, for completeness.
        let host = GarbageHost;
        assert!(host.list_repos(SPACE, "c", None, None).await.is_ok());
        assert!(host.register_notify(SPACE, "c", "e").await.is_ok());
    }

    #[tokio::test]
    async fn static_credential_bypasses_minting() {
        let source = StaticCredential("static.jwt".to_string());
        assert_eq!(source.credential(0).await.unwrap(), "static.jwt");
    }

    #[tokio::test]
    async fn pds_delegation_source_calls_get_delegation_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/xrpc/com.atproto.space.getDelegationToken"))
            .and(query_param("space", SPACE))
            .and(header("authorization", "Bearer access.jwt"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"token": "dt.jwt"})),
            )
            .mount(&server)
            .await;

        let source = PdsDelegationSource::new(format!("{}/", server.uri()), "access.jwt");
        assert_eq!(source.delegation_token(SPACE).await.unwrap(), "dt.jwt");
    }

    #[tokio::test]
    async fn pds_delegation_source_surfaces_errors() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/xrpc/com.atproto.space.getDelegationToken"))
            .respond_with(
                ResponseTemplate::new(401)
                    .set_body_json(serde_json::json!({"error": "AuthenticationRequired"})),
            )
            .mount(&server)
            .await;

        let source = PdsDelegationSource::new(server.uri(), "expired.jwt");
        assert!(source.delegation_token(SPACE).await.is_err());
    }

    #[test]
    fn unix_now_is_after_2020() {
        assert!(unix_now() > 1_577_836_800);
    }

    // The MintingHost's unused trait methods still need exercising once.
    #[tokio::test]
    async fn minting_host_stubs() {
        let host = MintingHost {
            mints: AtomicUsize::new(0),
            iats: vec![],
        };
        assert!(host.list_repos(SPACE, "c", None, None).await.is_ok());
        assert!(host.register_notify(SPACE, "c", "e").await.is_ok());
    }
}
