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
