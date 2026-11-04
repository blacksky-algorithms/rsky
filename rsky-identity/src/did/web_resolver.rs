use crate::common::decode_uri_component;
use crate::errors::Error;
use crate::safe_fetch::{NetworkPolicy, Redirects, SafeClient};
use crate::types::DidCache;
use anyhow::{bail, Result};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use url::Url;

pub const DOC_PATH: &str = "/.well-known/did.json";

/// The most a DID document may be.
const DOCUMENT_LIMIT: usize = 64 * 1024;

#[derive(Clone, Debug)]
pub struct DidWebResolver {
    pub timeout: Duration,
    pub cache: Option<Arc<dyn DidCache>>,
    client: SafeClient,
}

impl DidWebResolver {
    pub fn new(timeout: Duration, cache: Option<Arc<dyn DidCache>>) -> Self {
        Self {
            timeout,
            cache,
            client: SafeClient::new(NetworkPolicy::PUBLIC, timeout).expect("reqwest client"),
        }
    }

    /// Fetches documents under `policy` instead of the public default.
    pub fn with_network(mut self, policy: NetworkPolicy) -> Self {
        self.client = SafeClient::new(policy, self.timeout).expect("reqwest client");
        self
    }

    pub async fn resolve_no_check(&self, did: String) -> Result<Option<Value>> {
        let parsed_id: String = did.split(":").collect::<Vec<&str>>()[2..].join(":");
        let parts = parsed_id
            .split(":")
            .map(decode_uri_component)
            .collect::<Result<Vec<String>>>()?;
        let path: String = if parts.is_empty() {
            bail!(Error::PoorlyFormattedDidError(did))
        } else if parts.len() == 1 {
            parts[0].clone() + DOC_PATH
        } else {
            // how we *would* resolve a did:web with path, if atproto supported it
            // path = parts.join('/') + "/did.json";
            bail!(Error::UnsupportedDidWebPathError(did))
        };

        let mut url = Url::parse(&format!("https://{path}"))?;

        if url.host_str() == Some("localhost") {
            let _ = url.set_scheme("http");
        }

        let response = self.client.get(url, Redirects::Follow(3)).await?;
        let (status, body) = SafeClient::read_bounded(response, DOCUMENT_LIMIT).await?;
        if status == reqwest::StatusCode::NOT_FOUND {
            // Positively not found, versus due to e.g. network error
            return Ok(None);
        }
        if !status.is_success() {
            bail!("did:web document request answered {status}")
        }
        Ok(Some(serde_json::from_slice::<Value>(&body)?))
    }
}
