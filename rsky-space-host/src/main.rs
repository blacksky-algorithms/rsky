//! Space-host service entrypoint.
//!
//! First-pass skeleton: parse config, load the authority signing key, construct
//! the [`Authority`], and log readiness. The HTTP routes
//! (`getSpace`/`getSpaceCredential`/`listRepos`/notification routing) build on
//! the tested core in the library and land next, once the upstream
//! `com.atproto.space.*` request/response shapes (PR #5187) are pinned.

use clap::Parser;
use rsky_space::space_id::SpaceId;
use rsky_space_host::appaccess::AppAccess;
use rsky_space_host::authority::Authority;
use rsky_space_host::config::Config;
use rsky_space_host::signing::Signer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cfg = Config::parse();
    let signer = Signer::from_hex(&cfg.signing_key_hex)?;
    let space = SpaceId::new(
        cfg.authority_did.clone(),
        cfg.space_type().to_string(),
        cfg.space_skey().to_string(),
    );
    let authority = Authority::new(space, signer, AppAccess::Open);

    tracing::info!(
        space = %authority.space_uri(),
        authority_key = %authority.signer.did_key(),
        bind = %cfg.bind,
        "space-host ready (HTTP routes pending upstream com.atproto.space.* shapes)"
    );

    // TODO: bind the HTTP host and serve getSpace / getSpaceCredential /
    // listRepos / registerNotify / notifyWrite / notifySpaceDeleted against the
    // Authority + a Postgres-backed MembershipStore.
    Ok(())
}
