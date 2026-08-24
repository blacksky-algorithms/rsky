//! Configuration for the space-host service (env prefix `SPACEHOST_`).

use crate::oauth::AuthConfig;
use crate::pds_seam::VerifyOnlyHs256Secret;
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
    /// Optional bootstrap authority pin: a space authority DID served from
    /// startup with an explicit signing key. Set together with
    /// `SPACEHOST_SIGNING_KEY_HEX`, or leave both unset and let authorities
    /// arrive via registration.
    #[arg(long, env = "SPACEHOST_AUTHORITY_DID", default_value = "")]
    pub authority_did: String,

    /// Hex-encoded secp256k1 space signing key (`#atproto_space`) for the
    /// pinned bootstrap authority.
    #[arg(long, env = "SPACEHOST_SIGNING_KEY_HEX", default_value = "")]
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

    /// Feeds base URL that receives host-registration acknowledgements.
    #[arg(long, env = "SPACEHOST_LIFECYCLE_URL", default_value = "")]
    pub lifecycle_url: String,

    /// Feeds service DID, used as the acknowledgement JWT audience.
    #[arg(long, env = "SPACEHOST_LIFECYCLE_SERVICE_DID", default_value = "")]
    pub lifecycle_service_did: String,

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

    /// Public origin this host is reached at. DPoP proofs bind to it, so a
    /// value that does not match what clients call fails every proof.
    #[arg(
        long,
        env = "SPACEHOST_PUBLIC_URL",
        default_value = "http://localhost:3600"
    )]
    pub public_url: String,

    #[arg(long, env = "SPACEHOST_OAUTH_ISSUER", default_value = "")]
    pub oauth_issuer: String,
    #[arg(long, env = "SPACEHOST_OAUTH_JWKS_URI", default_value = "")]
    pub oauth_jwks_uri: String,
    #[arg(long, env = "SPACEHOST_OAUTH_AUDIENCE", default_value = "")]
    pub oauth_audience: String,
    #[arg(long, env = "SPACEHOST_OAUTH_CLIENT_IDS", default_value = "")]
    pub oauth_client_ids: String,
    #[arg(
        long,
        env = "SPACEHOST_OAUTH_HS256_SECRET",
        default_value = "",
        hide_env_values = true
    )]
    pub oauth_hs256_secret: String,
    #[arg(long, env = "SPACEHOST_ACTOR_STORE_DIR", default_value = "")]
    pub actor_store_dir: String,
    #[arg(
        long,
        env = "SPACEHOST_MINT_TOKEN",
        default_value = "",
        hide_env_values = true
    )]
    pub mint_token: String,
    #[arg(long, env = "SPACEHOST_DAEMON_SERVICE_DID", default_value = "")]
    pub daemon_service_did: String,
    #[arg(long, env = "SPACEHOST_APPVIEW_SERVICE_DID", default_value = "")]
    pub appview_service_did: String,
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

    pub fn auth_config(&self) -> AuthConfig {
        AuthConfig {
            issuer: self.oauth_issuer.clone(),
            jwks_uri: self.oauth_jwks_uri.clone(),
            audience: self.oauth_audience.clone(),
            client_ids: self
                .oauth_client_ids
                .split(',')
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_string)
                .collect(),
            hs256_secret: VerifyOnlyHs256Secret::new(self.oauth_hs256_secret.as_bytes().to_vec()),
        }
    }

    pub fn bootstrap_pin(&self) -> Option<(&str, &str)> {
        (!self.authority_did.is_empty() && !self.signing_key_hex.is_empty())
            .then_some((self.authority_did.as_str(), self.signing_key_hex.as_str()))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.authority_did.is_empty() != self.signing_key_hex.is_empty() {
            return Err(
                "SPACEHOST_AUTHORITY_DID and SPACEHOST_SIGNING_KEY_HEX must be set together (bootstrap pin) or both left unset"
                    .to_string(),
            );
        }
        if self.bootstrap_pin().is_none() && self.actor_store_dir.is_empty() {
            return Err(
                "no space authority available: set SPACEHOST_ACTOR_STORE_DIR (authorities register with actor-store keys) or pin one with SPACEHOST_AUTHORITY_DID + SPACEHOST_SIGNING_KEY_HEX"
                    .to_string(),
            );
        }
        if self.policy == PolicyMode::ManagingApp && !self.managing_app.contains('#') {
            return Err(
                "managing-app policy requires SPACEHOST_MANAGING_APP (did#fragment)".to_string(),
            );
        }
        if self.policy == PolicyMode::ManagingApp
            && (self.lifecycle_url.is_empty() || self.lifecycle_service_did.is_empty())
        {
            return Err(
                "managing-app policy requires SPACEHOST_LIFECYCLE_URL and SPACEHOST_LIFECYCLE_SERVICE_DID"
                    .to_string(),
            );
        }
        if self.public_url.trim_end_matches('/').is_empty() {
            return Err("SPACEHOST_PUBLIC_URL must be an absolute origin".to_string());
        }
        self.auth_config().validate()?;
        if self.actor_store_dir.is_empty() {
            return Err("SPACEHOST_ACTOR_STORE_DIR is required".to_string());
        }
        if self.mint_token.is_empty()
            || self.daemon_service_did.is_empty()
            || self.appview_service_did.is_empty()
        {
            return Err("SPACEHOST_MINT_TOKEN, SPACEHOST_DAEMON_SERVICE_DID, and SPACEHOST_APPVIEW_SERVICE_DID are required".to_string());
        }
        Ok(())
    }

    /// The managing-app conversation is service auth, which is keyed on the
    /// authority's `#atproto` key rather than the space key. A pinned authority
    /// carries only the space key, so without an actor-store key for it the
    /// policy can never mint a call the managing app will accept. Fail at boot
    /// instead of at a member's first read. `has_service_key` answers whether
    /// the actor store holds a signing key for that DID.
    pub fn validate_pinned_service_key(
        &self,
        has_service_key: impl FnOnce(&str) -> bool,
    ) -> Result<(), String> {
        let Some((authority_did, _)) = self.bootstrap_pin() else {
            return Ok(());
        };
        if self.policy != PolicyMode::ManagingApp {
            return Ok(());
        }
        if has_service_key(authority_did) {
            return Ok(());
        }
        Err(format!(
            "managing-app policy needs an actor-store signing key for the pinned authority {authority_did}: SPACEHOST_ACTOR_STORE_DIR has none, so only the public and member-list policies are available to a pinned host"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // `Config` falls back to env vars, and the env-var test below mutates
    // process-global state, so every parse in this module is serialized.
    static PARSE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn parse_lock() -> std::sync::MutexGuard<'static, ()> {
        PARSE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    // One sequential test: the env-var section mutates process-global state,
    // which would race sibling tests run in parallel.
    #[test]
    fn parses_args_env_and_requirements() {
        let _guard = parse_lock();
        let bare = Config::try_parse_from(["rsky-space-host"]).unwrap();
        assert!(bare.bootstrap_pin().is_none());
        assert!(bare.validate().is_err());

        let cfg = Config::try_parse_from([
            "rsky-space-host",
            "--authority-did",
            "did:plc:authority",
            "--signing-key-hex",
            "aa".repeat(32).as_str(),
            "--oauth-issuer",
            "https://pds.example",
            "--oauth-jwks-uri",
            "https://pds.example/jwks",
            "--oauth-audience",
            "did:web:pds.example",
            "--oauth-client-ids",
            "https://client.example",
            "--actor-store-dir",
            "/actors",
            "--mint-token",
            "token",
            "--daemon-service-did",
            "did:plc:daemon",
            "--appview-service-did",
            "did:plc:appview",
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
        cfg.update_from([
            "rsky-space-host",
            "--bind",
            "127.0.0.1:9",
            "--authority-did",
            "did:plc:authority",
        ]);
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
        std::env::set_var("SPACEHOST_LIFECYCLE_URL", "https://feeds.example");
        std::env::set_var("SPACEHOST_LIFECYCLE_SERVICE_DID", "did:web:feeds.example");
        std::env::set_var("SPACEHOST_DB_PATH", "/tmp/space.db");
        std::env::set_var("SPACEHOST_PLC_URL", "https://plc.example");
        std::env::set_var("SPACEHOST_BIND", "127.0.0.1:1234");
        std::env::set_var("SPACEHOST_OAUTH_ISSUER", "https://pds.example");
        std::env::set_var("SPACEHOST_OAUTH_JWKS_URI", "https://pds.example/jwks");
        std::env::set_var("SPACEHOST_OAUTH_AUDIENCE", "did:web:pds.example");
        std::env::set_var("SPACEHOST_OAUTH_CLIENT_IDS", "https://client.example");
        std::env::set_var("SPACEHOST_ACTOR_STORE_DIR", "/actors");
        std::env::set_var("SPACEHOST_MINT_TOKEN", "token");
        std::env::set_var("SPACEHOST_DAEMON_SERVICE_DID", "did:plc:daemon");
        std::env::set_var("SPACEHOST_APPVIEW_SERVICE_DID", "did:plc:appview");
        let cfg = Config::try_parse_from(["rsky-space-host"]).unwrap();
        for k in [
            "SPACEHOST_AUTHORITY_DID",
            "SPACEHOST_SIGNING_KEY_HEX",
            "SPACEHOST_POLICY",
            "SPACEHOST_MANAGING_APP",
            "SPACEHOST_MEMBERS",
            "SPACEHOST_MEMBERSHIP_DB_URL",
            "SPACEHOST_LIFECYCLE_URL",
            "SPACEHOST_LIFECYCLE_SERVICE_DID",
            "SPACEHOST_DB_PATH",
            "SPACEHOST_PLC_URL",
            "SPACEHOST_BIND",
            "SPACEHOST_OAUTH_ISSUER",
            "SPACEHOST_OAUTH_JWKS_URI",
            "SPACEHOST_OAUTH_AUDIENCE",
            "SPACEHOST_OAUTH_CLIENT_IDS",
            "SPACEHOST_ACTOR_STORE_DIR",
            "SPACEHOST_MINT_TOKEN",
            "SPACEHOST_DAEMON_SERVICE_DID",
            "SPACEHOST_APPVIEW_SERVICE_DID",
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
        assert_eq!(cfg.lifecycle_url, "https://feeds.example");
        assert_eq!(cfg.lifecycle_service_did, "did:web:feeds.example");
        assert_eq!(cfg.plc_url, "https://plc.example");
        assert_eq!(cfg.bind, "127.0.0.1:1234");
        assert!(cfg.validate().is_ok());

        // managing-app policy without a service identifier is invalid.
        let mut invalid = cfg;
        invalid.managing_app = String::new();
        assert!(invalid.validate().is_err());
    }

    fn valid_unpinned() -> Config {
        let _guard = parse_lock();
        Config::try_parse_from([
            "rsky-space-host",
            "--oauth-issuer",
            "https://pds.example",
            "--oauth-jwks-uri",
            "https://pds.example/jwks",
            "--oauth-audience",
            "did:web:pds.example",
            "--oauth-client-ids",
            "https://client.example",
            "--actor-store-dir",
            "/actors",
            "--mint-token",
            "token",
            "--daemon-service-did",
            "did:plc:daemon",
            "--appview-service-did",
            "did:plc:appview",
        ])
        .unwrap()
    }

    #[test]
    fn pinned_managing_app_needs_an_actor_store_service_key() {
        fn pinned_managing_app(policy: PolicyMode) -> Config {
            let mut cfg = valid_unpinned();
            cfg.authority_did = "did:plc:authority".to_string();
            cfg.signing_key_hex = "aa".repeat(32);
            cfg.policy = policy;
            cfg.managing_app = "did:web:feeds.example#bsky_fg".to_string();
            cfg.lifecycle_url = "https://feeds.example".to_string();
            cfg.lifecycle_service_did = "did:web:feeds.example".to_string();
            cfg
        }

        let pinned = pinned_managing_app(PolicyMode::ManagingApp);
        assert!(pinned.validate().is_ok());

        // No actor-store key for the pinned authority: refuse at boot.
        let message = pinned
            .validate_pinned_service_key(|_| false)
            .expect_err("must refuse");
        assert!(message.contains("did:plc:authority"), "{message}");
        assert!(message.contains("member-list"), "{message}");

        // With a key, the same config is accepted, and the probe sees the
        // pinned authority rather than some other DID.
        let mut asked = String::new();
        pinned
            .validate_pinned_service_key(|did| {
                asked = did.to_string();
                true
            })
            .unwrap();
        assert_eq!(asked, "did:plc:authority");

        // The other policies are unaffected — that is the point of the fallback.
        pinned_managing_app(PolicyMode::MemberList)
            .validate_pinned_service_key(|_| false)
            .unwrap();
        pinned_managing_app(PolicyMode::Public)
            .validate_pinned_service_key(|_| false)
            .unwrap();

        // An unpinned host resolves its authorities from the actor store, so
        // there is nothing to reject.
        let mut unpinned = valid_unpinned();
        unpinned.policy = PolicyMode::ManagingApp;
        unpinned.validate_pinned_service_key(|_| false).unwrap();
    }

    #[test]
    fn bootstrap_pin_is_optional_but_all_or_nothing() {
        let cfg = valid_unpinned();
        assert!(cfg.bootstrap_pin().is_none());
        assert!(cfg.validate().is_ok());

        let mut half = valid_unpinned();
        half.authority_did = "did:plc:authority".to_string();
        assert!(half.validate().is_err());

        let mut half = valid_unpinned();
        half.signing_key_hex = "aa".repeat(32);
        assert!(half.validate().is_err());

        let mut pinned = valid_unpinned();
        pinned.authority_did = "did:plc:authority".to_string();
        pinned.signing_key_hex = "aa".repeat(32);
        assert_eq!(
            pinned.bootstrap_pin(),
            Some(("did:plc:authority", pinned.signing_key_hex.as_str()))
        );
        assert!(pinned.validate().is_ok());

        let mut keyless = valid_unpinned();
        keyless.actor_store_dir = String::new();
        let message = keyless.validate().unwrap_err();
        assert!(
            message.contains("no space authority available"),
            "{message}"
        );
    }
}
