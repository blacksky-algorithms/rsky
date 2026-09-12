//! The one transport for requests whose destination someone else chose:
//! service endpoints from DID documents, subscriber endpoints, client
//! metadata URLs, well-known documents. Configured services (the PLC
//! directory, the app view, relays, mail) use their own plain transports
//! and never accept a user-derived destination.

use rsky_common::env::env_bool;
use rsky_identity::safe_fetch::{NetworkPolicy, SafeClient};
use std::sync::LazyLock;
use std::time::Duration;

/// Public addresses over https, unless the server runs in development
/// mode against local services.
pub fn policy() -> NetworkPolicy {
    policy_for(env_bool("PDS_DEV_MODE").unwrap_or(false))
}

fn policy_for(dev_mode: bool) -> NetworkPolicy {
    if dev_mode {
        NetworkPolicy::PERMISSIVE
    } else {
        NetworkPolicy::PUBLIC
    }
}

static CLIENT: LazyLock<SafeClient> =
    LazyLock::new(|| SafeClient::new(policy(), Duration::from_secs(30)).expect("reqwest client"));

pub fn client() -> &'static SafeClient {
    &CLIENT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn development_mode_relaxes_the_policy() {
        assert_eq!(policy_for(true), NetworkPolicy::PERMISSIVE);
        assert_eq!(policy_for(false), NetworkPolicy::PUBLIC);
        assert!(matches!(
            policy(),
            NetworkPolicy::PUBLIC | NetworkPolicy::PERMISSIVE
        ));
        assert!(client().checked("https://example.com/").is_ok());
    }
}
