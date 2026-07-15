use crate::account_manager::oauth_store::PdsOAuthStore;
use crate::db::sqlite::Db;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use rocket::fairing::{Fairing, Info, Kind};
use rocket::http::{Cookie, CookieJar, Header, SameSite};
use rocket::{Request, Response};
use rsky_common::env::{env_list, env_str};
use rsky_oauth::dpop::{DpopManager, DpopNonce, InMemoryReplayStore, DEFAULT_ROTATION_INTERVAL};
use rsky_oauth::jwk::{EcCurve, Jwk};
use rsky_oauth::store::DeviceData;
use rsky_oauth::{OAuthError, OAuthProvider, OAuthProviderConfig};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

pub mod fetcher;
pub mod routes;
pub mod templates;

pub const DEVICE_COOKIE: &str = "device-id";

/// Rocket-managed OAuth provider handle.
pub struct SharedOAuthProvider {
    pub provider: Arc<OAuthProvider>,
}

impl SharedOAuthProvider {
    pub fn new(account_db: Db, issuer: String, audience: String) -> Self {
        let private_key = std::env::var("PDS_JWT_KEY_K256_PRIVATE_KEY_HEX")
            .expect("PDS_JWT_KEY_K256_PRIVATE_KEY_HEX must be set");
        let key_bytes = hex::decode(private_key).expect("invalid provider signing key hex");
        let signing_key = Jwk::from_private_key_bytes(EcCurve::K256, &key_bytes)
            .expect("invalid provider signing key");
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
            fetcher: Arc::new(fetcher::HttpClientMetadataFetcher::new()),
            store: Arc::new(PdsOAuthStore::new(account_db)),
            dpop: DpopManager::new(Some(nonce), Box::new(InMemoryReplayStore::default())),
            trusted_clients: env_list("PDS_OAUTH_TRUSTED_CLIENTS"),
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
    pub csrf: String,
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
    let value = format!("{device_id}.{session_id}");
    let csrf = csrf_token(&value);
    let cookie = Cookie::build((DEVICE_COOKIE, value))
        .http_only(true)
        .same_site(SameSite::Lax)
        .path("/oauth")
        .build();
    jar.add(cookie);
    Ok(DeviceSession { device_id, csrf })
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
