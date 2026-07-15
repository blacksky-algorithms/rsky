//! Configuration for the space-host service (env prefix `SPACEHOST_`).

use clap::Parser;

/// The Blacksky community space (v1: a single typed space under the authority).
pub const SPACE_TYPE: &str = "community.blacksky.feed";
pub const SPACE_SKEY: &str = "main";

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum PolicyMode {
    MemberList,
    Public,
    ManagingApp,
}

#[derive(Debug, Parser)]
#[command(
    name = "rsky-space-host",
    about = "atproto permissioned-data space authority/host"
)]
pub struct Config {
    /// The space authority DID (dedicated community DID).
    #[arg(long, env = "SPACEHOST_AUTHORITY_DID")]
    pub authority_did: String,

    /// Hex-encoded secp256k1 space signing key (`#atproto_space`).
    #[arg(long, env = "SPACEHOST_SIGNING_KEY_HEX")]
    pub signing_key_hex: String,

    /// How the authority authorizes users at credential-mint time.
    #[arg(
        long,
        env = "SPACEHOST_POLICY",
        value_enum,
        default_value = "member-list"
    )]
    pub policy: PolicyMode,

    /// The managing app's service identifier (`did#fragment`); required when
    /// the policy is `managing-app`.
    #[arg(long, env = "SPACEHOST_MANAGING_APP", default_value = "")]
    pub managing_app: String,

    /// Comma-separated member DIDs seeding the `member-list` policy.
    #[arg(long, env = "SPACEHOST_MEMBERS", default_value = "")]
    pub members: String,

    /// Postgres URL for the `blacksky-beta` membership list (managing-app policy).
    #[arg(long, env = "SPACEHOST_MEMBERSHIP_DB_URL", default_value = "")]
    pub membership_db_url: String,

    /// SQLite path for host state (writer set, registrations, used nonces).
    #[arg(long, env = "SPACEHOST_DB_PATH", default_value = "./space_host.db")]
    pub db_path: String,

    /// PLC directory used for DID resolution.
    #[arg(
        long,
        env = "SPACEHOST_PLC_URL",
        default_value = "https://plc.directory"
    )]
    pub plc_url: String,

    /// Bind address for the HTTP host.
    #[arg(long, env = "SPACEHOST_BIND", default_value = "0.0.0.0:3600")]
    pub bind: String,
}

impl Config {
    pub fn space_type(&self) -> &str {
        SPACE_TYPE
    }
    pub fn space_skey(&self) -> &str {
        SPACE_SKEY
    }

    pub fn member_dids(&self) -> Vec<String> {
        self.members
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.policy == PolicyMode::ManagingApp && !self.managing_app.contains('#') {
            return Err(
                "managing-app policy requires SPACEHOST_MANAGING_APP (did#fragment)".to_string(),
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // One sequential test: the env-var section mutates process-global state,
    // which would race sibling tests run in parallel.
    #[test]
    fn parses_args_env_and_requirements() {
        assert!(Config::try_parse_from(["rsky-space-host"]).is_err());

        let cfg = Config::try_parse_from([
            "rsky-space-host",
            "--authority-did",
            "did:plc:authority",
            "--signing-key-hex",
            "aa".repeat(32).as_str(),
        ])
        .unwrap();
        assert_eq!(cfg.authority_did, "did:plc:authority");
        assert_eq!(cfg.bind, "0.0.0.0:3600");
        assert_eq!(cfg.policy, PolicyMode::MemberList);
        assert_eq!(cfg.db_path, "./space_host.db");
        assert_eq!(cfg.plc_url, "https://plc.directory");
        assert_eq!(cfg.space_type(), SPACE_TYPE);
        assert_eq!(cfg.space_skey(), SPACE_SKEY);
        assert!(cfg.member_dids().is_empty());
        assert!(cfg.validate().is_ok());
        assert!(format!("{cfg:?}").contains("did:plc:authority"));

        let mut cfg = cfg;
        cfg.update_from(["rsky-space-host", "--bind", "127.0.0.1:9"]);
        assert_eq!(cfg.bind, "127.0.0.1:9");
        assert_eq!(cfg.authority_did, "did:plc:authority");

        // Unknown policy values are rejected.
        assert!(Config::try_parse_from([
            "rsky-space-host",
            "--authority-did",
            "did:plc:authority",
            "--signing-key-hex",
            "aa",
            "--policy",
            "bogus",
        ])
        .is_err());

        std::env::set_var("SPACEHOST_AUTHORITY_DID", "did:plc:envauthority");
        std::env::set_var("SPACEHOST_SIGNING_KEY_HEX", "bb".repeat(32));
        std::env::set_var("SPACEHOST_POLICY", "managing-app");
        std::env::set_var("SPACEHOST_MANAGING_APP", "did:web:app#svc");
        std::env::set_var("SPACEHOST_MEMBERS", "did:plc:aaa, did:plc:bbb,");
        std::env::set_var("SPACEHOST_MEMBERSHIP_DB_URL", "postgres://env");
        std::env::set_var("SPACEHOST_DB_PATH", "/tmp/space.db");
        std::env::set_var("SPACEHOST_PLC_URL", "https://plc.example");
        std::env::set_var("SPACEHOST_BIND", "127.0.0.1:1234");
        let cfg = Config::try_parse_from(["rsky-space-host"]).unwrap();
        for k in [
            "SPACEHOST_AUTHORITY_DID",
            "SPACEHOST_SIGNING_KEY_HEX",
            "SPACEHOST_POLICY",
            "SPACEHOST_MANAGING_APP",
            "SPACEHOST_MEMBERS",
            "SPACEHOST_MEMBERSHIP_DB_URL",
            "SPACEHOST_DB_PATH",
            "SPACEHOST_PLC_URL",
            "SPACEHOST_BIND",
        ] {
            std::env::remove_var(k);
        }
        assert_eq!(cfg.authority_did, "did:plc:envauthority");
        assert_eq!(cfg.policy, PolicyMode::ManagingApp);
        assert_eq!(cfg.managing_app, "did:web:app#svc");
        assert_eq!(
            cfg.member_dids(),
            vec!["did:plc:aaa".to_string(), "did:plc:bbb".to_string()]
        );
        assert_eq!(cfg.db_path, "/tmp/space.db");
        assert_eq!(cfg.plc_url, "https://plc.example");
        assert_eq!(cfg.bind, "127.0.0.1:1234");
        assert!(cfg.validate().is_ok());

        // managing-app policy without a service identifier is invalid.
        let mut invalid = cfg;
        invalid.managing_app = String::new();
        assert!(invalid.validate().is_err());
    }
}
