use crate::account_manager::oauth_store::PdsOAuthStore;
use crate::db::sqlite::Db;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use rocket::fairing::{Fairing, Info, Kind};
use rocket::http::{Cookie, CookieJar, Header, SameSite};
use rocket::{Request, Response};
use rsky_common::env::{env_list, env_str};
use rsky_oauth::dpop::{
    DpopManager, DpopNonce, InMemoryReplayStore, ReplayStore, DEFAULT_ROTATION_INTERVAL,
};
use rsky_oauth::jwk::{EcCurve, Jwk, SigningKey};
use rsky_oauth::store::DeviceData;
use rsky_oauth::{OAuthError, OAuthProvider, OAuthProviderConfig, ScopeExpandError, ScopeExpander};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

pub mod body;
pub mod fetcher;
pub mod replay;
pub mod routes;
pub mod templates;

pub const DEVICE_COOKIE: &str = "device-id";

/// Expands `include:` permission sets into a granted scope's effective grants,
/// so the issued access token's `scope` claim carries the resolved `space:`
/// grants a client reads to know the session's access (proposal 0011). The
/// same resolver enforcement uses, cached, so issuance and enforcement agree.
#[derive(Default)]
pub struct IncludeExpander {
    resolver: crate::permission_set::PermissionSetResolver,
}

#[rocket::async_trait]
impl ScopeExpander for IncludeExpander {
    /// Every `include:` is replaced by the grants its set confers; a set that
    /// cannot be resolved fails the whole expansion.
    async fn expand(&self, granted_scope: &str) -> Result<String, ScopeExpandError> {
        let mut compiled = Vec::new();
        for scope in granted_scope.split_whitespace() {
            match scope.strip_prefix(crate::oauth_scope::INCLUDE_PREFIX) {
                Some(nsid) => compiled.extend(
                    self.resolver
                        .try_resolved_scopes(nsid)
                        .await
                        .map_err(|error| ScopeExpandError(error.to_string()))?,
                ),
                None => compiled.push(scope.to_owned()),
            }
        }
        Ok(compiled.join(" "))
    }
}

/// Rocket-managed OAuth provider handle.
pub struct SharedOAuthProvider {
    pub provider: Arc<OAuthProvider>,
}

/// The access-token signing key: the shared session secret when the server
/// is configured to interoperate with the reference PDS, otherwise the K-256
/// JWT key.
fn signing_key_from_env() -> SigningKey {
    signing_key_for(
        env_str("PDS_JWT_SECRET"),
        env_str("PDS_JWT_KEY_K256_PRIVATE_KEY_HEX"),
    )
}

fn signing_key_for(secret: Option<String>, private_key_hex: Option<String>) -> SigningKey {
    if let Some(secret) = secret {
        return SigningKey::Symmetric(secret.into_bytes());
    }
    let private_key =
        private_key_hex.expect("PDS_JWT_SECRET or PDS_JWT_KEY_K256_PRIVATE_KEY_HEX must be set");
    let key_bytes = hex::decode(private_key).expect("invalid provider signing key hex");
    SigningKey::Ec(
        Jwk::from_private_key_bytes(EcCurve::K256, &key_bytes)
            .expect("invalid provider signing key"),
    )
}

/// DPoP replay tracking shared through redis when `PDS_REDIS_SCRATCH_ADDRESS`
/// names one, otherwise kept in memory.
async fn replay_store_from_env() -> Box<dyn ReplayStore> {
    replay_store_for(
        env_str("PDS_REDIS_SCRATCH_ADDRESS"),
        env_str("PDS_REDIS_SCRATCH_PASSWORD"),
    )
    .await
}

async fn replay_store_for(
    address: Option<String>,
    password: Option<String>,
) -> Box<dyn ReplayStore> {
    let Some(address) = address else {
        return Box::new(InMemoryReplayStore::default());
    };
    let url = crate::rate_limits::scratch_redis_url(&address, password.as_deref());
    match replay::RedisReplayStore::connect(&url).await {
        Ok(store) => Box::new(store),
        Err(error) => panic!("PDS_REDIS_SCRATCH_ADDRESS is set but redis is unreachable: {error}"),
    }
}

impl SharedOAuthProvider {
    /// `sessions_since` is the instant (unix seconds) before which device
    /// authentications no longer count.
    pub async fn new(
        account_db: Db,
        issuer: String,
        audience: String,
        sessions_since: Option<u64>,
    ) -> Self {
        let signing_key = signing_key_from_env();
        let replay_store = replay_store_from_env().await;
        let nonce = match env_str("PDS_DPOP_SECRET") {
            Some(secret_hex) => {
                let secret: [u8; 32] = hex::decode(secret_hex)
                    .expect("PDS_DPOP_SECRET must be hex")
                    .try_into()
                    .expect("PDS_DPOP_SECRET must be 32 bytes");
                DpopNonce::new(secret, DEFAULT_ROTATION_INTERVAL)
            }
            None => DpopNonce::new_random(DEFAULT_ROTATION_INTERVAL),
        }
        .expect("valid DPoP nonce rotation interval");
        let provider = OAuthProvider::new(OAuthProviderConfig {
            issuer,
            audience,
            signing_key,
            fetcher: Arc::new(fetcher::HttpClientMetadataFetcher::default()),
            store: Arc::new(PdsOAuthStore::new(account_db)),
            dpop: DpopManager::new(Some(nonce), replay_store),
            trusted_clients: env_list("PDS_OAUTH_TRUSTED_CLIENTS"),
            scope_expander: Some(Arc::new(IncludeExpander::default())),
            sessions_since,
        });
        Self {
            provider: Arc::new(provider),
        }
    }
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_secs()
}

fn random_prefixed_id(prefix: &str) -> String {
    format!(
        "{prefix}{}",
        hex::encode(rsky_crypto::utils::random_bytes(16))
    )
}

/// The CSRF token for a device session is derived from the HttpOnly
/// cookie value, which page scripts and other origins cannot read.
pub fn csrf_token(cookie_value: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(cookie_value.as_bytes()))
}

/// The authenticated device session for the authorization UI.
pub struct DeviceSession {
    pub device_id: String,
    /// The session secret the request presented.
    pub session_id: String,
    pub csrf: String,
}

/// Builds the device cookie carrying `device_id` and `session_id`.
pub fn device_cookie(device_id: &str, session_id: &str) -> Cookie<'static> {
    Cookie::build((DEVICE_COOKIE, format!("{device_id}.{session_id}")))
        .http_only(true)
        .same_site(SameSite::Lax)
        .path("/oauth")
        .build()
}

/// Loads the device session from the cookie, creating a fresh device row
/// (and cookie) when absent or invalid.
pub async fn ensure_device_session(
    provider: &OAuthProvider,
    jar: &CookieJar<'_>,
    user_agent: Option<&str>,
    ip_address: &str,
    now: u64,
) -> Result<DeviceSession, OAuthError> {
    if let Some(cookie) = jar.get(DEVICE_COOKIE) {
        let value = cookie.value().to_string();
        if let Some((device_id, session_id)) = value.split_once('.') {
            if let Some(device) = provider.store().read_device(device_id).await? {
                if device.session_id == session_id {
                    return Ok(DeviceSession {
                        device_id: device_id.to_string(),
                        session_id: session_id.to_string(),
                        csrf: csrf_token(&value),
                    });
                }
            }
        }
    }
    let device_id = random_prefixed_id("dev-");
    let session_id = random_prefixed_id("ses-");
    provider
        .store()
        .create_device(
            &device_id,
            &DeviceData {
                session_id: session_id.clone(),
                user_agent: user_agent.map(String::from),
                ip_address: ip_address.to_string(),
                last_seen_at: now,
            },
        )
        .await?;
    let cookie = device_cookie(&device_id, &session_id);
    let csrf = csrf_token(cookie.value());
    jar.add(cookie);
    Ok(DeviceSession {
        device_id,
        session_id,
        csrf,
    })
}

/// Response headers produced while handling a DPoP-authenticated request,
/// staged in the request-local cache and emitted by [`OAuthHeaders`].
#[derive(Debug, Default, Clone)]
pub struct OAuthResponseHeaders {
    pub dpop_nonce: Option<String>,
    pub www_authenticate: Option<String>,
}

pub fn stage_oauth_headers(req: &Request<'_>, headers: OAuthResponseHeaders) {
    req.local_cache(|| headers);
}

pub struct OAuthHeaders;

#[rocket::async_trait]
impl Fairing for OAuthHeaders {
    fn info(&self) -> Info {
        Info {
            name: "OAuth response headers",
            kind: Kind::Response,
        }
    }

    async fn on_response<'r>(&self, request: &'r Request<'_>, response: &mut Response<'r>) {
        let headers: &OAuthResponseHeaders = request.local_cache(OAuthResponseHeaders::default);
        if let Some(nonce) = &headers.dpop_nonce {
            response.set_header(Header::new("DPoP-Nonce", nonce.clone()));
            response.adjoin_header(Header::new(
                "Access-Control-Expose-Headers",
                "DPoP-Nonce, WWW-Authenticate",
            ));
        }
        if let Some(www_authenticate) = &headers.www_authenticate {
            response.set_header(Header::new("WWW-Authenticate", www_authenticate.clone()));
        }
    }
}

#[cfg(test)]
mod configuration_tests {
    use super::*;

    const K256_HEX: &str = "9d5907143471e8f0e8df0f8b9512a8c5377878ee767f18fcf961055ecfc071cd";

    #[tokio::test]
    async fn include_expander_replaces_sets_and_fails_closed() {
        let expander = IncludeExpander::default();
        assert_eq!(
            expander.expand("atproto blob:image/*").await.unwrap(),
            "atproto blob:image/*"
        );
        expander
            .resolver
            .prime(
                "app.example.set",
                vec!["repo:app.example.record".to_string()],
            )
            .await;
        assert_eq!(
            expander
                .expand("atproto include:app.example.set transition:generic")
                .await
                .unwrap(),
            "atproto repo:app.example.record transition:generic"
        );
        // `.invalid` never resolves, so the set cannot be established
        let err = expander
            .expand("atproto include:invalid.example.nothing")
            .await
            .unwrap_err();
        assert!(err.0.contains("invalid.example.nothing"));
    }

    #[test]
    fn device_cookie_carries_both_ids_under_the_oauth_path() {
        let cookie = device_cookie("dev-1", "ses-1");
        assert_eq!(cookie.name(), DEVICE_COOKIE);
        assert_eq!(cookie.value(), "dev-1.ses-1");
        assert_eq!(cookie.path(), Some("/oauth"));
        assert_eq!(cookie.http_only(), Some(true));
        assert_eq!(cookie.same_site(), Some(SameSite::Lax));
    }

    #[test]
    fn signing_key_prefers_the_shared_secret() {
        assert!(matches!(
            signing_key_for(Some("secret".to_owned()), Some(K256_HEX.to_owned())),
            SigningKey::Symmetric(_)
        ));
        assert!(matches!(
            signing_key_for(None, Some(K256_HEX.to_owned())),
            SigningKey::Ec(_)
        ));
    }

    #[test]
    #[should_panic(expected = "must be set")]
    fn signing_key_requires_a_key() {
        signing_key_for(None, None);
    }

    #[tokio::test]
    async fn replay_store_is_in_memory_without_redis() {
        let store = replay_store_for(None, None).await;
        assert!(store.unique("DPoP", "a", 1000).await.unwrap());
        assert!(!store.unique("DPoP", "a", 1000).await.unwrap());
    }

    /// Runs against the redis named by `TEST_REDIS_URL`, skipped without one.
    #[tokio::test]
    async fn replay_store_uses_redis_when_configured() {
        let Ok(url) = std::env::var("TEST_REDIS_URL") else {
            return;
        };
        let address = url.trim_start_matches("redis://").to_owned();
        let store = replay_store_for(Some(address), Some(String::new())).await;
        let nonce = format!("jti-{}", rsky_common::get_random_str());
        assert!(store.unique("DPoP", &nonce, 60_000).await.unwrap());
        assert!(!store.unique("DPoP", &nonce, 60_000).await.unwrap());
    }

    #[tokio::test]
    #[should_panic(expected = "redis is unreachable")]
    async fn replay_store_refuses_to_start_without_its_redis() {
        replay_store_for(Some("127.0.0.1:1".to_owned()), None).await;
    }
}
