//! Space-host service entrypoint: parse config, wire the authority, policy,
//! stores, and HTTP surface, and serve until shutdown.

use clap::Parser;
use rsky_identity::did::did_resolver::DidResolver;
use rsky_identity::types::{DidCache, DidResolverOpts};
use rsky_space::space_id::SpaceId;
use rsky_space_host::appaccess::AppAccess;
use rsky_space_host::attestation::HttpMetadataFetcher;
use rsky_space_host::authority::Authority;
use rsky_space_host::config::{Config, PolicyMode};
use rsky_space_host::http::{router, AppState, DEFAULT_REGISTRATION_TTL_SECS};
use rsky_space_host::keys::{DocKeyResolver, ResolverDocSource};
use rsky_space_host::managing_app::HttpManagingApp;
use rsky_space_host::membership::InMemoryMembership;
use rsky_space_host::notify::HttpNotifier;
use rsky_space_host::policy::Policy;
use rsky_space_host::signing::Signer;
use rsky_space_host::store::SqliteStore;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before unix epoch")
        .as_secs()
}

fn random_jti() -> String {
    hex::encode(rand::random::<[u8; 16]>())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutdown signal received");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cfg = Config::parse();
    cfg.validate()?;
    let signer = Signer::from_hex(&cfg.signing_key_hex)?;
    let space = SpaceId::new(
        cfg.authority_did.clone(),
        cfg.space_type().to_string(),
        cfg.space_skey().to_string(),
    );
    let authority = Authority::new(space, signer.clone(), AppAccess::Open);

    let now: Arc<dyn Fn() -> u64 + Send + Sync> = Arc::new(unix_now);
    let jti: Arc<dyn Fn() -> String + Send + Sync> = Arc::new(random_jti);
    let docs = Arc::new(ResolverDocSource::new(DidResolver::new(DidResolverOpts {
        timeout: None,
        plc_url: Some(cfg.plc_url.clone()),
        did_cache: DidCache::new(None, None),
    })));
    let policy = match cfg.policy {
        PolicyMode::Public => Policy::Public,
        PolicyMode::MemberList => {
            Policy::MemberList(Arc::new(InMemoryMembership::new(cfg.member_dids())))
        }
        PolicyMode::ManagingApp => Policy::ManagingApp {
            service_id: cfg.managing_app.clone(),
            client: Arc::new(HttpManagingApp::new(
                cfg.managing_app.clone(),
                cfg.authority_did.clone(),
                signer.clone(),
                docs.clone(),
                now.clone(),
                jti.clone(),
            )),
        },
    };
    let store = Arc::new(SqliteStore::open(&cfg.db_path)?);
    let state = AppState {
        authority: Arc::new(authority),
        policy: Arc::new(policy),
        keys: Arc::new(DocKeyResolver::new(docs)),
        metadata: Arc::new(HttpMetadataFetcher::new()),
        jti_store: store.clone(),
        writers: store.clone(),
        registrations: store,
        notifier: Arc::new(HttpNotifier::new(
            cfg.authority_did.clone(),
            signer,
            now.clone(),
            jti.clone(),
        )),
        now,
        jti,
        registration_ttl_secs: DEFAULT_REGISTRATION_TTL_SECS,
    };

    let listener = tokio::net::TcpListener::bind(&cfg.bind).await?;
    tracing::info!(
        space = %state.authority.space_uri(),
        authority_key = %state.authority.signer.did_key(),
        policy = ?cfg.policy,
        bind = %cfg.bind,
        db = %cfg.db_path,
        "space-host serving"
    );
    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}
