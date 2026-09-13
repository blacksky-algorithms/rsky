extern crate url;

pub mod safe_fetch;

use crate::did::did_resolver::DidResolver;
use crate::handle::HandleResolver;
use crate::types::{DidResolverOpts, HandleResolverOpts, IdentityResolverOpts, MemoryCache};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct IdResolver {
    pub handle: HandleResolver,
    pub did: DidResolver,
}

impl IdResolver {
    pub fn new(opts: IdentityResolverOpts) -> Self {
        let IdentityResolverOpts {
            timeout,
            plc_url,
            did_cache,
            backup_nameservers,
        } = opts;
        let timeout = timeout.unwrap_or_else(|| Duration::from_millis(3000));
        let did_cache = did_cache.unwrap_or_else(|| {
            Arc::new(MemoryCache::new(
                Some(Duration::default()),
                Some(Duration::default()),
            ))
        });

        Self {
            handle: HandleResolver::new(HandleResolverOpts {
                timeout: Some(timeout),
                backup_nameservers,
            }),
            did: DidResolver::new(DidResolverOpts {
                timeout: Some(timeout),
                plc_url,
                did_cache,
            }),
        }
    }

    /// Fetches handle and `did:web` documents under `policy` instead of the
    /// public default.
    pub fn with_network(self, policy: safe_fetch::NetworkPolicy) -> Self {
        Self {
            handle: self.handle.with_network(policy),
            did: self.did.with_network(policy),
        }
    }
}

pub mod common;
pub mod did;
pub mod errors;
pub mod handle;
pub mod types;

#[cfg(test)]
mod network_tests {
    use super::*;
    use crate::safe_fetch::NetworkPolicy;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Serves one fixed response on loopback.
    async fn serve(status: u16, body: &'static str) -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = vec![0u8; 4096];
                let _ = socket.read(&mut buf).await;
                let response = format!(
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });
        port
    }

    fn resolver(policy: NetworkPolicy) -> IdResolver {
        IdResolver::new(IdentityResolverOpts {
            timeout: Some(Duration::from_secs(5)),
            plc_url: Some("http://127.0.0.1:1".to_string()),
            did_cache: None,
            backup_nameservers: None,
        })
        .with_network(policy)
    }

    #[tokio::test]
    async fn did_web_documents_come_through_the_bound_transport() {
        let port = serve(
            200,
            r#"{"id":"did:web:localhost","alsoKnownAs":["at://alice.test"]}"#,
        )
        .await;
        let did = format!("did:web:localhost%3A{port}");
        // refused outright under the public policy: loopback name
        let public = resolver(NetworkPolicy::PUBLIC);
        assert!(public.did.resolve_no_check(did.clone()).await.is_err());
        let local = resolver(NetworkPolicy::PERMISSIVE);
        let doc = local.did.resolve_no_check(did).await.unwrap().unwrap();
        assert_eq!(doc["id"], "did:web:localhost");
        let missing = serve(404, "").await;
        assert!(local
            .did
            .resolve_no_check(format!("did:web:localhost%3A{missing}"))
            .await
            .unwrap()
            .is_none());
        let broken = serve(500, "").await;
        let err = local
            .did
            .resolve_no_check(format!("did:web:localhost%3A{broken}"))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("500"), "{err}");
    }

    #[tokio::test]
    async fn handle_well_known_documents_come_through_the_bound_transport() {
        let port = serve(200, "did:plc:alice\n").await;
        let handle = format!("localhost:{port}");
        let public = resolver(NetworkPolicy::PUBLIC);
        assert!(public.handle.resolve_http(&handle).await.is_err());
        let local = resolver(NetworkPolicy::PERMISSIVE);
        assert_eq!(
            local.handle.resolve_http(&handle).await.unwrap().as_deref(),
            Some("did:plc:alice")
        );
        let junk = serve(200, "not a did").await;
        assert!(local
            .handle
            .resolve_http(&format!("localhost:{junk}"))
            .await
            .unwrap()
            .is_none());
        let empty = serve(200, "").await;
        assert!(local
            .handle
            .resolve_http(&format!("localhost:{empty}"))
            .await
            .unwrap()
            .is_none());
    }
}
