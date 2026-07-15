//! Per-app authorization (proposal §Configuration, Diary 6 client-ID filtering).
//!
//! A user must be authorized by the space `policy` AND their app by `appAccess`
//! for a credential to be minted. `appAccess` is an open union; two variants
//! exist today.

/// How the authority decides whether to authorize an *app*.
#[derive(Debug, Clone)]
pub enum AppAccess {
    /// Any application may access the space; no client attestation required
    /// (public clients work). This is the default.
    Open,
    /// Only the named `client_id`s may access the space, evaluated against the
    /// attested `client_id` (the `iss` of a verified client attestation).
    AllowList(Vec<String>),
}

impl Default for AppAccess {
    fn default() -> Self {
        Self::Open
    }
}

impl AppAccess {
    /// Whether an app (identified by its attested `client_id`, if any) is
    /// authorized. `Open` ignores the client entirely; `AllowList` requires a
    /// verified `client_id` present on the list.
    pub fn permits(&self, attested_client_id: Option<&str>) -> bool {
        match self {
            AppAccess::Open => true,
            AppAccess::AllowList(allowed) => attested_client_id
                .map(|c| allowed.iter().any(|a| a == c))
                .unwrap_or(false),
        }
    }

    /// Whether this configuration requires a client attestation at all.
    pub fn requires_attestation(&self) -> bool {
        matches!(self, AppAccess::AllowList(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_open() {
        assert!(matches!(AppAccess::default(), AppAccess::Open));
    }

    #[test]
    fn open_permits_any_client() {
        assert!(AppAccess::Open.permits(None));
        assert!(AppAccess::Open.permits(Some("https://app.example/client")));
        assert!(!AppAccess::Open.requires_attestation());
    }

    #[test]
    fn allowlist_requires_named_client() {
        let a = AppAccess::AllowList(vec!["https://blacksky.community/client".into()]);
        assert!(a.requires_attestation());
        assert!(!a.permits(None));
        assert!(!a.permits(Some("https://evil.example/client")));
        assert!(a.permits(Some("https://blacksky.community/client")));
    }
}
