use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

use clap::Parser;
use color_eyre::Result;
use mimalloc::MiMalloc;
use signal_hook::consts::{SIGINT, TERM_SIGNALS};
use signal_hook::flag;
use signal_hook::iterator::SignalsInfo;
use signal_hook::iterator::exfiltrator::WithOrigin;
use tracing_subscriber::EnvFilter;

use rsky_wintermute::SHUTDOWN;
use rsky_wintermute::backfiller::BackfillerManager;
use rsky_wintermute::indexer::IndexerManager;
use rsky_wintermute::ingester::IngesterManager;
use rsky_wintermute::metrics;
use rsky_wintermute::storage::Storage;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

const SLEEP: Duration = Duration::from_millis(10);

#[derive(Debug, clap::Parser)]
pub struct Args {
    #[clap(long, env = "RELAY_HOSTS", value_delimiter = ',')]
    relay_hosts: Vec<String>,

    #[clap(long, env = "LABELER_HOSTS", value_delimiter = ',', default_value = "")]
    labeler_hosts: Vec<String>,

    #[clap(long, env = "DATABASE_URL")]
    database_url: String,

    #[clap(long, env = "METRICS_PORT", default_value = "9090")]
    metrics_port: u16,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    color_eyre::install()?;

    let args = Args::parse();

    if args.relay_hosts.is_empty() {
        return Err(color_eyre::eyre::eyre!("no relay hosts specified"));
    }

    tracing::info!("starting wintermute");
    tracing::info!("relay hosts: {:?}", args.relay_hosts);
    tracing::info!("database: {}", args.database_url);
    tracing::info!("metrics port: {}", args.metrics_port);

    // Initialize metrics
    metrics::initialize_metrics();

    let terminate_now = Arc::new(std::sync::atomic::AtomicBool::new(false));
    flag::register_conditional_shutdown(SIGINT, 1, Arc::clone(&terminate_now))?;
    flag::register(SIGINT, Arc::clone(&terminate_now))?;

    let storage = Arc::new(Storage::new(None)?);
    let labeler_hosts: Vec<String> = args
        .labeler_hosts
        .into_iter()
        .filter(|h| !h.is_empty())
        .collect();
    let ingester = IngesterManager::new(
        args.relay_hosts,
        labeler_hosts,
        Arc::clone(&storage),
        args.database_url.clone(),
    )?;
    let backfiller = BackfillerManager::new(Arc::clone(&storage))?;
    let indexer = IndexerManager::new(Arc::clone(&storage), args.database_url)?;

    let metrics_port = args.metrics_port;

    thread::scope(move |s| {
        let handles = vec![
            thread::Builder::new()
                .name("wintermute-ingester".into())
                .spawn_scoped(s, move || ingester.run())?,
            thread::Builder::new()
                .name("wintermute-backfiller".into())
                .spawn_scoped(s, move || backfiller.run())?,
            thread::Builder::new()
                .name("wintermute-indexer".into())
                .spawn_scoped(s, move || indexer.run())?,
            thread::Builder::new()
                .name("wintermute-metrics".into())
                .spawn_scoped(
                    s,
                    move || -> Result<(), rsky_wintermute::types::WintermuteError> {
                        start_metrics_server(metrics_port).map_err(|e| {
                            rsky_wintermute::types::WintermuteError::Other(format!(
                                "metrics error: {e}"
                            ))
                        })?;
                        Ok(())
                    },
                )?,
        ];

        let mut signals = SignalsInfo::<WithOrigin>::new(TERM_SIGNALS)
            .map_err(|e| color_eyre::eyre::eyre!("failed to init signals: {e}"))?;

        'outer: loop {
            for signal_info in signals.pending() {
                if TERM_SIGNALS.contains(&signal_info.signal) {
                    break 'outer;
                }
            }
            for handle in &handles {
                if handle.is_finished() {
                    break 'outer;
                }
            }
            thread::sleep(SLEEP);
        }

        tracing::info!("shutting down");
        SHUTDOWN.store(true, Ordering::Relaxed);

        for handle in handles {
            if let Ok(res) = handle.join() {
                res?;
            }
        }

        Ok(())
    })
}

fn start_metrics_server(port: u16) -> Result<()> {
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
        .map_err(|e| color_eyre::eyre::eyre!("failed to create tokio runtime: {e}"))?;

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
                    continue;
                }
                Err(e) => {
                    return Err(color_eyre::eyre::eyre!(
                        "failed to bind metrics server on ports {port}-{try_port}: {e}"
                    ));
                }
            }
        }
        let listener = listener.unwrap();
        let addr = SocketAddr::from(([0, 0, 0, 0], bound_port));

        tracing::info!("metrics server listening on http://{addr}/metrics");

        loop {
            if SHUTDOWN.load(Ordering::Relaxed) {
                break;
            }

            let (stream, _) = match listener.accept().await {
                Ok(conn) => conn,
                Err(e) => {
                    tracing::error!("failed to accept connection: {e}");
                    continue;
                }
            };

            tokio::task::spawn(async move {
                let service = service_fn(move |req: Request<hyper::body::Incoming>| async move {
                    if req.uri().path() == "/metrics" {
                        match metrics::encode_metrics() {
                            Ok(body) => Ok::<_, color_eyre::eyre::Error>(
                                Response::builder()
                                    .status(200)
                                    .header("Content-Type", "text/plain; version=0.0.4")
                                    .body(Full::new(Bytes::from(body)))
                                    .map_err(|e| {
                                        color_eyre::eyre::eyre!("failed to build response: {e}")
                                    })?,
                            ),
                            Err(e) => Ok(Response::builder()
                                .status(500)
                                .body(Full::new(Bytes::from(format!(
                                    "Error encoding metrics: {e}"
                                ))))
                                .map_err(|e| {
                                    color_eyre::eyre::eyre!("failed to build response: {e}")
                                })?),
                        }
                    } else {
                        Ok(Response::builder()
                            .status(404)
                            .body(Full::new(Bytes::from("Not Found")))
                            .map_err(|e| {
                                color_eyre::eyre::eyre!("failed to build response: {e}")
                            })?)
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

        Ok::<_, color_eyre::eyre::Error>(())
    })
    .map_err(|e| color_eyre::eyre::eyre!("metrics server error: {e}"))?;

    Ok(())
}
