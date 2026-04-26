use crate::graph::FollowGraph;
use crate::metrics;
use bytes::Bytes;
use http_body_util::Full;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::net::TcpListener;

pub async fn serve(
    port: u16,
    graph: Arc<FollowGraph>,
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
        tokio::spawn(async move {
            let service = service_fn(move |req: Request<hyper::body::Incoming>| {
                let graph = Arc::clone(&graph);
                async move { handle_request(req, &graph).await }
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

async fn handle_request(
    req: Request<hyper::body::Incoming>,
    graph: &FollowGraph,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let path = req.uri().path();

    match path {
        "/_health" => Ok(Response::builder()
            .status(StatusCode::OK)
            .body(Full::new(Bytes::from("ok")))
            .unwrap()),

        "/metrics" => {
            let body = metrics::encode_metrics();
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "text/plain; version=0.0.4")
                .body(Full::new(Bytes::from(body)))
                .unwrap())
        }

        "/v1/follows-following" => {
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
                return Ok(Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(
                        r#"{"error":"viewer and targets required"}"#,
                    )))
                    .unwrap());
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

            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .header("X-Query-Time-Ms", elapsed.as_millis().to_string())
                .body(Full::new(Bytes::from(body.to_string())))
                .unwrap())
        }

        "/v1/is-following" => {
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

            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(body.to_string())))
                .unwrap())
        }

        _ => Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Full::new(Bytes::from("Not Found")))
            .unwrap()),
    }
}
