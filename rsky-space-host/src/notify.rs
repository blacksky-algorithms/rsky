//! Outbound write / deletion notifications (spec §Write notifications,
//! §Space deletion). The authority only routes notifications; delivery is
//! best-effort and failures are logged, never retried inline.

use async_trait::async_trait;
use rsky_lexicon::com::atproto::space::{NotifySpaceDeletedInput, NotifyWriteInput};
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;

use crate::error::{HostError, Result};
use crate::service_jwt;
use crate::signing::Signer;
use crate::store::Subscriber;

pub const NOTIFY_WRITE_LXM: &str = "com.atproto.space.notifyWrite";
pub const NOTIFY_SPACE_DELETED_LXM: &str = "com.atproto.space.notifySpaceDeleted";
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Delivers a single notification to a single registered endpoint.
#[async_trait]
pub trait Notifier: Send + Sync {
    async fn notify_write(&self, to: &Subscriber, input: &NotifyWriteInput) -> Result<()>;
    async fn notify_space_deleted(
        &self,
        to: &Subscriber,
        input: &NotifySpaceDeletedInput,
    ) -> Result<()>;
}

/// HTTP [`Notifier`]: POSTs the XRPC procedure to the subscriber's endpoint
/// with a service-auth JWT signed by the authority, addressed to the
/// subscriber's service identifier. A registration made before proposals#100
/// names no identifier, and its deliveries fall back to addressing the
/// endpoint URL -- which no receiver can verify itself against, which is why
/// the amendment exists.
pub struct HttpNotifier {
    authority_did: String,
    signer: Signer,
    http: reqwest::Client,
    now: Arc<dyn Fn() -> u64 + Send + Sync>,
    jti: Arc<dyn Fn() -> String + Send + Sync>,
}

impl HttpNotifier {
    pub fn new(
        authority_did: String,
        signer: Signer,
        now: Arc<dyn Fn() -> u64 + Send + Sync>,
        jti: Arc<dyn Fn() -> String + Send + Sync>,
    ) -> Self {
        Self::with_timeout(authority_did, signer, now, jti, DEFAULT_TIMEOUT)
    }

    pub fn with_timeout(
        authority_did: String,
        signer: Signer,
        now: Arc<dyn Fn() -> u64 + Send + Sync>,
        jti: Arc<dyn Fn() -> String + Send + Sync>,
        timeout: Duration,
    ) -> Self {
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .expect("reqwest client");
        Self {
            authority_did,
            signer,
            http,
            now,
            jti,
        }
    }

    async fn post<T: Serialize + Sync>(&self, to: &Subscriber, lxm: &str, input: &T) -> Result<()> {
        let token = service_jwt::mint(
            &self.signer,
            &self.authority_did,
            to.audience(),
            lxm,
            (self.now)(),
            (self.jti)(),
        )?;
        let endpoint = to.endpoint.as_str();
        let url = format!("{}/xrpc/{lxm}", endpoint.trim_end_matches('/'));
        let response = self
            .http
            .post(url)
            .bearer_auth(token)
            .json(input)
            .send()
            .await
            .map_err(|e| HostError::Store(e.to_string()))?;
        if !response.status().is_success() {
            return Err(HostError::Store(format!(
                "{endpoint} returned {}",
                response.status()
            )));
        }
        Ok(())
    }
}

#[async_trait]
impl Notifier for HttpNotifier {
    async fn notify_write(&self, to: &Subscriber, input: &NotifyWriteInput) -> Result<()> {
        self.post(to, NOTIFY_WRITE_LXM, input).await
    }

    async fn notify_space_deleted(
        &self,
        to: &Subscriber,
        input: &NotifySpaceDeletedInput,
    ) -> Result<()> {
        self.post(to, NOTIFY_SPACE_DELETED_LXM, input).await
    }
}

/// Fan a write notification out to every registered endpoint as detached
/// best-effort tasks.
pub fn fan_out_write(
    notifier: Arc<dyn Notifier>,
    subscribers: Vec<Subscriber>,
    input: NotifyWriteInput,
) {
    for subscriber in subscribers {
        let notifier = notifier.clone();
        let input = input.clone();
        tokio::spawn(async move {
            if let Err(e) = notifier.notify_write(&subscriber, &input).await {
                tracing::warn!(endpoint = subscriber.endpoint, error = %e,
                    "notifyWrite delivery failed");
            }
        });
    }
}

/// Broadcast a space deletion to syncers/repo hosts, best-effort (outbound
/// helper for the authority; the inbound syncer handler is not this crate).
pub async fn broadcast_space_deleted(
    notifier: &dyn Notifier,
    subscribers: &[Subscriber],
    space: &str,
) {
    let input = NotifySpaceDeletedInput {
        space: space.to_string(),
    };
    for subscriber in subscribers {
        if let Err(e) = notifier.notify_space_deleted(subscriber, &input).await {
            tracing::warn!(endpoint = subscriber.endpoint, error = %e,
                "notifySpaceDeleted delivery failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signing::test_signer;
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn to(endpoint: impl Into<String>) -> Subscriber {
        Subscriber {
            endpoint: endpoint.into(),
            service: None,
        }
    }

    const SPACE: &str = "at://did:plc:auth/space/community.blacksky.feed/main";

    fn notifier() -> HttpNotifier {
        HttpNotifier::new(
            "did:plc:auth".to_string(),
            test_signer(),
            Arc::new(|| 1000),
            Arc::new(|| "jti-n".to_string()),
        )
    }

    fn write_input() -> NotifyWriteInput {
        NotifyWriteInput {
            space: SPACE.to_string(),
            repo: "did:plc:writer".to_string(),
            rev: "3jzfcijpj2z2c".to_string(),
        }
    }

    #[tokio::test]
    async fn posts_signed_notify_write() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(format!("/xrpc/{NOTIFY_WRITE_LXM}")))
            .and(body_json(serde_json::json!({
                "space": SPACE,
                "repo": "did:plc:writer",
                "rev": "3jzfcijpj2z2c",
            })))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        notifier()
            .notify_write(&to(server.uri()), &write_input())
            .await
            .unwrap();

        let requests = server.received_requests().await.unwrap();
        let auth = requests[0].headers.get("authorization").unwrap();
        let jwt = auth.to_str().unwrap().strip_prefix("Bearer ").unwrap();
        let claims = service_jwt::verify(
            jwt,
            &[server.uri().as_str()],
            NOTIFY_WRITE_LXM,
            test_signer().did_key(),
            1000,
        )
        .unwrap();
        assert_eq!(claims.iss, "did:plc:auth");
    }

    #[tokio::test]
    async fn posts_notify_space_deleted_and_broadcasts() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(format!("/xrpc/{NOTIFY_SPACE_DELETED_LXM}")))
            .and(body_json(serde_json::json!({"space": SPACE})))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        let n = notifier();
        // One reachable endpoint, one failing: both are attempted.
        broadcast_space_deleted(&n, &[to(server.uri()), to("http://127.0.0.1:1")], SPACE).await;
    }

    #[tokio::test]
    async fn non_200_and_transport_failures_are_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let res = notifier()
            .notify_write(&to(server.uri()), &write_input())
            .await;
        assert!(matches!(res, Err(HostError::Store(msg)) if msg.contains("500")));

        let res = notifier()
            .notify_write(&to("http://127.0.0.1:1"), &write_input())
            .await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn fan_out_write_delivers_to_all_endpoints_best_effort() {
        struct Recording {
            tx: tokio::sync::mpsc::UnboundedSender<String>,
            fail: bool,
        }
        #[async_trait]
        impl Notifier for Recording {
            async fn notify_write(&self, to: &Subscriber, _input: &NotifyWriteInput) -> Result<()> {
                self.tx.send(to.endpoint.clone()).unwrap();
                if self.fail {
                    Err(HostError::Store("down".into()))
                } else {
                    Ok(())
                }
            }
            async fn notify_space_deleted(
                &self,
                _to: &Subscriber,
                _input: &NotifySpaceDeletedInput,
            ) -> Result<()> {
                Ok(())
            }
        }

        for fail in [false, true] {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let notifier = Arc::new(Recording { tx, fail });
            notifier
                .notify_space_deleted(
                    &to("https://a.example"),
                    &NotifySpaceDeletedInput {
                        space: SPACE.to_string(),
                    },
                )
                .await
                .unwrap();
            fan_out_write(
                notifier,
                vec![to("https://a.example"), to("https://b.example")],
                write_input(),
            );
            let mut seen = vec![rx.recv().await.unwrap(), rx.recv().await.unwrap()];
            seen.sort();
            assert_eq!(
                seen,
                vec![
                    "https://a.example".to_string(),
                    "https://b.example".to_string()
                ]
            );
        }
    }
}
