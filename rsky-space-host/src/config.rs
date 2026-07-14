//! Configuration for the space-host service (env prefix `SPACEHOST_`).

use clap::Parser;

/// The Blacksky community space (v1: a single typed space under the authority).
pub const SPACE_TYPE: &str = "community.blacksky.feed";
pub const SPACE_SKEY: &str = "main";

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

    /// The managing app's service identifier (surfaced in getSpace config).
    #[arg(long, env = "SPACEHOST_MANAGING_APP", default_value = "")]
    pub managing_app: String,

    /// Postgres URL for the `blacksky-beta` membership list (managing-app policy).
    #[arg(long, env = "SPACEHOST_MEMBERSHIP_DB_URL", default_value = "")]
    pub membership_db_url: String,

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
        assert_eq!(cfg.space_type(), SPACE_TYPE);
        assert_eq!(cfg.space_skey(), SPACE_SKEY);

        std::env::set_var("SPACEHOST_AUTHORITY_DID", "did:plc:envauthority");
        std::env::set_var("SPACEHOST_SIGNING_KEY_HEX", "bb".repeat(32));
        std::env::set_var("SPACEHOST_MANAGING_APP", "did:web:app#svc");
        std::env::set_var("SPACEHOST_MEMBERSHIP_DB_URL", "postgres://env");
        std::env::set_var("SPACEHOST_BIND", "127.0.0.1:1234");
        let cfg = Config::try_parse_from(["rsky-space-host"]).unwrap();
        for k in [
            "SPACEHOST_AUTHORITY_DID",
            "SPACEHOST_SIGNING_KEY_HEX",
            "SPACEHOST_MANAGING_APP",
            "SPACEHOST_MEMBERSHIP_DB_URL",
            "SPACEHOST_BIND",
        ] {
            std::env::remove_var(k);
        }
        assert_eq!(cfg.authority_did, "did:plc:envauthority");
        assert_eq!(cfg.managing_app, "did:web:app#svc");
        assert_eq!(cfg.bind, "127.0.0.1:1234");
    }
}
