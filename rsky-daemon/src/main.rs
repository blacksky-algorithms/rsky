//! Syncer daemon entrypoint.
//!
//! First-pass skeleton: parse config and log readiness. The run loop — obtain a
//! space credential from the host, `listRepos` for the writer set, `sync_repo`
//! each advanced repo via the [`rsky_daemon::engine`], subscribe to write
//! notifications, and periodically sweep — builds on the tested engine and
//! lands once the upstream `com.atproto.space.*` shapes (PR #5187) are pinned.

use clap::Parser;
use rsky_daemon::config::Config;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cfg = Config::parse();
    tracing::info!(
        space = %cfg.space_uri,
        host = %cfg.space_host_url,
        sweep_secs = cfg.sweep_interval_secs,
        "daemon ready (sync loop pending upstream com.atproto.space.* shapes)"
    );

    // TODO: mint a space credential from the host, then loop:
    //   listRepos -> for each advanced repo, sync_repo(HttpRepoHost, index, keys, ..)
    //   registerNotify for push; sweep every sweep_interval_secs for self-healing.
    Ok(())
}
