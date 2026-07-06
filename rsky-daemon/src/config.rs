//! Configuration for the syncer daemon (env prefix `DAEMON_`).

use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "rsky-daemon",
    about = "atproto permissioned-data syncer daemon"
)]
pub struct Config {
    /// The space to sync: `at://{authority}/space/{type}/{skey}`.
    #[arg(long, env = "DAEMON_SPACE_URI")]
    pub space_uri: String,

    /// The space host (authority) base URL, for listRepos + credential mint.
    #[arg(long, env = "DAEMON_SPACE_HOST_URL")]
    pub space_host_url: String,

    /// Postgres URL for the synced index the appview reads.
    #[arg(long, env = "DAEMON_INDEX_DB_URL", default_value = "")]
    pub index_db_url: String,

    /// Seconds between writer-set sweeps (self-healing when notifications drop).
    #[arg(long, env = "DAEMON_SWEEP_INTERVAL_SECS", default_value_t = 300)]
    pub sweep_interval_secs: u64,
}
