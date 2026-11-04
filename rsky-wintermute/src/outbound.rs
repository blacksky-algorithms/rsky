//! The transport for requests to hosts named by DID documents.
//!
//! A PDS's repository export and its repository status come from here.
//! Configured services (the relay, the PLC directory, the reconciliation
//! PDS) use their own plain transports and never accept a resolved
//! destination.

use rsky_identity::safe_fetch::{NetworkPolicy, SafeClient};
use std::sync::LazyLock;

/// Public addresses over https, unless `FETCH_ALLOW_PRIVATE` says the
/// process runs against local services.
#[must_use]
pub fn policy() -> NetworkPolicy {
    policy_for(std::env::var("FETCH_ALLOW_PRIVATE").ok().as_deref())
}

fn policy_for(allow_private: Option<&str>) -> NetworkPolicy {
    if matches!(allow_private, Some("true" | "1")) {
        NetworkPolicy::PERMISSIVE
    } else {
        NetworkPolicy::PUBLIC
    }
}

static CLIENT: LazyLock<Option<SafeClient>> = LazyLock::new(|| {
    SafeClient::new(policy(), crate::config::backfiller_timeout())
        .inspect_err(|e| tracing::error!("outbound transport not built: {e}"))
        .ok()
});

/// The process-wide bound transport.
pub fn client() -> Result<SafeClient, crate::types::WintermuteError> {
    CLIENT.clone().ok_or_else(|| {
        crate::types::WintermuteError::Other("outbound transport unavailable".into())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_policy_follows_the_environment() {
        assert_eq!(policy_for(Some("true")), NetworkPolicy::PERMISSIVE);
        assert_eq!(policy_for(Some("1")), NetworkPolicy::PERMISSIVE);
        assert_eq!(policy_for(Some("no")), NetworkPolicy::PUBLIC);
        assert_eq!(policy_for(None), NetworkPolicy::PUBLIC);
        assert!(matches!(
            policy(),
            NetworkPolicy::PUBLIC | NetworkPolicy::PERMISSIVE
        ));
        assert!(client().is_ok());
    }
}
