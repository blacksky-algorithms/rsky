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

    /// This syncer's service identity: the required `aud` on inbound
    /// service-auth notifications.
    #[arg(long, env = "DAEMON_SERVICE_IDENTITY")]
    pub service_identity: String,

    /// Member repo-host base URL; defaults to the space host URL (a single
    /// service is usually both roles).
    #[arg(long, env = "DAEMON_REPO_HOST_URL", default_value = "")]
    pub repo_host_url: String,

    /// The syncer account's PDS base URL, for getDelegationToken.
    #[arg(long, env = "DAEMON_PDS_URL", default_value = "")]
    pub pds_url: String,

    /// Access token for the syncer account on its PDS.
    #[arg(
        long,
        env = "DAEMON_PDS_ACCESS_TOKEN",
        default_value = "",
        hide_env_values = true
    )]
    pub pds_access_token: String,

    /// Dev mode: a pre-issued space credential, bypassing minting entirely.
    #[arg(
        long,
        env = "DAEMON_STATIC_CREDENTIAL",
        default_value = "",
        hide_env_values = true
    )]
    pub static_credential: String,

    /// Bind address for the notify listener.
    #[arg(long, env = "DAEMON_NOTIFY_BIND", default_value = "127.0.0.1:8055")]
    pub notify_bind: String,

    /// Public endpoint registered with the space host for notifications;
    /// defaults to `http://{notify_bind}`.
    #[arg(long, env = "DAEMON_NOTIFY_ENDPOINT", default_value = "")]
    pub notify_endpoint: String,

    /// SQLite path for the synced index (empty = in-memory, dev only).
    #[arg(long, env = "DAEMON_INDEX_DB_PATH", default_value = "")]
    pub index_db_path: String,

    /// Seconds between writer-set sweeps (self-healing when notifications drop).
    #[arg(long, env = "DAEMON_SWEEP_INTERVAL_SECS", default_value_t = 300)]
    pub sweep_interval_secs: u64,
}

impl Config {
    pub fn repo_host_url(&self) -> &str {
        if self.repo_host_url.is_empty() {
            &self.space_host_url
        } else {
            &self.repo_host_url
        }
    }

    pub fn notify_endpoint(&self) -> String {
        if self.notify_endpoint.is_empty() {
            format!("http://{}", self.notify_bind)
        } else {
            self.notify_endpoint.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REQUIRED: [&str; 7] = [
        "rsky-daemon",
        "--space-uri",
        "at://did:plc:authority/space/community.blacksky.feed/main",
        "--space-host-url",
        "https://host.example",
        "--service-identity",
        "did:web:syncer.example",
    ];

    // One sequential test: the env-var section mutates process-global state,
    // which would race sibling tests run in parallel.
    #[test]
    fn parses_args_env_and_requirements() {
        use clap::CommandFactory;
        Config::command().debug_assert();
        assert!(Config::try_parse_from(["rsky-daemon"]).is_err());

        let cfg = Config::try_parse_from(REQUIRED).unwrap();
        assert_eq!(cfg.sweep_interval_secs, 300);
        assert_eq!(cfg.index_db_path, "");
        assert_eq!(cfg.notify_bind, "127.0.0.1:8055");
        assert_eq!(cfg.repo_host_url(), "https://host.example");
        assert_eq!(cfg.notify_endpoint(), "http://127.0.0.1:8055");
        assert_eq!(cfg.pds_url, "");
        assert_eq!(cfg.pds_access_token, "");
        assert_eq!(cfg.static_credential, "");

        let mut cfg = Config::try_parse_from(REQUIRED.into_iter().chain([
            "--repo-host-url",
            "https://pds.example",
            "--notify-endpoint",
            "https://syncer.example/notify",
        ]))
        .unwrap();
        assert_eq!(cfg.repo_host_url(), "https://pds.example");
        assert_eq!(cfg.notify_endpoint(), "https://syncer.example/notify");

        cfg.try_update_from(["rsky-daemon", "--sweep-interval-secs", "900"])
            .unwrap();
        assert_eq!(cfg.sweep_interval_secs, 900);

        let env = [
            ("DAEMON_SPACE_URI", "at://a/space/t/main"),
            ("DAEMON_SPACE_HOST_URL", "https://host.example"),
            ("DAEMON_SERVICE_IDENTITY", "did:web:syncer.example"),
            ("DAEMON_PDS_URL", "https://pds.example"),
            ("DAEMON_PDS_ACCESS_TOKEN", "access.jwt"),
            ("DAEMON_STATIC_CREDENTIAL", "sc.jwt"),
            ("DAEMON_NOTIFY_BIND", "0.0.0.0:9000"),
            ("DAEMON_INDEX_DB_PATH", "/data/space.sqlite"),
            ("DAEMON_SWEEP_INTERVAL_SECS", "60"),
        ];
        for (k, v) in env {
            std::env::set_var(k, v);
        }
        let cfg = Config::try_parse_from(["rsky-daemon"]).unwrap();
        for (k, _) in env {
            std::env::remove_var(k);
        }
        assert_eq!(cfg.space_uri, "at://a/space/t/main");
        assert_eq!(cfg.service_identity, "did:web:syncer.example");
        assert_eq!(cfg.pds_url, "https://pds.example");
        assert_eq!(cfg.pds_access_token, "access.jwt");
        assert_eq!(cfg.static_credential, "sc.jwt");
        assert_eq!(cfg.notify_bind, "0.0.0.0:9000");
        assert_eq!(cfg.notify_endpoint(), "http://0.0.0.0:9000");
        assert_eq!(cfg.index_db_path, "/data/space.sqlite");
        assert_eq!(cfg.sweep_interval_secs, 60);
    }
}
