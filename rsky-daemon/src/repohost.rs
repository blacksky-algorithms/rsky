//! Client for a member's permissioned repo host (their PDS): the oplog
//! (`listRepoOps`), full-state CARs (`getRepo`), and the current signed commit
//! (`getLatestCommit`), all authenticated with a space credential.

use async_trait::async_trait;
use rsky_lexicon::com::atproto::space as wire;
use rsky_space::types::{RepoOp, SignedCommit};
use serde_bytes::ByteBuf;

use crate::error::Result;
use crate::xrpc::{check, http_client, net_err};

/// A page of the operation log, plus the current signed commit when the page
/// reaches the repo head (proposal §Incremental sync).
#[derive(Debug)]
pub struct OplogPage {
    pub ops: Vec<RepoOp>,
    /// Present iff this page includes the last available op — lets the syncer
    /// authenticate the resulting state against the signed commit.
    pub commit: Option<SignedCommit>,
    /// Continuation cursor when more pages remain.
    pub cursor: Option<String>,
}

/// Reads permissioned repos from their hosts.
#[async_trait]
pub trait RepoHostClient: Send + Sync {
    /// One `listRepoOps` page since a revision (inlining record values).
    async fn list_repo_ops(
        &self,
        space: &str,
        did: &str,
        since: Option<&str>,
        cursor: Option<&str>,
    ) -> Result<OplogPage>;
    /// The whole repo as a serialized CAR (`getRepo`), for full-state recovery.
    async fn get_repo_car(&self, space: &str, did: &str) -> Result<Vec<u8>>;
    /// The repo's current signed commit (`getLatestCommit`).
    async fn get_latest_commit(&self, space: &str, did: &str) -> Result<SignedCommit>;
}

// The wire types (rsky-lexicon, `$bytes`/JSON values) and the internal types
// (rsky-space, serde_bytes) are both foreign, so the conversions live here as
// functions rather than `From` impls.
pub(crate) fn commit_from_wire(w: wire::SignedCommit) -> SignedCommit {
    SignedCommit {
        ver: w.ver as u8,
        hash: ByteBuf::from(w.hash),
        ikm: ByteBuf::from(w.ikm),
        sig: ByteBuf::from(w.sig),
        mac: ByteBuf::from(w.mac),
        rev: w.rev,
    }
}

pub(crate) fn op_from_wire(w: wire::RepoOp) -> RepoOp {
    // serde_json::Value serialization cannot fail (string keys, no NaN).
    let value = w
        .value
        .map(|v| ByteBuf::from(serde_json::to_vec(&v).expect("Value serializes")));
    RepoOp {
        rev: w.rev,
        collection: w.collection,
        rkey: w.rkey,
        cid: w.cid,
        prev: w.prev,
        value,
    }
}

pub struct HttpRepoHost {
    base_url: String,
    credential: String,
    http: reqwest::Client,
}

impl HttpRepoHost {
    pub fn new(base_url: impl Into<String>, credential: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            credential: credential.into(),
            http: http_client(),
        }
    }

    fn url(&self, nsid: &str) -> String {
        format!("{}/xrpc/{nsid}", self.base_url)
    }

    async fn get(&self, nsid: &str, query: &[(&str, &str)]) -> Result<reqwest::Response> {
        let resp = self
            .http
            .get(self.url(nsid))
            .bearer_auth(&self.credential)
            .query(query)
            .send()
            .await
            .map_err(net_err)?;
        check(resp).await
    }
}

#[async_trait]
impl RepoHostClient for HttpRepoHost {
    async fn list_repo_ops(
        &self,
        space: &str,
        did: &str,
        since: Option<&str>,
        cursor: Option<&str>,
    ) -> Result<OplogPage> {
        let mut query = vec![("space", space), ("did", did)];
        if let Some(since) = since {
            query.push(("since", since));
        }
        if let Some(cursor) = cursor {
            query.push(("cursor", cursor));
        }
        let out: wire::ListRepoOpsOutput = self
            .get("com.atproto.space.listRepoOps", &query)
            .await?
            .json()
            .await
            .map_err(net_err)?;
        Ok(OplogPage {
            ops: out.ops.into_iter().map(op_from_wire).collect(),
            commit: out.commit.map(commit_from_wire),
            cursor: out.cursor,
        })
    }

    async fn get_repo_car(&self, space: &str, did: &str) -> Result<Vec<u8>> {
        let resp = self
            .get(
                "com.atproto.space.getRepo",
                &[("space", space), ("did", did)],
            )
            .await?;
        Ok(resp.bytes().await.map_err(net_err)?.to_vec())
    }

    async fn get_latest_commit(&self, space: &str, did: &str) -> Result<SignedCommit> {
        let out: wire::GetLatestCommitOutput = self
            .get(
                "com.atproto.space.getLatestCommit",
                &[("space", space), ("did", did)],
            )
            .await?
            .json()
            .await
            .map_err(net_err)?;
        Ok(commit_from_wire(out.commit))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::DaemonError;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const SPACE: &str = "at://did:plc:authority/space/community.blacksky.feed/main";
    const AUTHOR: &str = "did:plc:author";

    fn wire_commit_json() -> serde_json::Value {
        serde_json::json!({
            "ver": 1,
            "hash": {"$bytes": "AQID"},
            "ikm": {"$bytes": "BAUG"},
            "sig": {"$bytes": "BwgJ"},
            "mac": {"$bytes": "CgsM"},
            "rev": "3krev"
        })
    }

    fn expected_commit() -> SignedCommit {
        SignedCommit {
            ver: 1,
            hash: ByteBuf::from(vec![1, 2, 3]),
            ikm: ByteBuf::from(vec![4, 5, 6]),
            sig: ByteBuf::from(vec![7, 8, 9]),
            mac: ByteBuf::from(vec![10, 11, 12]),
            rev: "3krev".to_string(),
        }
    }

    #[tokio::test]
    async fn list_repo_ops_paginates_and_converts_wire_types() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/xrpc/com.atproto.space.listRepoOps"))
            .and(query_param("space", SPACE))
            .and(query_param("did", AUTHOR))
            .and(query_param("since", "3ka"))
            .and(query_param("cursor", "c1"))
            .and(header("authorization", "Bearer sc.jwt"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ops": [{
                    "rev": "3krev",
                    "collection": "community.blacksky.feed.post",
                    "rkey": "3kb",
                    "prev": "bafyOld"
                }],
                "commit": wire_commit_json()
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/xrpc/com.atproto.space.listRepoOps"))
            .and(query_param("since", "3ka"))
            .and(header("authorization", "Bearer sc.jwt"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "cursor": "c1",
                "ops": [{
                    "rev": "3krev",
                    "collection": "community.blacksky.feed.post",
                    "rkey": "3ka",
                    "cid": "bafyA",
                    "value": {"text": "hi"}
                }]
            })))
            .mount(&server)
            .await;

        let host = HttpRepoHost::new(format!("{}/", server.uri()), "sc.jwt");
        let first = host
            .list_repo_ops(SPACE, AUTHOR, Some("3ka"), None)
            .await
            .unwrap();
        assert_eq!(first.cursor.as_deref(), Some("c1"));
        assert!(first.commit.is_none());
        assert_eq!(first.ops[0].cid.as_deref(), Some("bafyA"));
        assert_eq!(
            first.ops[0].value.as_ref().unwrap().as_ref(),
            br#"{"text":"hi"}"#
        );

        let last = host
            .list_repo_ops(SPACE, AUTHOR, Some("3ka"), Some("c1"))
            .await
            .unwrap();
        assert_eq!(last.cursor, None);
        assert_eq!(last.commit.unwrap(), expected_commit());
        assert!(last.ops[0].is_delete());
        assert_eq!(last.ops[0].value, None);
    }

    #[tokio::test]
    async fn history_unavailable_maps_to_distinct_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/xrpc/com.atproto.space.listRepoOps"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": "HistoryUnavailable",
                "message": "oplog compacted"
            })))
            .mount(&server)
            .await;

        let host = HttpRepoHost::new(server.uri(), "sc.jwt");
        let err = host
            .list_repo_ops(SPACE, AUTHOR, Some("3ka"), None)
            .await
            .unwrap_err();
        assert!(matches!(err, DaemonError::HistoryUnavailable(m) if m == "oplog compacted"));
    }

    #[tokio::test]
    async fn history_unavailable_without_message_uses_default() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/xrpc/com.atproto.space.listRepoOps"))
            .respond_with(
                ResponseTemplate::new(400)
                    .set_body_json(serde_json::json!({"error": "HistoryUnavailable"})),
            )
            .mount(&server)
            .await;

        let host = HttpRepoHost::new(server.uri(), "sc.jwt");
        let err = host
            .list_repo_ops(SPACE, AUTHOR, None, None)
            .await
            .unwrap_err();
        assert!(matches!(err, DaemonError::HistoryUnavailable(m) if m.contains("oplog window")));
    }

    #[tokio::test]
    async fn get_repo_car_returns_raw_bytes() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/xrpc/com.atproto.space.getRepo"))
            .and(query_param("space", SPACE))
            .and(query_param("did", AUTHOR))
            .and(header("authorization", "Bearer sc.jwt"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![0xCAu8, 0x11]))
            .mount(&server)
            .await;

        let host = HttpRepoHost::new(server.uri(), "sc.jwt");
        let car = host.get_repo_car(SPACE, AUTHOR).await.unwrap();
        assert_eq!(car, vec![0xCAu8, 0x11]);
    }

    #[tokio::test]
    async fn get_latest_commit_converts_wire_commit() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/xrpc/com.atproto.space.getLatestCommit"))
            .and(query_param("space", SPACE))
            .and(query_param("did", AUTHOR))
            .and(header("authorization", "Bearer sc.jwt"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"commit": wire_commit_json()})),
            )
            .mount(&server)
            .await;

        let host = HttpRepoHost::new(server.uri(), "sc.jwt");
        let commit = host.get_latest_commit(SPACE, AUTHOR).await.unwrap();
        assert_eq!(commit, expected_commit());
    }

    #[tokio::test]
    async fn malformed_success_body_maps_to_xrpc_variant() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/xrpc/com.atproto.space.getLatestCommit"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
            .mount(&server)
            .await;

        let host = HttpRepoHost::new(server.uri(), "sc.jwt");
        let err = host.get_latest_commit(SPACE, AUTHOR).await.unwrap_err();
        assert!(matches!(err, DaemonError::Xrpc(_)));
    }
}
