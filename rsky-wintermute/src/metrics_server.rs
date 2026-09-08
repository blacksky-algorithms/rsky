//! The `/metrics` + `/_health` HTTP server shared by the daemon and the
//! backfill CLI.

use std::sync::atomic::Ordering;

use crate::SHUTDOWN;
use crate::metrics;
use crate::types::WintermuteError;

/// Serve `/metrics` and `/_health` on `port` (falling forward through the next
/// nine ports if it is taken) until shutdown. Blocks the calling thread on its
/// own current-thread runtime.
pub fn serve(port: u16) -> Result<(), WintermuteError> {
    use http_body_util::Full;
    use hyper::body::Bytes;
    use hyper::server::conn::http1;
    use hyper::service::service_fn;
    use hyper::{Request, Response};
    use hyper_util::rt::TokioIo;
    use std::net::SocketAddr;
    use tokio::net::TcpListener;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| WintermuteError::Other(format!("failed to create tokio runtime: {e}")))?;

    rt.block_on(async move {
        // Try up to 10 consecutive ports starting from the requested one
        let mut listener = None;
        let mut bound_port = port;
        for offset in 0..10 {
            let try_port = port.saturating_add(offset);
            let addr = SocketAddr::from(([0, 0, 0, 0], try_port));
            match TcpListener::bind(addr).await {
                Ok(l) => {
                    if offset > 0 {
                        tracing::warn!("port {port} in use, using alternate port {try_port}");
                    }
                    listener = Some(l);
                    bound_port = try_port;
                    break;
                }
                Err(e) if offset < 9 => {
                    tracing::debug!("port {try_port} unavailable: {e}");
                }
                Err(e) => {
                    return Err(WintermuteError::Other(format!(
                        "failed to bind metrics server on ports {port}-{try_port}: {e}"
                    )));
                }
            }
        }
        let Some(listener) = listener else {
            return Err(WintermuteError::Other("metrics: no listener bound".into()));
        };
        let addr = SocketAddr::from(([0, 0, 0, 0], bound_port));

        tracing::info!("metrics server listening on http://{addr} (endpoints: /metrics, /_health)");

        loop {
            if SHUTDOWN.load(Ordering::Relaxed) {
                tracing::info!("shutdown requested for metrics server");
                break;
            }

            let (stream, _) = tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok(conn) => conn,
                        Err(e) => {
                            tracing::error!("failed to accept connection: {e}");
                            continue;
                        }
                    }
                }
                () = tokio::time::sleep(std::time::Duration::from_millis(100)) => continue,
            };

            tokio::task::spawn(async move {
                let service = service_fn(move |req: Request<hyper::body::Incoming>| async move {
                    match req.uri().path() {
                        "/metrics" => match metrics::encode_metrics() {
                            Ok(body) => Ok::<_, WintermuteError>(
                                Response::builder()
                                    .status(200)
                                    .header("Content-Type", "text/plain; version=0.0.4")
                                    .body(Full::new(Bytes::from(body)))
                                    .map_err(|e| {
                                        WintermuteError::Other(format!(
                                            "failed to build response: {e}"
                                        ))
                                    })?,
                            ),
                            Err(e) => Ok(Response::builder()
                                .status(500)
                                .body(Full::new(Bytes::from(format!(
                                    "Error encoding metrics: {e}"
                                ))))
                                .map_err(|e| {
                                    WintermuteError::Other(format!("failed to build response: {e}"))
                                })?),
                        },
                        "/_health" => {
                            let shutting_down = SHUTDOWN.load(Ordering::Relaxed);
                            if shutting_down {
                                Ok(Response::builder()
                                    .status(503)
                                    .body(Full::new(Bytes::from("shutting_down")))
                                    .map_err(|e| {
                                        WintermuteError::Other(format!(
                                            "failed to build response: {e}"
                                        ))
                                    })?)
                            } else {
                                let last_event = metrics::INGESTER_LAST_EVENT_TIME_SECONDS.get();
                                let now = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
                                // gauge at 0 = no event since boot; lag is unknown, not now-0.
                                // clamped: event times are source-declared and can be skewed
                                let lag = (last_event > 0).then(|| (now - last_event).max(0));
                                let body = serde_json::json!({
                                    "status": "ok",
                                    "ingestLagSeconds": lag,
                                    "indexerQueueLength":
                                        metrics::INGESTER_FIREHOSE_LIVE_LENGTH.get(),
                                });
                                Ok(Response::builder()
                                    .status(200)
                                    .header("Content-Type", "application/json")
                                    .body(Full::new(Bytes::from(body.to_string())))
                                    .map_err(|e| {
                                        WintermuteError::Other(format!(
                                            "failed to build response: {e}"
                                        ))
                                    })?)
                            }
                        }
                        _ => Ok(Response::builder()
                            .status(404)
                            .body(Full::new(Bytes::from("Not Found")))
                            .map_err(|e| {
                                WintermuteError::Other(format!("failed to build response: {e}"))
                            })?),
                    }
                });

                if let Err(e) = http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .await
                {
                    tracing::error!("error serving connection: {e}");
                }
            });
        }

        Ok::<_, WintermuteError>(())
    })
    .map_err(|e| WintermuteError::Other(format!("metrics server error: {e}")))?;

    Ok(())
}
