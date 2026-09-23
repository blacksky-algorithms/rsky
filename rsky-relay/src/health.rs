use std::net::SocketAddr;
use std::sync::atomic::Ordering;

use http_body_util::Full;
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use metrics_exporter_prometheus::PrometheusHandle;
use tokio::net::TcpListener;

use crate::config::HEAD_STALL_SECONDS;
use crate::{HEAD_SEQ, HEAD_TIME, Health, health_snapshot, metrics, unix_now};

/// JSON body of `/_health`; `status` is `ok` only when the validator is live and
/// its head is still moving (or its ring is empty).
#[must_use]
pub fn health_body() -> (StatusCode, String) {
    let (health, stalled) = health_snapshot(HEAD_STALL_SECONDS);
    let status = match (health, stalled) {
        (Health::Live, false) => "ok",
        (Health::Live, true) => "stalled",
        (other, _) => other.as_str(),
    };
    let code = if status == "ok" { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
    let head_time = HEAD_TIME.load(Ordering::Relaxed);
    let head_age = if head_time == 0 {
        None
    } else {
        Some(i64::try_from(unix_now()).unwrap_or(i64::MAX).saturating_sub(head_time))
    };
    let body = serde_json::json!({
        "status": status,
        "validator": health.as_str(),
        "version": env!("CARGO_PKG_VERSION"),
        "headSeq": HEAD_SEQ.load(Ordering::Relaxed),
        "headAgeSeconds": head_age,
    });
    (code, body.to_string())
}

fn head_age_seconds() -> f64 {
    let head_time = HEAD_TIME.load(Ordering::Relaxed);
    if head_time == 0 {
        return 0.0;
    }
    #[expect(clippy::cast_precision_loss)]
    let age = i64::try_from(unix_now()).unwrap_or(i64::MAX).saturating_sub(head_time) as f64;
    age
}

/// Renders `/metrics` with the head-age gauge computed at scrape time, so a
/// frozen validator cannot leave a stale small value behind.
#[must_use]
pub fn metrics_body(handle: &PrometheusHandle) -> String {
    metrics::record_firehose_head_age(head_age_seconds());
    handle.render()
}

#[must_use]
pub fn respond(handle: &PrometheusHandle, path: &str) -> (StatusCode, &'static str, String) {
    match path {
        "/metrics" => {
            (StatusCode::OK, "text/plain; version=0.0.4; charset=utf-8", metrics_body(handle))
        }
        "/_health" | "/xrpc/_health" => {
            let (code, body) = health_body();
            (code, "application/json", body)
        }
        _ => (StatusCode::NOT_FOUND, "application/json", "{\"error\":\"NotFound\"}".to_owned()),
    }
}

fn respond_to(
    handle: &PrometheusHandle, req: &Request<hyper::body::Incoming>,
) -> std::future::Ready<Result<Response<Full<Bytes>>, std::convert::Infallible>> {
    let (code, content_type, body) = respond(handle, req.uri().path());
    #[expect(clippy::unwrap_used)]
    let response = Response::builder()
        .status(code)
        .header("Content-Type", content_type)
        .header("Cache-Control", "no-store")
        .body(Full::new(Bytes::from(body)))
        .unwrap();
    std::future::ready(Ok(response))
}

/// Serves `/metrics` and `/_health` on the shared runtime, independent of the
/// validator thread and the mio server's accept loop.
pub async fn serve(addr: SocketAddr, handle: PrometheusHandle) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    tracing::info!(%addr, "health listener bound");
    loop {
        let (stream, _) = listener.accept().await?;
        let handle = handle.clone();
        tokio::spawn(async move {
            let io = TokioIo::new(stream);
            let service = service_fn(move |req| respond_to(&handle, &req));
            if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                tracing::debug!(%err, "health connection error");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HEAD_PROGRESS_AT, set_health};
    use metrics_exporter_prometheus::PrometheusBuilder;
    use std::sync::Mutex;

    static GUARD: Mutex<()> = Mutex::new(());

    fn guard() -> std::sync::MutexGuard<'static, ()> {
        GUARD.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[test]
    fn health_body_reflects_states() {
        let _g = guard();
        set_health(Health::Starting);
        let (code, body) = health_body();
        assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
        assert!(body.contains("\"status\":\"starting\""));

        set_health(Health::Live);
        HEAD_PROGRESS_AT.store(unix_now(), Ordering::Relaxed);
        HEAD_TIME.store(0, Ordering::Relaxed);
        let (code, body) = health_body();
        assert_eq!(code, StatusCode::OK);
        assert!(body.contains("\"status\":\"ok\""));
        assert!(body.contains("\"headAgeSeconds\":null"));

        HEAD_PROGRESS_AT.store(unix_now() - HEAD_STALL_SECONDS - 5, Ordering::Relaxed);
        HEAD_TIME.store(i64::try_from(unix_now()).unwrap() - 120, Ordering::Relaxed);
        let (code, body) = health_body();
        assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
        assert!(body.contains("\"status\":\"stalled\""));
        assert!(body.contains("\"headAgeSeconds\":12"));

        set_health(Health::Dead);
        let (code, body) = health_body();
        assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
        assert!(body.contains("\"status\":\"dead\""));
        set_health(Health::Starting);
    }

    #[test]
    fn respond_routes_paths() {
        let _g = guard();
        let handle = PrometheusBuilder::new().build_recorder().handle();
        HEAD_TIME.store(0, Ordering::Relaxed);
        let (code, ct, _) = respond(&handle, "/metrics");
        assert_eq!(code, StatusCode::OK);
        assert!(ct.starts_with("text/plain"));
        HEAD_TIME.store(i64::try_from(unix_now()).unwrap() - 3, Ordering::Relaxed);
        let (code, _, _) = respond(&handle, "/metrics");
        assert_eq!(code, StatusCode::OK);
        assert!(head_age_seconds() >= 3.0);
        let (_, ct, body) = respond(&handle, "/xrpc/_health");
        assert_eq!(ct, "application/json");
        assert!(body.contains("\"version\""));
        let (code, _, body) = respond(&handle, "/nope");
        assert_eq!(code, StatusCode::NOT_FOUND);
        assert!(body.contains("NotFound"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn serve_answers_http_requests() {
        let handle = PrometheusBuilder::new().build_recorder().handle();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let server = tokio::spawn(serve(addr, handle));
        let mut body = String::new();
        for _ in 0..50 {
            match tokio::net::TcpStream::connect(addr).await {
                Ok(mut stream) => {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    stream
                        .write_all(
                            b"GET /xrpc/_health HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
                        )
                        .await
                        .unwrap();
                    stream.read_to_string(&mut body).await.unwrap();
                    break;
                }
                Err(_) => tokio::time::sleep(std::time::Duration::from_millis(10)).await,
            }
        }
        assert!(body.contains("HTTP/1.1"), "{body}");
        assert!(body.contains("\"validator\""), "{body}");
        server.abort();
        // binding an occupied port surfaces the io error path
        let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let taken = occupied.local_addr().unwrap();
        let err = serve(taken, PrometheusBuilder::new().build_recorder().handle()).await;
        assert!(err.is_err());
    }
}
