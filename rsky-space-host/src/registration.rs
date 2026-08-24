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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pds_seam::{test_actor_store, PdsSeam, PdsServiceJwtIssuer};
    use crate::service_jwt;
    use crate::signing::test_signer;
    use std::sync::Arc;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const AUTHORITY: &str = "did:plc:auth";
    const FEEDS: &str = "did:web:feeds.test";
    const SPACE: &str = "at://did:plc:auth/space/community.blacksky.feed/main";

    #[tokio::test]
    async fn ack_is_signed_with_the_authority_account_key() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(format!("/xrpc/{ACK_HOST_REGISTERED_LXM}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&server)
            .await;

        let secret = [11u8; 32];
        let directory = test_actor_store(AUTHORITY, secret);
        let seam = Arc::new(PdsSeam::open(directory.path()).unwrap());
        let acker = HttpLifecycleAcker::with_issuer(
            server.uri(),
            FEEDS,
            Arc::new(PdsServiceJwtIssuer::new(seam, AUTHORITY.to_string())),
            Arc::new(|| 1000),
            Arc::new(|| "jti-fixed".to_string()),
        );
        acker.ack_host_registered(SPACE, 1).await.unwrap();

        let requests = server.received_requests().await.unwrap();
        let auth = requests[0].headers.get("authorization").unwrap();
        let jwt = auth.to_str().unwrap().strip_prefix("Bearer ").unwrap();

        let account_key =
            crate::signing::Signer::from_secret(secp256k1::SecretKey::from_slice(&secret).unwrap());
        let claims = service_jwt::verify(
            jwt,
            &[FEEDS],
            ACK_HOST_REGISTERED_LXM,
            account_key.did_key(),
            1000,
        )
        .unwrap();
        assert_eq!(claims.iss, AUTHORITY);

        // The space key must not verify it.
        assert!(service_jwt::verify(
            jwt,
            &[FEEDS],
            ACK_HOST_REGISTERED_LXM,
            test_signer().did_key(),
            1000,
        )
        .is_err());
    }

    #[tokio::test]
    async fn a_rejected_ack_is_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let directory = test_actor_store(AUTHORITY, [11u8; 32]);
        let seam = Arc::new(PdsSeam::open(directory.path()).unwrap());
        let acker = HttpLifecycleAcker::with_issuer(
            server.uri(),
            FEEDS,
            Arc::new(PdsServiceJwtIssuer::new(seam, AUTHORITY.to_string())),
            Arc::new(|| 1000),
            Arc::new(|| "jti-fixed".to_string()),
        );
        let error = acker.ack_host_registered(SPACE, 1).await.unwrap_err();
        assert!(matches!(error, HostError::ManagingApp(msg) if msg.contains("401")));
    }
}
