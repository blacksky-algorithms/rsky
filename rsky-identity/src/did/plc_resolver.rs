use crate::common::encode_uri_component;
use crate::types::DidCache;
use anyhow::{bail, Result};
use serde_json::Value;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct DidPlcResolver {
    pub plc_url: String,
    pub timeout: Duration,
    pub cache: Option<DidCache>,
}

impl DidPlcResolver {
    pub fn new(plc_url: String, timeout: Duration, cache: Option<DidCache>) -> Self {
        Self {
            plc_url,
            timeout,
            cache,
        }
    }

    pub async fn resolve_no_check(&self, did: String) -> Result<Option<Value>> {
        let client = reqwest::Client::new();
        let response = client
            .get(format!("{0}/{1}", self.plc_url, encode_uri_component(&did)))
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
