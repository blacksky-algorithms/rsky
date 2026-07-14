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

#[cfg(test)]
mod tests {
    use super::*;

    // One sequential test: the env-var section mutates process-global state,
    // which would race sibling tests run in parallel.
    #[test]
    fn parses_args_env_and_requirements() {
        assert!(Config::try_parse_from(["rsky-daemon"]).is_err());

        let cfg = Config::try_parse_from([
            "rsky-daemon",
            "--space-uri",
            "at://did:plc:authority/space/community.blacksky.feed/main",
            "--space-host-url",
            "https://host.example",
        ])
        .unwrap();
        assert_eq!(cfg.sweep_interval_secs, 300);
        assert_eq!(cfg.index_db_url, "");

        std::env::set_var("DAEMON_SPACE_URI", "at://a/space/t/main");
        std::env::set_var("DAEMON_SPACE_HOST_URL", "https://host.example");
        std::env::set_var("DAEMON_INDEX_DB_URL", "postgres://env");
        std::env::set_var("DAEMON_SWEEP_INTERVAL_SECS", "60");
        let cfg = Config::try_parse_from(["rsky-daemon"]).unwrap();
        for k in [
            "DAEMON_SPACE_URI",
            "DAEMON_SPACE_HOST_URL",
            "DAEMON_INDEX_DB_URL",
            "DAEMON_SWEEP_INTERVAL_SECS",
        ] {
            std::env::remove_var(k);
        }
        assert_eq!(cfg.space_uri, "at://a/space/t/main");
        assert_eq!(cfg.index_db_url, "postgres://env");
        assert_eq!(cfg.sweep_interval_secs, 60);
    }
}
