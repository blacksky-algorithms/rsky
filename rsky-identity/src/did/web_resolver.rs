use crate::common::decode_uri_component;
use crate::errors::Error;
use crate::types::DidCache;
use anyhow::{bail, Result};
use serde_json::Value;
use std::time::Duration;
use url::Url;

pub const DOC_PATH: &str = "/.well-known/did.json";

#[derive(Clone, Debug)]
pub struct DidWebResolver {
    pub timeout: Duration,
    pub cache: Option<DidCache>,
}

impl DidWebResolver {
    pub fn new(timeout: Duration, cache: Option<DidCache>) -> Self {
        Self { timeout, cache }
    }

    pub async fn resolve_no_check(&self, did: String) -> Result<Option<Value>> {
        let parsed_id: String = did.split(":").collect::<Vec<&str>>()[2..].join(":");
        let parts = parsed_id
            .split(":")
            .into_iter()
            .map(|part| decode_uri_component(part))
            .collect::<Result<Vec<String>>>()?;
        let path: String;
        if parts.len() < 1 {
            bail!(Error::PoorlyFormattedDidError(did))
        } else if parts.len() == 1 {
            path = parts[0].clone() + DOC_PATH;
        } else {
            // how we *would* resolve a did:web with path, if atproto supported it
            // path = parts.join('/') + "/did.json";
            bail!(Error::UnsupportedDidWebPathError(did))
        }

        let mut url = Url::parse(&format!("https://{path}"))?;

        if url.host_str() == Some("localhost") {
            let _ = url.set_scheme("http");
        }

        let client = reqwest::Client::new();
        let response = client
            .get(url.to_string())
            .timeout(self.timeout)
            .header("Connection", "Keep-Alive")
            .header("Keep-Alive", "timeout=5, max=1000")
            .send()
            .await?;
        let res = &response;
        match res.error_for_status_ref() {
            Ok(_) => Ok(Some(response.json::<Value>().await?)),
            // Positively not found, versus due to e.g. network error
            Err(error) if error.status() == Some(reqwest::StatusCode::NOT_FOUND) => Ok(None),
            Err(error) => bail!(error.to_string()),
        }
    }
}
