//! Fixed-window request limits with the reference PDS's table, keys, and
//! answers. `PDS_RATE_LIMITS_ENABLED` turns them on; a bypass key or
//! address skips every limit. With `PDS_REDIS_SCRATCH_ADDRESS` the windows
//! live in redis under the reference's key scheme and script, so every
//! process sharing that redis charges the same budgets; otherwise they live
//! in this process's memory.

use crate::apis::ApiError;
use redis::aio::{ConnectionManager, ConnectionManagerConfig};
use rocket::http::Header;
use rocket::request::{FromRequest, Outcome};
use rocket::Request;
use rsky_common::env::{env_bool, env_list, env_str};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MINUTE: u64 = 60;
const HOUR: u64 = 60 * MINUTE;
const DAY: u64 = 24 * HOUR;

/// One window: so many points per duration for one key. `prefix` is the
/// reference PDS's redis key prefix for the same limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limit {
    pub name: &'static str,
    pub prefix: &'static str,
    pub duration_secs: u64,
    pub points: u32,
}

const fn limit(name: &'static str, prefix: &'static str, duration_secs: u64, points: u32) -> Limit {
    Limit {
        name,
        prefix,
        duration_secs,
        points,
    }
}

/// The reference PDS's per-address limit on every XRPC request except
/// repository exports, which have their own budget.
pub const GLOBAL_IP: Limit = limit("global-ip", "rl-global-ip", 5 * MINUTE, 3000);
pub const REPO_WRITE_HOUR: Limit = limit("repo-write-hour", "rl-repo-write-hour", HOUR, 5000);
pub const REPO_WRITE_DAY: Limit = limit("repo-write-day", "rl-repo-write-day", DAY, 35000);
pub const CREATE_SESSION: [Limit; 2] = [
    limit(
        "createSession-day",
        "com.atproto.server.createSession-0",
        DAY,
        300,
    ),
    limit(
        "createSession-5min",
        "com.atproto.server.createSession-1",
        5 * MINUTE,
        30,
    ),
];
pub const CREATE_ACCOUNT: [Limit; 1] = [limit(
    "createAccount",
    "com.atproto.server.createAccount-0",
    5 * MINUTE,
    100,
)];
pub const UPLOAD_BLOB: [Limit; 1] = [limit(
    "uploadBlob",
    "com.atproto.repo.uploadBlob-0",
    DAY,
    1000,
)];
pub const GET_REPO: [Limit; 1] = [limit(
    "getRepo",
    "com.atproto.sync.getRepo-0",
    5 * MINUTE,
    6000,
)];
pub const UPDATE_HANDLE: [Limit; 2] = [
    limit(
        "updateHandle-5min",
        "com.atproto.identity.updateHandle-0",
        5 * MINUTE,
        10,
    ),
    limit(
        "updateHandle-day",
        "com.atproto.identity.updateHandle-1",
        DAY,
        50,
    ),
];
pub const DELETE_ACCOUNT: [Limit; 1] = [limit(
    "deleteAccount",
    "com.atproto.server.deleteAccount-0",
    5 * MINUTE,
    50,
)];
pub const RESET_PASSWORD: [Limit; 1] = [limit(
    "resetPassword",
    "com.atproto.server.resetPassword-0",
    5 * MINUTE,
    50,
)];
pub const REQUEST_PASSWORD_RESET: [Limit; 2] = [
    limit(
        "requestPasswordReset-day",
        "com.atproto.server.requestPasswordReset-0",
        DAY,
        50,
    ),
    limit(
        "requestPasswordReset-hour",
        "com.atproto.server.requestPasswordReset-1",
        HOUR,
        15,
    ),
];
pub const REQUEST_ACCOUNT_DELETE: [Limit; 2] = [
    limit(
        "requestAccountDelete-day",
        "com.atproto.server.requestAccountDelete-0",
        DAY,
        15,
    ),
    limit(
        "requestAccountDelete-hour",
        "com.atproto.server.requestAccountDelete-1",
        HOUR,
        5,
    ),
];
pub const REQUEST_EMAIL_CONFIRMATION: [Limit; 2] = [
    limit(
        "requestEmailConfirmation-day",
        "com.atproto.server.requestEmailConfirmation-0",
        DAY,
        15,
    ),
    limit(
        "requestEmailConfirmation-hour",
        "com.atproto.server.requestEmailConfirmation-1",
        HOUR,
        5,
    ),
];
pub const REQUEST_EMAIL_UPDATE: [Limit; 2] = [
    limit(
        "requestEmailUpdate-day",
        "com.atproto.server.requestEmailUpdate-0",
        DAY,
        15,
    ),
    limit(
        "requestEmailUpdate-hour",
        "com.atproto.server.requestEmailUpdate-1",
        HOUR,
        5,
    ),
];
pub const REPO_WRITES: [Limit; 2] = [REPO_WRITE_HOUR, REPO_WRITE_DAY];

/// Every limit this server charges.
pub const ALL_LIMITS: [Limit; 20] = [
    GLOBAL_IP,
    REPO_WRITE_HOUR,
    REPO_WRITE_DAY,
    CREATE_SESSION[0],
    CREATE_SESSION[1],
    CREATE_ACCOUNT[0],
    UPLOAD_BLOB[0],
    GET_REPO[0],
    UPDATE_HANDLE[0],
    UPDATE_HANDLE[1],
    DELETE_ACCOUNT[0],
    RESET_PASSWORD[0],
    REQUEST_PASSWORD_RESET[0],
    REQUEST_PASSWORD_RESET[1],
    REQUEST_ACCOUNT_DELETE[0],
    REQUEST_ACCOUNT_DELETE[1],
    REQUEST_EMAIL_CONFIRMATION[0],
    REQUEST_EMAIL_CONFIRMATION[1],
    REQUEST_EMAIL_UPDATE[0],
    REQUEST_EMAIL_UPDATE[1],
];

/// Points a repository write costs, as the reference charges them.
pub const CREATE_POINTS: u32 = 3;
pub const UPDATE_POINTS: u32 = 2;
pub const DELETE_POINTS: u32 = 1;

/// The script rate-limiter-flexible runs for one consumption: create the
/// counter with the window's expiry when absent, add the points, and
/// report the count and the time left. Running the same script over the
/// same keys keeps the budgets shared with the reference process.
const CONSUME_SCRIPT: &str = "redis.call('set', KEYS[1], 0, 'EX', ARGV[2], 'NX') \
local consumed = redis.call('incrby', KEYS[1], ARGV[1]) \
local ttl = redis.call('pttl', KEYS[1]) \
if ttl == -1 then \
  redis.call('expire', KEYS[1], ARGV[2]) \
  ttl = 1000 * ARGV[2] \
end \
return {consumed, ttl}";

/// Where a limit stands after a request, for the response headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LimitStatus {
    pub limit: u32,
    pub duration_secs: u64,
    pub remaining: u32,
    pub reset_at_secs: u64,
    pub retry_after_secs: u64,
}

impl LimitStatus {
    pub fn headers(&self) -> Vec<Header<'static>> {
        vec![
            Header::new("RateLimit-Limit", self.limit.to_string()),
            Header::new("RateLimit-Remaining", self.remaining.to_string()),
            Header::new("RateLimit-Reset", self.reset_at_secs.to_string()),
            Header::new(
                "RateLimit-Policy",
                format!("{};w={}", self.limit, self.duration_secs),
            ),
            Header::new("Retry-After", self.retry_after_secs.to_string()),
            Header::new(
                "Access-Control-Expose-Headers",
                "RateLimit-Limit, RateLimit-Reset, RateLimit-Remaining, RateLimit-Policy, Retry-After",
            ),
        ]
    }

    fn from_counter(limit: &Limit, consumed: i64, ms_before_next: u64, wall: SystemTime) -> Self {
        let wall_secs = wall
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let consumed = u32::try_from(consumed.max(0)).unwrap_or(u32::MAX);
        LimitStatus {
            limit: limit.points,
            duration_secs: limit.duration_secs,
            remaining: limit.points.saturating_sub(consumed),
            reset_at_secs: wall_secs + ms_before_next / 1000,
            retry_after_secs: (ms_before_next / 1000).max(1),
        }
    }
}

struct Window {
    started: Instant,
    consumed: u32,
}

struct RedisStore {
    connection: tokio::sync::Mutex<ConnectionManager>,
    script: redis::Script,
}

enum Store {
    Memory(Mutex<HashMap<(&'static str, String), Window>>),
    Redis(RedisStore),
}

pub struct RateLimits {
    enabled: bool,
    bypass_key: Option<String>,
    bypass_ips: Vec<IpAddr>,
    store: Store,
}

impl RateLimits {
    pub fn new(enabled: bool, bypass_key: Option<String>, bypass_ips: Vec<IpAddr>) -> Self {
        RateLimits {
            enabled,
            bypass_key: bypass_key.filter(|key| !key.is_empty()),
            bypass_ips,
            store: Store::Memory(Mutex::new(HashMap::new())),
        }
    }

    /// Limits whose windows live in the redis at `url`.
    pub async fn with_redis(
        enabled: bool,
        bypass_key: Option<String>,
        bypass_ips: Vec<IpAddr>,
        url: &str,
    ) -> Result<Self, redis::RedisError> {
        let client = redis::Client::open(url)?;
        let config = ConnectionManagerConfig::new()
            .set_number_of_retries(3)
            .set_max_delay(1_000)
            .set_connection_timeout(Duration::from_secs(5))
            .set_response_timeout(Duration::from_secs(5));
        let connection = ConnectionManager::new_with_config(client, config).await?;
        Ok(RateLimits {
            enabled,
            bypass_key: bypass_key.filter(|key| !key.is_empty()),
            bypass_ips,
            store: Store::Redis(RedisStore {
                connection: tokio::sync::Mutex::new(connection),
                script: redis::Script::new(CONSUME_SCRIPT),
            }),
        })
    }

    fn env_settings() -> (bool, Option<String>, Vec<IpAddr>) {
        let bypass_ips = env_list("PDS_RATE_LIMIT_BYPASS_IPS")
            .iter()
            .filter_map(|ip| ip.trim().parse().ok())
            .collect();
        (
            env_bool("PDS_RATE_LIMITS_ENABLED").unwrap_or(false),
            env_str("PDS_RATE_LIMIT_BYPASS_KEY"),
            bypass_ips,
        )
    }

    /// In-memory limits configured from the environment.
    pub fn from_env() -> Self {
        let (enabled, bypass_key, bypass_ips) = Self::env_settings();
        RateLimits::new(enabled, bypass_key, bypass_ips)
    }

    /// Limits configured from the environment, shared through redis when
    /// `PDS_REDIS_SCRATCH_ADDRESS` names one and limits are enabled.
    pub async fn connect_from_env() -> Self {
        let (enabled, bypass_key, bypass_ips) = Self::env_settings();
        match env_str("PDS_REDIS_SCRATCH_ADDRESS").filter(|url| enabled && !url.is_empty()) {
            None => RateLimits::new(enabled, bypass_key, bypass_ips),
            Some(url) => {
                match RateLimits::with_redis(enabled, bypass_key, bypass_ips, &url).await {
                    Ok(limits) => {
                        tracing::info!("rate limits shared through redis");
                        limits
                    }
                    Err(error) => {
                        panic!("PDS_REDIS_SCRATCH_ADDRESS is set but redis is unreachable: {error}")
                    }
                }
            }
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Whether the windows are shared through redis.
    pub fn shared(&self) -> bool {
        matches!(self.store, Store::Redis(_))
    }

    /// Whether a request with this bypass header or address skips every limit.
    pub fn bypasses(&self, bypass_header: Option<&str>, ip: Option<IpAddr>) -> bool {
        let by_key =
            matches!((&self.bypass_key, bypass_header), (Some(key), Some(header)) if key == header);
        by_key || ip.is_some_and(|ip| self.bypass_ips.contains(&ip))
    }

    /// The redis key a limit uses for a caller, as the reference writes it.
    pub fn redis_key(limit: &Limit, key: &str) -> String {
        format!("{}:{}", limit.prefix, key)
    }

    /// Charges `points` against `limit` for `key`. `Err` carries the
    /// exhausted window's status. A redis that cannot answer lets the
    /// request through, as the reference does, and is counted.
    pub async fn consume(
        &self,
        limit: &Limit,
        key: &str,
        points: u32,
    ) -> Result<LimitStatus, LimitStatus> {
        match &self.store {
            Store::Memory(_) => {
                self.consume_at(limit, key, points, Instant::now(), SystemTime::now())
            }
            Store::Redis(store) => {
                let redis_key = Self::redis_key(limit, key);
                let result: Result<(i64, i64), redis::RedisError> = {
                    let mut connection = store.connection.lock().await;
                    store
                        .script
                        .key(&redis_key)
                        .arg(points)
                        .arg(limit.duration_secs)
                        .invoke_async(&mut *connection)
                        .await
                };
                match result {
                    Ok((consumed, ttl_ms)) => {
                        let status = LimitStatus::from_counter(
                            limit,
                            consumed,
                            u64::try_from(ttl_ms).unwrap_or(0),
                            SystemTime::now(),
                        );
                        if consumed > i64::from(limit.points) {
                            Err(status)
                        } else {
                            Ok(status)
                        }
                    }
                    Err(error) => {
                        tracing::warn!(
                            %error,
                            limit = limit.name,
                            "rate limit store unavailable; request allowed"
                        );
                        crate::metrics::METRICS.rate_limit_store_errors.inc();
                        Ok(LimitStatus::from_counter(
                            limit,
                            0,
                            limit.duration_secs * 1000,
                            SystemTime::now(),
                        ))
                    }
                }
            }
        }
    }

    fn consume_at(
        &self,
        limit: &Limit,
        key: &str,
        points: u32,
        now: Instant,
        wall: SystemTime,
    ) -> Result<LimitStatus, LimitStatus> {
        let Store::Memory(windows) = &self.store else {
            unreachable!("memory windows are only consulted for the memory store")
        };
        let duration = Duration::from_secs(limit.duration_secs);
        let mut windows = windows.lock().expect("rate limit windows poisoned");
        if windows.len() > 10_000 {
            windows.retain(|(name, _), window| {
                let expired = now.duration_since(window.started)
                    >= Duration::from_secs(limits_by_name(name).unwrap_or(limit.duration_secs));
                !expired
            });
        }
        let window = windows
            .entry((limit.name, key.to_string()))
            .or_insert(Window {
                started: now,
                consumed: 0,
            });
        if now.duration_since(window.started) >= duration {
            window.started = now;
            window.consumed = 0;
        }
        window.consumed = window.consumed.saturating_add(points);
        let elapsed = now.duration_since(window.started);
        let ms_before_next = duration.saturating_sub(elapsed);
        let status = LimitStatus::from_counter(
            limit,
            i64::from(window.consumed),
            u64::try_from(ms_before_next.as_millis()).unwrap_or(u64::MAX),
            wall,
        );
        if window.consumed > limit.points {
            Err(status)
        } else {
            Ok(status)
        }
    }

    /// Charges every limit in `limits` for `key`; the first exhausted one
    /// answers 429 with its headers.
    pub async fn consume_all(
        &self,
        limits: &[Limit],
        key: &str,
        points: u32,
        bypass: bool,
    ) -> Result<(), ApiError> {
        if !self.enabled || bypass {
            return Ok(());
        }
        for limit in limits {
            if let Err(status) = self.consume(limit, key, points).await {
                return Err(ApiError::RateLimitExceeded(status));
            }
        }
        Ok(())
    }
}

fn limits_by_name(name: &str) -> Option<u64> {
    ALL_LIMITS
        .iter()
        .find(|limit| limit.name == name)
        .map(|limit| limit.duration_secs)
}

/// The address a request came from: the first `X-Forwarded-For` entry
/// when the edge set one, else the connection's peer.
pub fn client_ip(req: &Request<'_>) -> Option<IpAddr> {
    req.headers()
        .get_one("X-Forwarded-For")
        .and_then(|value| value.split(',').next())
        .and_then(|first| first.trim().parse().ok())
        .or_else(|| req.client_ip())
}

/// What a handler needs to charge its route limits: the caller's address,
/// rendered, and whether the request bypasses limits.
pub struct Caller {
    pub ip: String,
    pub bypass: bool,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for Caller {
    type Error = ();

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let ip = client_ip(req);
        let bypass = req
            .rocket()
            .state::<RateLimits>()
            .is_some_and(|limits| limits.bypasses(req.headers().get_one("x-ratelimit-bypass"), ip));
        Outcome::Success(Caller {
            ip: ip.map(|ip| ip.to_string()).unwrap_or_default(),
            bypass,
        })
    }
}

/// Whether the global per-address limit applies to a path.
pub fn global_limit_applies(path: &str) -> bool {
    path.starts_with("/xrpc/") && path != "/xrpc/com.atproto.sync.getRepo"
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests that change limit settings in the environment take this lock.
    static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[tokio::test]
    async fn windows_fill_reset_and_report_like_the_reference() {
        let limits = RateLimits::new(
            true,
            Some("secret".into()),
            vec!["10.0.0.9".parse().unwrap()],
        );
        assert!(!limits.shared());
        let limit = limit("test", "test-0", 60, 2);
        let t0 = Instant::now();
        let wall = UNIX_EPOCH + Duration::from_secs(1_000);
        let first = limits.consume_at(&limit, "k", 1, t0, wall).unwrap();
        assert_eq!(
            first,
            LimitStatus {
                limit: 2,
                duration_secs: 60,
                remaining: 1,
                reset_at_secs: 1_060,
                retry_after_secs: 60,
            }
        );
        let second = limits
            .consume_at(&limit, "k", 1, t0 + Duration::from_secs(10), wall)
            .unwrap();
        assert_eq!(second.remaining, 0);
        let refused = limits
            .consume_at(&limit, "k", 1, t0 + Duration::from_secs(20), wall)
            .unwrap_err();
        assert_eq!(refused.remaining, 0);
        assert_eq!(refused.retry_after_secs, 40);
        // another key has its own window; the window resets after its duration
        assert!(limits.consume_at(&limit, "other", 1, t0, wall).is_ok());
        assert!(limits
            .consume_at(&limit, "k", 1, t0 + Duration::from_secs(60), wall)
            .is_ok());
        let headers = refused.headers();
        let names: Vec<&str> = headers.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "RateLimit-Limit",
                "RateLimit-Remaining",
                "RateLimit-Reset",
                "RateLimit-Policy",
                "Retry-After",
                "Access-Control-Expose-Headers"
            ]
        );
        assert_eq!(headers[3].value, "2;w=60");
        assert!(limits.bypasses(Some("secret"), None));
        assert!(!limits.bypasses(Some("wrong"), None));
        assert!(limits.bypasses(None, Some("10.0.0.9".parse().unwrap())));
        assert!(!limits.bypasses(None, Some("10.0.0.8".parse().unwrap())));
        assert!(limits.consume_all(&[limit], "k", 5, true).await.is_ok());
        assert!(matches!(
            limits.consume_all(&[limit], "k", 5, false).await,
            Err(ApiError::RateLimitExceeded(_))
        ));
        assert!(limits.consume(&limit, "fresh", 1).await.is_ok());
        let disabled = RateLimits::new(false, None, vec![]);
        assert!(!disabled.enabled());
        assert!(disabled
            .consume_all(&[limit], "k", 500, false)
            .await
            .is_ok());
        assert!(global_limit_applies("/xrpc/app.bsky.feed.getTimeline"));
        assert!(!global_limit_applies("/xrpc/com.atproto.sync.getRepo"));
        assert!(!global_limit_applies("/tls-check"));
        assert_eq!(limits_by_name("global-ip"), Some(300));
        assert_eq!(limits_by_name("nope"), None);
    }

    #[test]
    fn the_table_carries_the_reference_key_prefixes() {
        assert_eq!(
            RateLimits::redis_key(&GLOBAL_IP, "1.2.3.4"),
            "rl-global-ip:1.2.3.4"
        );
        assert_eq!(
            RateLimits::redis_key(&CREATE_SESSION[1], "alice.test-1.2.3.4"),
            "com.atproto.server.createSession-1:alice.test-1.2.3.4"
        );
        assert_eq!(
            RateLimits::redis_key(&REPO_WRITE_DAY, "did:plc:a"),
            "rl-repo-write-day:did:plc:a"
        );
        let mut prefixes: Vec<&str> = ALL_LIMITS.iter().map(|limit| limit.prefix).collect();
        prefixes.sort_unstable();
        prefixes.dedup();
        assert_eq!(
            prefixes.len(),
            ALL_LIMITS.len(),
            "every limit has its own prefix"
        );
        assert!(ALL_LIMITS
            .iter()
            .all(|limit| !limit.name.is_empty() && limit.points > 0));
    }

    #[test]
    fn stale_windows_are_pruned_once_the_table_is_large() {
        let limits = RateLimits::new(true, None, vec![]);
        let limit = limit("global-ip", "rl-global-ip", 300, 10);
        let t0 = Instant::now();
        let wall = SystemTime::now();
        for i in 0..10_001 {
            limits
                .consume_at(&limit, &format!("key-{i}"), 1, t0, wall)
                .unwrap();
        }
        let Store::Memory(windows) = &limits.store else {
            unreachable!()
        };
        assert_eq!(windows.lock().unwrap().len(), 10_001);
        limits
            .consume_at(&limit, "late", 1, t0 + Duration::from_secs(300), wall)
            .unwrap();
        assert_eq!(windows.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn settings_come_from_the_environment() {
        let _env = ENV_LOCK.lock().await;
        std::env::set_var("PDS_RATE_LIMITS_ENABLED", "true");
        std::env::set_var("PDS_RATE_LIMIT_BYPASS_KEY", "k");
        std::env::set_var("PDS_RATE_LIMIT_BYPASS_IPS", "10.0.0.1, junk");
        std::env::remove_var("PDS_REDIS_SCRATCH_ADDRESS");
        let limits = RateLimits::from_env();
        assert!(limits.enabled());
        assert!(limits.bypasses(Some("k"), None));
        assert!(limits.bypasses(None, Some("10.0.0.1".parse().unwrap())));
        let connected = RateLimits::connect_from_env().await;
        assert!(connected.enabled() && !connected.shared());
        std::env::remove_var("PDS_RATE_LIMITS_ENABLED");
        std::env::remove_var("PDS_RATE_LIMIT_BYPASS_KEY");
        std::env::remove_var("PDS_RATE_LIMIT_BYPASS_IPS");
        assert!(!RateLimits::from_env().enabled());
        // a redis address is only consulted while limits are enabled
        std::env::set_var("PDS_REDIS_SCRATCH_ADDRESS", "redis://127.0.0.1:1");
        assert!(!RateLimits::connect_from_env().await.shared());
        if let Ok(url) = std::env::var("TEST_REDIS_URL") {
            std::env::set_var("PDS_RATE_LIMITS_ENABLED", "true");
            std::env::set_var("PDS_REDIS_SCRATCH_ADDRESS", url);
            assert!(RateLimits::connect_from_env().await.shared());
            std::env::remove_var("PDS_RATE_LIMITS_ENABLED");
        }
        std::env::remove_var("PDS_REDIS_SCRATCH_ADDRESS");
        assert!(RateLimits::with_redis(true, None, vec![], "not a url")
            .await
            .is_err());
    }

    #[tokio::test]
    #[should_panic(expected = "redis is unreachable")]
    async fn an_unreachable_redis_stops_the_boot() {
        let _env = ENV_LOCK.lock().await;
        std::env::set_var("PDS_RATE_LIMITS_ENABLED", "true");
        std::env::set_var("PDS_REDIS_SCRATCH_ADDRESS", "redis://127.0.0.1:1");
        let limits = RateLimits::connect_from_env().await;
        std::env::remove_var("PDS_RATE_LIMITS_ENABLED");
        std::env::remove_var("PDS_REDIS_SCRATCH_ADDRESS");
        drop(limits);
    }

    /// Needs `TEST_REDIS_URL`; the windows and keys must match what the
    /// reference PDS writes so the two implementations share budgets.
    #[tokio::test]
    async fn redis_windows_are_shared_under_the_reference_keys() {
        let Ok(url) = std::env::var("TEST_REDIS_URL") else {
            eprintln!("TEST_REDIS_URL is not set; skipping the redis rate limit test");
            return;
        };
        let limits = RateLimits::with_redis(true, None, vec![], &url)
            .await
            .unwrap();
        assert!(limits.shared());
        let key = format!("shared-{}", std::process::id());
        let limit = limit("test", "rl-test", 60, 2);
        let redis_key = RateLimits::redis_key(&limit, &key);
        let client = redis::Client::open(url.as_str()).unwrap();
        let mut raw = client.get_multiplexed_async_connection().await.unwrap();
        let _: () = redis::cmd("DEL")
            .arg(&redis_key)
            .query_async(&mut raw)
            .await
            .unwrap();

        let first = limits.consume(&limit, &key, 1).await.unwrap();
        assert_eq!(
            (first.limit, first.remaining, first.duration_secs),
            (2, 1, 60)
        );
        assert!((59..=60).contains(&first.retry_after_secs));
        let stored: i64 = redis::cmd("GET")
            .arg(&redis_key)
            .query_async(&mut raw)
            .await
            .unwrap();
        assert_eq!(stored, 1);
        let ttl: i64 = redis::cmd("TTL")
            .arg(&redis_key)
            .query_async(&mut raw)
            .await
            .unwrap();
        assert!(ttl > 0 && ttl <= 60);
        // another process (here: a raw client) consuming the same key counts
        let _: () = redis::cmd("INCRBY")
            .arg(&redis_key)
            .arg(1)
            .query_async(&mut raw)
            .await
            .unwrap();
        let refused = limits.consume(&limit, &key, 1).await.unwrap_err();
        assert_eq!(refused.remaining, 0);
        assert!(matches!(
            limits.consume_all(&[limit], &key, 1, false).await,
            Err(ApiError::RateLimitExceeded(_))
        ));
        // a counter that lost its expiry gets one back
        let _: () = redis::cmd("PERSIST")
            .arg(&redis_key)
            .query_async(&mut raw)
            .await
            .unwrap();
        let _ = limits.consume(&limit, &key, 1).await;
        let ttl: i64 = redis::cmd("TTL")
            .arg(&redis_key)
            .query_async(&mut raw)
            .await
            .unwrap();
        assert!(ttl > 0);
        let _: () = redis::cmd("DEL")
            .arg(&redis_key)
            .query_async(&mut raw)
            .await
            .unwrap();
    }
}
