//! The publication frontier and repository export of one actor, read
//! from the configured PDS over a fixed transport: the address comes from
//! configuration, never from a DID document or a request.

use crate::types::WintermuteError;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// What the PDS reports about an actor's publication history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Frontier {
    pub did: String,
    #[serde(default)]
    pub publication_max_rev: Option<String>,
    #[serde(default)]
    pub exposed_max_rev: Option<String>,
    #[serde(default)]
    pub current_commit_cid: Option<String>,
    #[serde(default)]
    pub signed_commit_rev: Option<String>,
    #[serde(default)]
    pub repo_root_rev: Option<String>,
    #[serde(default)]
    pub restore_event_count: i64,
    #[serde(default)]
    pub lifetime: String,
    #[serde(default)]
    pub genesis_kind: String,
    pub complete: bool,
    #[serde(default)]
    pub reason: Option<String>,
}

/// A client for the reconciliation reads a PDS answers to its admin.
#[derive(Clone)]
pub struct FrontierClient {
    base_url: String,
    admin_password: String,
    http: reqwest::Client,
}

impl FrontierClient {
    pub fn new(base_url: &str, admin_password: &str) -> Result<Self, WintermuteError> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(600))
            .build()?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            admin_password: admin_password.to_owned(),
            http,
        })
    }

    fn admin_header(&self) -> String {
        use base64::Engine;
        let credentials = format!("admin:{}", self.admin_password);
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(credentials)
        )
    }

    /// The actor's frontier, fetched now.
    pub async fn frontier(&self, did: &str) -> Result<Frontier, WintermuteError> {
        let response = self
            .http
            .get(format!(
                "{}/xrpc/community.blacksky.pds.getPublicationFrontier",
                self.base_url
            ))
            .query(&[("did", did)])
            .header("Authorization", self.admin_header())
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(WintermuteError::Other(format!(
                "frontier request answered {}",
                response.status()
            )));
        }
        Ok(response.json::<Frontier>().await?)
    }

    /// The actor's current repository export.
    pub async fn repo_car(&self, did: &str) -> Result<Vec<u8>, WintermuteError> {
        let response = self
            .http
            .get(format!("{}/xrpc/com.atproto.sync.getRepo", self.base_url))
            .query(&[("did", did)])
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(WintermuteError::Other(format!(
                "repository export answered {}",
                response.status()
            )));
        }
        Ok(response.bytes().await?.to_vec())
    }
}
