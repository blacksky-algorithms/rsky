//! The browser UI: the OAuth authorization screens and the account manager,
//! rendered on the server from askama templates, styled by one stylesheet,
//! and branded by the deployment's environment.

pub mod assets;
pub mod branding;
pub mod client;
pub mod pages;
pub mod respond;
pub mod scopes;
pub mod shell;
pub mod technical;

use branding::Branding;
use rsky_common::env::env_list;
use shell::PageShell;
use std::sync::Arc;

/// Rocket-managed state every page handler reaches for.
pub struct UiState {
    pub shell: Arc<PageShell>,
    /// Clients the deployment operates itself, from
    /// `PDS_OAUTH_FIRST_PARTY_CLIENTS`; the identity warning is not shown
    /// for one of these when it is also trusted
    pub first_party_clients: Vec<String>,
}

impl UiState {
    pub fn new(branding: &Branding, public_url: &str, hostname: &str) -> Self {
        Self::with_first_party(
            branding,
            public_url,
            hostname,
            env_list("PDS_OAUTH_FIRST_PARTY_CLIENTS"),
        )
    }

    pub fn with_first_party(
        branding: &Branding,
        public_url: &str,
        hostname: &str,
        first_party_clients: Vec<String>,
    ) -> Self {
        UiState {
            shell: Arc::new(PageShell::new(branding, public_url, hostname)),
            first_party_clients: first_party_clients
                .into_iter()
                .map(|id| id.trim().to_string())
                .filter(|id| !id.is_empty())
                .collect(),
        }
    }

    pub fn is_first_party_client(&self, client_id: &str) -> bool {
        self.first_party_clients.iter().any(|id| id == client_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_party_clients_are_trimmed_and_matched_exactly() {
        let state = UiState::with_first_party(
            &Branding::default(),
            "https://pds.test",
            "pds.test",
            vec![" https://a.test/c.json ".into(), String::new()],
        );
        assert_eq!(state.first_party_clients, ["https://a.test/c.json"]);
        assert!(state.is_first_party_client("https://a.test/c.json"));
        assert!(!state.is_first_party_client("https://b.test/c.json"));
        assert_eq!(state.shell.hostname, "pds.test");
    }
}
