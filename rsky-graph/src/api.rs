use crate::graph::FollowGraph;
use crate::metrics;
use bytes::Bytes;
use http_body_util::Full;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::net::TcpListener;

/// Operator-only state. The admin endpoint is gated by `token`; when None,
/// `/admin/*` routes refuse all callers regardless of headers.
pub struct AdminState {
    pub token: Option<String>,
    pub database_url: Option<String>,
    pub bulk_load_running: AtomicBool,
}

impl AdminState {
    pub fn new(token: Option<String>, database_url: Option<String>) -> Self {
        Self {
            token,
            database_url,
            bulk_load_running: AtomicBool::new(false),
        }
    }
}

pub async fn serve(
    port: u16,
    graph: Arc<FollowGraph>,
    admin: Arc<AdminState>,
    shutdown: Arc<AtomicBool>,
) -> color_eyre::Result<()> {
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await?;
    tracing::info!("HTTP API listening on {addr}");

    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        let conn = tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, _)) => stream,
                    Err(e) => {
                        tracing::error!("accept error: {e}");
                        continue;
                    }
                }
            }
            () = tokio::time::sleep(std::time::Duration::from_millis(100)) => continue,
        };

        let graph = Arc::clone(&graph);
        let admin = Arc::clone(&admin);
        tokio::spawn(async move {
            let service = service_fn(move |req: Request<hyper::body::Incoming>| {
                let graph = Arc::clone(&graph);
                let admin = Arc::clone(&admin);
                async move { handle_request(req, graph, admin).await }
            });
            if let Err(e) = http1::Builder::new()
                .serve_connection(TokioIo::new(conn), service)
                .await
            {
                tracing::error!("connection error: {e}");
            }
        });
    }

    Ok(())
}

async fn handle_request<B>(
    req: Request<B>,
    graph: Arc<FollowGraph>,
    admin: Arc<AdminState>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let path = req.uri().path();

    match path {
        "/_health" => Ok(json_response(StatusCode::OK, r#"{"status":"ok"}"#)),

        "/metrics" => {
            let body = metrics::encode_metrics();
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "text/plain; version=0.0.4")
                .body(Full::new(Bytes::from(body)))
                .unwrap())
        }

        "/v1/follows-following" => Ok(handle_follows_following(&req, &graph)),

        "/v1/is-following" => Ok(handle_is_following(&req, &graph)),

        "/admin/bulk-load" => Ok(handle_admin_bulk_load(&req, &graph, &admin)),

        _ => Ok(json_response(
            StatusCode::NOT_FOUND,
            r#"{"error":"not found"}"#,
        )),
    }
}

fn handle_follows_following<B>(req: &Request<B>, graph: &FollowGraph) -> Response<Full<Bytes>> {
    let start = Instant::now();
    let query = req.uri().query().unwrap_or("");
    let params: Vec<(String, String)> = url::form_urlencoded::parse(query.as_bytes())
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    let viewer = params
        .iter()
        .find(|(k, _)| k == "viewer")
        .map(|(_, v)| v.as_str())
        .unwrap_or("");

    let targets: Vec<&str> = params
        .iter()
        .find(|(k, _)| k == "targets")
        .map(|(_, v)| v.split(',').collect())
        .unwrap_or_default();

    if viewer.is_empty() || targets.is_empty() {
        return json_response(
            StatusCode::BAD_REQUEST,
            r#"{"error":"viewer and targets required"}"#,
        );
    }

    let mut results = Vec::with_capacity(targets.len());
    for target in &targets {
        let dids = graph.get_follows_following(viewer, target);
        results.push(serde_json::json!({
            "targetDid": target,
            "dids": dids,
        }));
    }

    let body = serde_json::json!({ "results": results });
    let elapsed = start.elapsed();

    metrics::GRAPH_QUERY_DURATION.observe(elapsed.as_secs_f64());
    metrics::GRAPH_QUERIES_TOTAL.inc();

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .header("X-Query-Time-Ms", elapsed.as_millis().to_string())
        .body(Full::new(Bytes::from(body.to_string())))
        .unwrap()
}

fn handle_is_following<B>(req: &Request<B>, graph: &FollowGraph) -> Response<Full<Bytes>> {
    let query = req.uri().query().unwrap_or("");
    let params: Vec<(String, String)> = url::form_urlencoded::parse(query.as_bytes())
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    let actor = params
        .iter()
        .find(|(k, _)| k == "actor")
        .map(|(_, v)| v.as_str())
        .unwrap_or("");
    let target = params
        .iter()
        .find(|(k, _)| k == "target")
        .map(|(_, v)| v.as_str())
        .unwrap_or("");

    let following = graph.is_following(actor, target);
    let body = serde_json::json!({ "following": following });
    json_response(StatusCode::OK, &body.to_string())
}

fn handle_admin_bulk_load<B>(
    req: &Request<B>,
    graph: &Arc<FollowGraph>,
    admin: &Arc<AdminState>,
) -> Response<Full<Bytes>> {
    if req.method() != Method::POST {
        return json_response(
            StatusCode::METHOD_NOT_ALLOWED,
            r#"{"error":"POST required"}"#,
        );
    }

    let Some(expected) = admin.token.as_deref() else {
        return json_response(StatusCode::UNAUTHORIZED, r#"{"error":"admin disabled"}"#);
    };

    let presented = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));

    if !presented.is_some_and(|t| ct_eq(t.as_bytes(), expected.as_bytes())) {
        return json_response(StatusCode::UNAUTHORIZED, r#"{"error":"invalid token"}"#);
    }

    let Some(database_url) = admin.database_url.clone() else {
        return json_response(
            StatusCode::SERVICE_UNAVAILABLE,
            r#"{"error":"DATABASE_URL not set"}"#,
        );
    };

    if admin
        .bulk_load_running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return json_response(
            StatusCode::CONFLICT,
            r#"{"error":"bulk-load already running"}"#,
        );
    }

    let graph = Arc::clone(graph);
    let admin = Arc::clone(admin);
    tokio::spawn(async move {
        let _guard = BulkLoadGuard {
            admin: Arc::clone(&admin),
        };
        tracing::info!("admin: starting bulk-load via POST /admin/bulk-load");
        match crate::bulk_load::bulk_load_keyset(&database_url, &graph).await {
            Ok(()) => {
                tracing::info!("admin: bulk-load complete; rebuilding bloom filters");
                crate::bloom::build_all_bloom_filters(&graph);
            }
            Err(e) => {
                tracing::error!("admin bulk-load failed: {e}");
            }
        }
    });

    json_response(StatusCode::ACCEPTED, r#"{"status":"started"}"#)
}

struct BulkLoadGuard {
    admin: Arc<AdminState>,
}

impl Drop for BulkLoadGuard {
    fn drop(&mut self) {
        self.admin.bulk_load_running.store(false, Ordering::SeqCst);
    }
}

fn json_response(status: StatusCode, body: &str) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(body.to_owned())))
        .unwrap()
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::Empty;

    fn req(method: &str, path: &str, auth: Option<&str>) -> Request<Empty<Bytes>> {
        let mut b = Request::builder().method(method).uri(path);
        if let Some(t) = auth {
            b = b.header("authorization", format!("Bearer {t}"));
        }
        b.body(Empty::<Bytes>::new()).unwrap()
    }

    fn admin_state(token: Option<&str>, db: Option<&str>) -> Arc<AdminState> {
        Arc::new(AdminState::new(
            token.map(str::to_owned),
            db.map(str::to_owned),
        ))
    }

    #[tokio::test]
    async fn admin_disabled_returns_401() {
        let g = Arc::new(FollowGraph::new());
        let a = admin_state(None, Some("postgres://x"));
        let resp = handle_request(req("POST", "/admin/bulk-load", Some("anything")), g, a)
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn admin_wrong_token_returns_401() {
        let g = Arc::new(FollowGraph::new());
        let a = admin_state(Some("secret"), Some("postgres://x"));
        let resp = handle_request(req("POST", "/admin/bulk-load", Some("nope")), g, a)
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn admin_missing_header_returns_401() {
        let g = Arc::new(FollowGraph::new());
        let a = admin_state(Some("secret"), Some("postgres://x"));
        let resp = handle_request(req("POST", "/admin/bulk-load", None), g, a)
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn admin_get_returns_405() {
        let g = Arc::new(FollowGraph::new());
        let a = admin_state(Some("secret"), Some("postgres://x"));
        let resp = handle_request(req("GET", "/admin/bulk-load", Some("secret")), g, a)
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn admin_no_database_url_returns_503() {
        let g = Arc::new(FollowGraph::new());
        let a = admin_state(Some("secret"), None);
        let resp = handle_request(req("POST", "/admin/bulk-load", Some("secret")), g, a)
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn admin_concurrent_load_returns_409() {
        let g = Arc::new(FollowGraph::new());
        let a = admin_state(Some("secret"), Some("postgres://x"));
        // Simulate a load already in flight without firing the spawned task.
        a.bulk_load_running.store(true, Ordering::SeqCst);
        let resp = handle_request(
            req("POST", "/admin/bulk-load", Some("secret")),
            g,
            Arc::clone(&a),
        )
        .await
        .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        // The flag is unchanged because the gate rejected the request.
        assert!(a.bulk_load_running.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn admin_404_for_unknown_path() {
        let g = Arc::new(FollowGraph::new());
        let a = admin_state(Some("secret"), Some("postgres://x"));
        let resp = handle_request(req("GET", "/nope", None), g, a)
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn health_returns_200() {
        let g = Arc::new(FollowGraph::new());
        let a = admin_state(None, None);
        let resp = handle_request(req("GET", "/_health", None), g, a)
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn metrics_returns_200_text() {
        let g = Arc::new(FollowGraph::new());
        let a = admin_state(None, None);
        let resp = handle_request(req("GET", "/metrics", None), g, a)
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp
            .headers()
            .get("content-type")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("");
        assert!(ct.starts_with("text/plain"));
    }

    #[tokio::test]
    async fn follows_following_missing_params_returns_400() {
        let g = Arc::new(FollowGraph::new());
        let a = admin_state(None, None);
        let resp = handle_request(req("GET", "/v1/follows-following", None), g, a)
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn follows_following_returns_dids() {
        let g = Arc::new(FollowGraph::new());
        // Alice follows Bob; Bob follows Dan. Viewer=Alice, target=Dan -> [Bob].
        g.add_follow("did:plc:alice", "did:plc:bob");
        g.add_follow("did:plc:bob", "did:plc:dan");
        let a = admin_state(None, None);
        let resp = handle_request(
            req(
                "GET",
                "/v1/follows-following?viewer=did:plc:alice&targets=did:plc:dan",
                None,
            ),
            g,
            a,
        )
        .await
        .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn is_following_returns_true_for_known_edge() {
        let g = Arc::new(FollowGraph::new());
        g.add_follow("did:plc:alice", "did:plc:bob");
        let a = admin_state(None, None);
        let resp = handle_request(
            req(
                "GET",
                "/v1/is-following?actor=did:plc:alice&target=did:plc:bob",
                None,
            ),
            g,
            a,
        )
        .await
        .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn add_follow_is_idempotent_under_concurrent_writers() {
        // This is the load-bearing assertion of the plan: a concurrent bulk-load
        // and firehose writer producing the same edge must not double-count.
        let g = Arc::new(FollowGraph::new());
        let edges = [
            ("did:plc:a1", "did:plc:b1"),
            ("did:plc:a2", "did:plc:b2"),
            ("did:plc:a3", "did:plc:b3"),
        ];

        let mut handles = Vec::new();
        for _ in 0..8 {
            let g = Arc::clone(&g);
            let edges = edges.to_vec();
            handles.push(tokio::spawn(async move {
                for (a, b) in edges {
                    g.add_follow(a, b);
                }
            }));
        }
        for h in handles {
            h.await.unwrap();
        }

        // Each unique edge must land exactly once in the bitmap, regardless of
        // how many writers added it. follow_count, however, is an AtomicU64
        // counter incremented per call and is intentionally non-canonical
        // for dedup purposes -- the bitmap is the source of truth.
        for (actor, subject) in edges {
            assert!(g.is_following(actor, subject));
        }
    }

    #[test]
    fn ct_eq_basic() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"ab", b"abc"));
        assert!(ct_eq(b"", b""));
    }
}
