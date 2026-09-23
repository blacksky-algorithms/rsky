//! `rsky-relay-compare`: continuous parity check of relays against a reference.
//!
//! Every window it prints one JSON report line per candidate relay and exits
//! non-zero at the end if any window breached the thresholds, so cron can alert.
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use clap::Parser;
use tungstenite::Message;

use rsky_relay::compare::{Session, Thresholds};

#[derive(Debug, Parser)]
struct Args {
    /// Reference relay hostname (bsky.network by default)
    #[clap(long, default_value = "bsky.network")]
    reference: String,
    /// Candidate relay hostnames, or full ws:// / wss:// origins
    #[clap(long, required = true)]
    relay: Vec<String>,
    /// Window length in seconds
    #[clap(long, default_value_t = 600)]
    window_seconds: u64,
    /// Number of windows before exiting (0 = run forever)
    #[clap(long, default_value_t = 1)]
    windows: u64,
    #[clap(long, default_value_t = 0.005)]
    max_missing_ratio: f64,
    #[clap(long, default_value_t = 2000)]
    max_latency_p50_ms: u64,
}

fn subscribe(host: String, tx: mpsc::Sender<(String, Vec<u8>, Instant)>) {
    thread::spawn(move || {
        loop {
            let url = if host.contains("://") {
                format!("{host}/xrpc/com.atproto.sync.subscribeRepos")
            } else {
                format!("wss://{host}/xrpc/com.atproto.sync.subscribeRepos")
            };
            match tungstenite::connect(&url) {
                Ok((mut socket, _)) => loop {
                    match socket.read() {
                        Ok(Message::Binary(frame)) => {
                            if tx.send((host.clone(), frame.to_vec(), Instant::now())).is_err() {
                                return;
                            }
                        }
                        Ok(
                            Message::Ping(_)
                            | Message::Pong(_)
                            | Message::Text(_)
                            | Message::Frame(_),
                        ) => {}
                        Ok(Message::Close(_)) | Err(_) => {
                            tracing::warn!(%host, "subscription closed; reconnecting");
                            break;
                        }
                    }
                },
                Err(err) => tracing::warn!(%host, %err, "connect failed; retrying"),
            }
            thread::sleep(Duration::from_secs(2));
        }
    });
}

fn main() -> std::process::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    #[expect(clippy::unwrap_used)]
    rustls::crypto::aws_lc_rs::default_provider().install_default().unwrap();
    let args = Args::parse();
    let (tx, rx) = mpsc::channel();
    subscribe(args.reference.clone(), tx.clone());
    for relay in &args.relay {
        subscribe(relay.clone(), tx.clone());
    }
    drop(tx);
    let thresholds = Thresholds {
        max_missing_ratio: args.max_missing_ratio,
        max_latency_p50: Duration::from_millis(args.max_latency_p50_ms),
    };
    let mut breached = false;
    let mut completed = 0u64;
    while args.windows == 0 || completed < args.windows {
        let mut session = Session::default();
        let deadline = Instant::now() + Duration::from_secs(args.window_seconds);
        while let Ok((host, frame, seen_at)) =
            rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
        {
            session.observe(&host, &frame, seen_at);
            if Instant::now() >= deadline {
                break;
            }
        }
        let reference_commits = session.reference_commits(&args.reference);
        if reference_commits == 0 {
            tracing::warn!(reference = %args.reference, "no commits from the reference this window");
            breached = true;
        }
        for report in session.reports(&args.reference) {
            let pass = report.passes(reference_commits, thresholds);
            breached |= !pass;
            let line = serde_json::json!({
                "window_end": chrono::Utc::now().to_rfc3339(),
                "reference": args.reference,
                "reference_commits": reference_commits,
                "pass": pass,
                "report": report,
            });
            tracing::info!("{line}");
        }
        completed += 1;
    }
    if breached { std::process::ExitCode::FAILURE } else { std::process::ExitCode::SUCCESS }
}
