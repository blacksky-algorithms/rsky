//! The browser UI: the OAuth authorization screens and the account manager,
//! rendered on the server from askama templates, styled by one stylesheet,
//! and branded by the deployment's environment.

pub mod assets;
pub mod branding;
pub mod client;
pub mod format;
pub mod pages;
pub mod respond;
pub mod scopes;
pub mod shell;
pub mod technical;

use branding::Branding;
use rsky_common::env::{env_list, env_str};
use shell::PageShell;
use std::sync::Arc;

/// Rocket-managed state every page handler reaches for.
pub struct UiState {
    pub shell: Arc<PageShell>,
    /// Clients the deployment operates itself, from
    /// `PDS_OAUTH_FIRST_PARTY_CLIENTS`; the identity warning is not shown
    /// for one of these when it is also trusted
    pub first_party_clients: Vec<String>,
    /// Where "Create a new account" leads: an external page from
    /// `PDS_OAUTH_SIGNUP_URL`, this server's own sign-up when handles are
    /// offered and `PDS_ACCOUNT_SIGNUP_ENABLED` is not false, or nothing
    pub signup: SignUp,
    /// Keys the attestation the delete flow carries between its steps
    pub intent_key: [u8; 32],
}

/// How a new account is created from the pages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignUp {
    Disabled,
    External(String),
    Internal,
}

impl UiState {
    pub fn new(
        branding: &Branding,
        public_url: &str,
        hostname: &str,
        handle_domains: &[String],
    ) -> Self {
        let signup = match env_str("PDS_OAUTH_SIGNUP_URL")
            .map(|url| url.trim().to_string())
            .filter(|url| !url.is_empty())
        {
            Some(url) => SignUp::External(url),
            None if !handle_domains.is_empty()
                && rsky_common::env::env_bool("PDS_ACCOUNT_SIGNUP_ENABLED").unwrap_or(true) =>
            {
                SignUp::Internal
            }
            None => SignUp::Disabled,
        };
        Self::with_options(
            branding,
            public_url,
            hostname,
            env_list("PDS_OAUTH_FIRST_PARTY_CLIENTS"),
            signup,
            intent_key_for(
                env_str("PDS_JWT_SECRET"),
                env_str("PDS_JWT_KEY_K256_PRIVATE_KEY_HEX"),
            ),
        )
    }

    pub fn with_options(
        branding: &Branding,
        public_url: &str,
        hostname: &str,
        first_party_clients: Vec<String>,
        signup: SignUp,
        intent_key: [u8; 32],
    ) -> Self {
        UiState {
            shell: Arc::new(PageShell::new(branding, public_url, hostname)),
            first_party_clients: first_party_clients
                .into_iter()
                .map(|id| id.trim().to_string())
                .filter(|id| !id.is_empty())
                .collect(),
            signup,
            intent_key,
        }
    }

    /// The sign-up link for the account pages, when there is one.
    pub fn account_signup_href(&self) -> Option<String> {
        match &self.signup {
            SignUp::Disabled => None,
            SignUp::External(url) => Some(url.clone()),
            SignUp::Internal => Some("/account/sign-up".to_string()),
        }
    }

    /// The sign-up link inside an authorization request, when there is one.
    pub fn flow_signup_href(&self, authorize_href: &str) -> Option<String> {
        match &self.signup {
            SignUp::Disabled => None,
            SignUp::External(url) => Some(url.clone()),
            SignUp::Internal => Some(format!("{authorize_href}&view=sign-up")),
        }
    }

    pub fn offers_signup(&self) -> bool {
        self.signup != SignUp::Disabled
    }

    pub fn is_first_party_client(&self, client_id: &str) -> bool {
        self.first_party_clients.iter().any(|id| id == client_id)
    }
}

/// The delete-intent key, derived from whichever session-signing material
/// the deployment has; one of the two is required to start at all.
pub fn intent_key_for(secret: Option<String>, private_key_hex: Option<String>) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let material: Vec<u8> = match (secret, private_key_hex) {
        (Some(secret), _) => secret.into_bytes(),
        (None, Some(hex)) => {
            hex::decode(hex.trim()).expect("PDS_JWT_KEY_K256_PRIVATE_KEY_HEX must be hex")
        }
        (None, None) => panic!("PDS_JWT_SECRET or PDS_JWT_KEY_K256_PRIVATE_KEY_HEX must be set"),
    };
    let mut hasher = Sha256::new();
    hasher.update(b"rsky-delete-intent");
    hasher.update(&material);
    hasher.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_party_clients_are_trimmed_and_matched_exactly() {
        let state = UiState::with_options(
            &Branding::default(),
            "https://pds.test",
            "pds.test",
            vec![" https://a.test/c.json ".into(), String::new()],
            SignUp::Disabled,
            [0; 32],
        );
        assert_eq!(state.first_party_clients, ["https://a.test/c.json"]);
        assert!(state.is_first_party_client("https://a.test/c.json"));
        assert!(!state.is_first_party_client("https://b.test/c.json"));
        assert_eq!(state.account_signup_href(), None);
        assert_eq!(state.flow_signup_href("/oauth/authorize?x"), None);
        assert!(!state.offers_signup());
        assert_eq!(state.shell.hostname, "pds.test");
    }

    #[test]
    fn sign_up_links_follow_the_configured_kind() {
        let external = UiState::with_options(
            &Branding::default(),
            "https://pds.test",
            "pds.test",
            vec![],
            SignUp::External("https://signup.test".into()),
            [0; 32],
        );
        assert_eq!(
            external.account_signup_href().as_deref(),
            Some("https://signup.test")
        );
        assert_eq!(
            external.flow_signup_href("/oauth/authorize?x").as_deref(),
            Some("https://signup.test")
        );
        let internal = UiState::with_options(
            &Branding::default(),
            "https://pds.test",
            "pds.test",
            vec![],
            SignUp::Internal,
            [0; 32],
        );
        assert_eq!(
            internal.account_signup_href().as_deref(),
            Some("/account/sign-up")
        );
        assert_eq!(
            internal.flow_signup_href("/oauth/authorize?x").as_deref(),
            Some("/oauth/authorize?x&view=sign-up")
        );
        assert!(internal.offers_signup());
    }

    #[test]
    fn sign_up_is_internal_only_with_handle_domains() {
        std::env::remove_var("PDS_OAUTH_SIGNUP_URL");
        std::env::set_var("PDS_JWT_SECRET", "secret");
        let without = UiState::new(&Branding::default(), "https://pds.test", "pds.test", &[]);
        assert_eq!(without.signup, SignUp::Disabled);
        let with = UiState::new(
            &Branding::default(),
            "https://pds.test",
            "pds.test",
            &[".pds.test".to_string()],
        );
        assert_eq!(with.signup, SignUp::Internal);
        std::env::set_var("PDS_OAUTH_SIGNUP_URL", "https://signup.test");
        let external = UiState::new(&Branding::default(), "https://pds.test", "pds.test", &[]);
        assert_eq!(
            external.signup,
            SignUp::External("https://signup.test".into())
        );
        std::env::remove_var("PDS_OAUTH_SIGNUP_URL");
    }

    #[test]
    fn the_intent_key_derives_from_either_signing_material() {
        let from_secret = intent_key_for(Some("secret".into()), None);
        let from_key = intent_key_for(
            None,
            Some("9d5907143471e8f0e8df0f8b9512a8c5377878ee767f18fcf961055ecfc071cd".into()),
        );
        assert_ne!(from_secret, from_key);
        assert_eq!(
            from_secret,
            intent_key_for(Some("secret".into()), Some("ff".into()))
        );
        assert!(std::panic::catch_unwind(|| intent_key_for(None, None)).is_err());
        assert!(std::panic::catch_unwind(|| intent_key_for(None, Some("zz".into()))).is_err());
    }
}
