//! Where the router listens, which upstreams it forwards to, and where its
//! policy, allowlist, journal, and account database live.

use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Clone, Parser)]
#[command(name = "pds-router")]
pub struct Config {
    #[arg(long, env = "ROUTER_PORT", default_value = "4100")]
    pub port: u16,
    #[arg(long, env = "ROUTER_METRICS_PORT", default_value = "4101")]
    pub metrics_port: u16,
    /// The TypeScript main process: writes, sessions, the default for reads.
    #[arg(
        long,
        env = "ROUTER_UPSTREAM_TS_MAIN",
        default_value = "http://127.0.0.1:3000"
    )]
    pub ts_main: String,
    /// The TypeScript sync sidecar: `com.atproto.sync.*`, uploads, imports.
    #[arg(
        long,
        env = "ROUTER_UPSTREAM_TS_SYNC",
        default_value = "http://127.0.0.1:3001"
    )]
    pub ts_sync: String,
    /// The TypeScript read workers, comma separated.
    #[arg(
        long,
        env = "ROUTER_UPSTREAM_TS_READ",
        value_delimiter = ',',
        default_value = "http://127.0.0.1:3002,http://127.0.0.1:3003"
    )]
    pub ts_read: Vec<String>,
    /// The rsky process.
    #[arg(
        long,
        env = "ROUTER_UPSTREAM_RSKY",
        default_value = "http://127.0.0.1:4000"
    )]
    pub rsky: String,
    /// The authorization server.
    #[arg(
        long,
        env = "ROUTER_UPSTREAM_OAUTH",
        default_value = "http://127.0.0.1:3000"
    )]
    pub oauth: String,
    #[arg(
        long,
        env = "ROUTER_POLICY_FILE",
        default_value = "/pds/router/policy.toml"
    )]
    pub policy_file: PathBuf,
    #[arg(
        long,
        env = "ROUTER_ALLOWLIST_FILE",
        default_value = "/pds/router/write-allowlist.toml"
    )]
    pub allowlist_file: PathBuf,
    /// This instance's write-ahead journal; never shared between instances.
    #[arg(long, env = "ROUTER_JOURNAL_FILE")]
    pub journal_file: Option<PathBuf>,
    #[arg(long, env = "ROUTER_ACCOUNT_DB", default_value = "/pds/account.sqlite")]
    pub account_db: PathBuf,
    /// Seconds an upstream may take to answer a read or main-class request.
    #[arg(long, env = "ROUTER_READ_TIMEOUT_SECS", default_value = "25")]
    pub read_timeout_secs: u64,
    /// Seconds an upstream may take to answer a sync-class request.
    #[arg(long, env = "ROUTER_SYNC_TIMEOUT_SECS", default_value = "55")]
    pub sync_timeout_secs: u64,
    /// Seconds an upstream may take to answer a mutation.
    #[arg(long, env = "ROUTER_WRITE_TIMEOUT_SECS", default_value = "300")]
    pub write_timeout_secs: u64,
}

impl Config {
    /// The journal path, defaulting beside the policy file and named by port.
    pub fn journal_path(&self) -> PathBuf {
        self.journal_file.clone().unwrap_or_else(|| {
            self.policy_file
                .parent()
                .map(std::path::Path::to_path_buf)
                .unwrap_or_default()
                .join(format!("mutations-{}.jsonl", self.port))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_journal_path() {
        let config = Config::parse_from(["pds-router"]);
        assert_eq!(config.port, 4100);
        assert_eq!(config.ts_read.len(), 2);
        assert_eq!(
            config.journal_path(),
            PathBuf::from("/pds/router/mutations-4100.jsonl")
        );
        let explicit = Config::parse_from([
            "pds-router",
            "--journal-file",
            "/tmp/j.jsonl",
            "--port",
            "4102",
        ]);
        assert_eq!(explicit.journal_path(), PathBuf::from("/tmp/j.jsonl"));
        assert_eq!(explicit.port, 4102);
    }
}
