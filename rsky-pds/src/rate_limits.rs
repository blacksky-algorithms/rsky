//! Fixed-window request limits with the reference PDS's table, keys, and
//! answers, kept in this process's memory. `PDS_RATE_LIMITS_ENABLED`
//! turns them on; a bypass key or address skips every limit.

use crate::apis::ApiError;
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

/// One window: so many points per duration for one key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limit {
    pub name: &'static str,
    pub duration_secs: u64,
    pub points: u32,
}

const fn limit(name: &'static str, duration_secs: u64, points: u32) -> Limit {
    Limit {
        name,
        duration_secs,
        points,
    }
}

/// The reference PDS's per-address limit on every XRPC request except
/// repository exports, which have their own budget.
pub const GLOBAL_IP: Limit = limit("global-ip", 5 * MINUTE, 3000);
pub const REPO_WRITE_HOUR: Limit = limit("repo-write-hour", HOUR, 5000);
pub const REPO_WRITE_DAY: Limit = limit("repo-write-day", DAY, 35000);
pub const CREATE_SESSION: [Limit; 2] = [
    limit("createSession-day", DAY, 300),
    limit("createSession-5min", 5 * MINUTE, 30),
];
pub const CREATE_ACCOUNT: [Limit; 1] = [limit("createAccount", 5 * MINUTE, 100)];
pub const UPLOAD_BLOB: [Limit; 1] = [limit("uploadBlob", DAY, 1000)];
pub const UPDATE_HANDLE: [Limit; 2] = [
    limit("updateHandle-5min", 5 * MINUTE, 10),
    limit("updateHandle-day", DAY, 50),
];
pub const DELETE_ACCOUNT: [Limit; 1] = [limit("deleteAccount", 5 * MINUTE, 50)];
pub const RESET_PASSWORD: [Limit; 1] = [limit("resetPassword", 5 * MINUTE, 50)];
pub const REQUEST_PASSWORD_RESET: [Limit; 2] = [
    limit("requestPasswordReset-day", DAY, 50),
    limit("requestPasswordReset-hour", HOUR, 15),
];
pub const EMAIL_REQUESTS: [Limit; 2] = [
    limit("emailRequest-day", DAY, 15),
    limit("emailRequest-hour", HOUR, 5),
];
pub const REPO_WRITES: [Limit; 2] = [REPO_WRITE_HOUR, REPO_WRITE_DAY];

/// Points a repository write costs, as the reference charges them.
pub const CREATE_POINTS: u32 = 3;
pub const UPDATE_POINTS: u32 = 2;
pub const DELETE_POINTS: u32 = 1;

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
}

struct Window {
    started: Instant,
    consumed: u32,
}

pub struct RateLimits {
    enabled: bool,
    bypass_key: Option<String>,
    bypass_ips: Vec<IpAddr>,
    windows: Mutex<HashMap<(&'static str, String), Window>>,
}

impl RateLimits {
    pub fn new(enabled: bool, bypass_key: Option<String>, bypass_ips: Vec<IpAddr>) -> Self {
        RateLimits {
            enabled,
            bypass_key: bypass_key.filter(|key| !key.is_empty()),
            bypass_ips,
            windows: Mutex::new(HashMap::new()),
        }
    }

    pub fn from_env() -> Self {
        let bypass_ips = env_list("PDS_RATE_LIMIT_BYPASS_IPS")
            .iter()
            .filter_map(|ip| ip.trim().parse().ok())
            .collect();
        RateLimits::new(
            env_bool("PDS_RATE_LIMITS_ENABLED").unwrap_or(false),
            env_str("PDS_RATE_LIMIT_BYPASS_KEY"),
            bypass_ips,
        )
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Whether a request with this bypass header or address skips every limit.
    pub fn bypasses(&self, bypass_header: Option<&str>, ip: Option<IpAddr>) -> bool {
        let by_key =
            matches!((&self.bypass_key, bypass_header), (Some(key), Some(header)) if key == header);
        by_key || ip.is_some_and(|ip| self.bypass_ips.contains(&ip))
    }

    /// Charges `points` against `limit` for `key`. `Err` carries the
    /// exhausted window's status.
    pub fn consume(
        &self,
        limit: &Limit,
        key: &str,
        points: u32,
    ) -> Result<LimitStatus, LimitStatus> {
        self.consume_at(limit, key, points, Instant::now(), SystemTime::now())
    }

    fn consume_at(
        &self,
        limit: &Limit,
        key: &str,
        points: u32,
        now: Instant,
        wall: SystemTime,
    ) -> Result<LimitStatus, LimitStatus> {
        let duration = Duration::from_secs(limit.duration_secs);
        let mut windows = self.windows.lock().expect("rate limit windows poisoned");
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
        let wall_secs = wall
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let status = LimitStatus {
            limit: limit.points,
            duration_secs: limit.duration_secs,
            remaining: limit.points.saturating_sub(window.consumed),
            reset_at_secs: wall_secs + ms_before_next.as_secs(),
            retry_after_secs: ms_before_next.as_secs().max(1),
        };
        if window.consumed > limit.points {
            Err(status)
        } else {
            Ok(status)
        }
    }

    /// Charges every limit in `limits` for `key`; the first exhausted one
    /// answers 429 with its headers.
    pub fn consume_all(
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
            if let Err(status) = self.consume(limit, key, points) {
                return Err(ApiError::RateLimitExceeded(status));
            }
        }
        Ok(())
    }
}

fn limits_by_name(name: &str) -> Option<u64> {
    [
        GLOBAL_IP,
        REPO_WRITE_HOUR,
        REPO_WRITE_DAY,
        CREATE_SESSION[0],
        CREATE_SESSION[1],
        CREATE_ACCOUNT[0],
        UPLOAD_BLOB[0],
        UPDATE_HANDLE[0],
        UPDATE_HANDLE[1],
        DELETE_ACCOUNT[0],
        RESET_PASSWORD[0],
        REQUEST_PASSWORD_RESET[0],
        REQUEST_PASSWORD_RESET[1],
        EMAIL_REQUESTS[0],
        EMAIL_REQUESTS[1],
    ]
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

    #[test]
    fn windows_fill_reset_and_report_like_the_reference() {
        let limits = RateLimits::new(
            true,
            Some("secret".into()),
            vec!["10.0.0.9".parse().unwrap()],
        );
        let limit = limit("test", 60, 2);
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
        assert!(limits.consume_all(&[limit], "k", 5, true).is_ok());
        assert!(matches!(
            limits.consume_all(&[limit], "k", 5, false),
            Err(ApiError::RateLimitExceeded(_))
        ));
        let disabled = RateLimits::new(false, None, vec![]);
        assert!(!disabled.enabled());
        assert!(disabled.consume_all(&[limit], "k", 500, false).is_ok());

        assert!(global_limit_applies("/xrpc/app.bsky.feed.getTimeline"));
        assert!(!global_limit_applies("/xrpc/com.atproto.sync.getRepo"));
        assert!(!global_limit_applies("/tls-check"));
        assert_eq!(limits_by_name("global-ip"), Some(300));
        assert_eq!(limits_by_name("nope"), None);
    }

    #[test]
    fn stale_windows_are_pruned_once_the_table_is_large() {
        let limits = RateLimits::new(true, None, vec![]);
        let limit = limit("global-ip", 300, 10);
        let t0 = Instant::now();
        let wall = SystemTime::now();
        for i in 0..10_001 {
            limits
                .consume_at(&limit, &format!("key-{i}"), 1, t0, wall)
                .unwrap();
        }
        assert_eq!(limits.windows.lock().unwrap().len(), 10_001);
        limits
            .consume_at(&limit, "late", 1, t0 + Duration::from_secs(300), wall)
            .unwrap();
        assert_eq!(limits.windows.lock().unwrap().len(), 1);
    }

    #[test]
    fn settings_come_from_the_environment() {
        std::env::set_var("PDS_RATE_LIMITS_ENABLED", "true");
        std::env::set_var("PDS_RATE_LIMIT_BYPASS_KEY", "k");
        std::env::set_var("PDS_RATE_LIMIT_BYPASS_IPS", "10.0.0.1, junk");
        let limits = RateLimits::from_env();
        assert!(limits.enabled());
        assert!(limits.bypasses(Some("k"), None));
        assert!(limits.bypasses(None, Some("10.0.0.1".parse().unwrap())));
        std::env::remove_var("PDS_RATE_LIMITS_ENABLED");
        std::env::remove_var("PDS_RATE_LIMIT_BYPASS_KEY");
        std::env::remove_var("PDS_RATE_LIMIT_BYPASS_IPS");
        assert!(!RateLimits::from_env().enabled());
    }
}
