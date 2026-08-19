use async_trait::async_trait;
use serde::Serialize;

use crate::error::{HostError, Result};
use crate::service_jwt::{ServiceJwtIssuer, SignerIssuer};
use crate::signing::Signer;

pub const REGISTER_SPACE_LXM: &str = "community.blacksky.space.register";
pub const ACK_HOST_REGISTERED_LXM: &str = "community.blacksky.space.ackHostRegistered";

#[async_trait]
pub trait LifecycleAcker: Send + Sync {
    async fn ack_host_registered(&self, space: &str, generation: i64) -> Result<()>;
}

pub struct HttpLifecycleAcker {
    base_url: String,
    audience: String,
    issuer: std::sync::Arc<dyn ServiceJwtIssuer>,
    now: std::sync::Arc<dyn Fn() -> u64 + Send + Sync>,
    jti: std::sync::Arc<dyn Fn() -> String + Send + Sync>,
    http: reqwest::Client,
}

impl HttpLifecycleAcker {
    pub fn new(
        base_url: impl Into<String>,
        audience: impl Into<String>,
        issuer: impl Into<String>,
        signer: Signer,
        now: std::sync::Arc<dyn Fn() -> u64 + Send + Sync>,
        jti: std::sync::Arc<dyn Fn() -> String + Send + Sync>,
    ) -> Self {
        Self::with_issuer(
            base_url,
            audience,
            std::sync::Arc::new(SignerIssuer::new(issuer, signer)),
            now,
            jti,
        )
    }

    pub fn with_issuer(
        base_url: impl Into<String>,
        audience: impl Into<String>,
        issuer: std::sync::Arc<dyn ServiceJwtIssuer>,
        now: std::sync::Arc<dyn Fn() -> u64 + Send + Sync>,
        jti: std::sync::Arc<dyn Fn() -> String + Send + Sync>,
    ) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            audience: audience.into(),
            issuer,
            now,
            jti,
            http: reqwest::Client::new(),
        }
    }
}

#[derive(Serialize)]
struct Ack<'a> {
    space: &'a str,
    generation: i64,
}

#[async_trait]
impl LifecycleAcker for HttpLifecycleAcker {
    async fn ack_host_registered(&self, space: &str, generation: i64) -> Result<()> {
        let token = self.issuer.mint(
            &self.audience,
            ACK_HOST_REGISTERED_LXM,
            (self.now)(),
            (self.jti)(),
        )?;
        let response = self
            .http
            .post(format!("{}/xrpc/{ACK_HOST_REGISTERED_LXM}", self.base_url))
            .bearer_auth(token)
            .json(&Ack { space, generation })
            .send()
            .await
            .map_err(|e| HostError::ManagingApp(e.to_string()))?;
        if !response.status().is_success() {
            return Err(HostError::ManagingApp(format!(
                "lifecycle acknowledgement returned {}",
                response.status()
            )));
        }
        Ok(())
    }
}
