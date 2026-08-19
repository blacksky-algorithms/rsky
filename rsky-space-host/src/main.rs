//! Space-host service entrypoint: parse config, wire the authority registry,
//! stores, and HTTP surface, and serve until shutdown.

use clap::Parser;
use rsky_identity::did::did_resolver::DidResolver;
use rsky_identity::types::{DidResolverOpts, MemoryCache};
use rsky_oauth::dpop::{DpopManager, InMemoryReplayStore};
use rsky_space::space_id::SpaceId;
use rsky_space_host::appaccess::AppAccess;
use rsky_space_host::attestation::HttpMetadataFetcher;
use rsky_space_host::authority::{
    Authority, AuthorityContext, AuthorityFactory, AuthorityRegistry,
};
use rsky_space_host::config::{Config, PolicyMode};
use rsky_space_host::http::{router, AppState, DEFAULT_REGISTRATION_TTL_SECS};
use rsky_space_host::keys::{DocKeyResolver, DocSource, ResolverDocSource};
use rsky_space_host::managing_app::HttpManagingApp;
use rsky_space_host::membership::InMemoryMembership;
use rsky_space_host::notify::HttpNotifier;
use rsky_space_host::pds_seam::PdsSeam;
use rsky_space_host::policy::Policy;
use rsky_space_host::registration::{HttpLifecycleAcker, LifecycleAcker};
use rsky_space_host::repo::SqliteRepos;
use rsky_space_host::signing::Signer;
use rsky_space_host::store::{HostedSpaceStore, SqliteStore};
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

/// Builds one authority's context from the shared host configuration.
struct ContextBuilder {
    policy: PolicyMode,
    managing_app: String,
    members: Vec<String>,
    lifecycle_url: String,
    lifecycle_service_did: String,
    docs: Arc<dyn DocSource>,
    now: Arc<dyn Fn() -> u64 + Send + Sync>,
    jti: Arc<dyn Fn() -> String + Send + Sync>,
}

impl ContextBuilder {
    fn context(&self, space: SpaceId, signer: Signer) -> AuthorityContext {
        let authority_did = space.authority.clone();
        let policy = match self.policy {
            PolicyMode::Public => Policy::Public,
            PolicyMode::MemberList => {
                Policy::MemberList(Arc::new(InMemoryMembership::new(self.members.clone())))
            }
            PolicyMode::ManagingApp => Policy::ManagingApp {
                service_id: self.managing_app.clone(),
                client: Arc::new(HttpManagingApp::new(
                    self.managing_app.clone(),
                    authority_did.clone(),
                    signer.clone(),
                    self.docs.clone(),
                    self.now.clone(),
                    self.jti.clone(),
                )),
            },
        };
        AuthorityContext {
            authority: Arc::new(Authority::new(space, signer.clone(), AppAccess::Open)),
            policy: Arc::new(policy),
            notifier: Arc::new(HttpNotifier::new(
                authority_did.clone(),
                signer.clone(),
                self.now.clone(),
                self.jti.clone(),
            )),
            lifecycle_acker: (self.policy == PolicyMode::ManagingApp).then(|| {
                Arc::new(HttpLifecycleAcker::new(
                    self.lifecycle_url.clone(),
                    self.lifecycle_service_did.clone(),
                    authority_did,
                    signer,
                    self.now.clone(),
                    self.jti.clone(),
                )) as Arc<dyn LifecycleAcker>
            }),
        }
    }
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

    let now: Arc<dyn Fn() -> u64 + Send + Sync> = Arc::new(unix_now);
    let jti: Arc<dyn Fn() -> String + Send + Sync> = Arc::new(random_jti);
    let docs = Arc::new(ResolverDocSource::new(DidResolver::new(DidResolverOpts {
        timeout: None,
        plc_url: Some(cfg.plc_url.clone()),
        did_cache: std::sync::Arc::new(MemoryCache::new(None, None)),
    })));
    let store = Arc::new(SqliteStore::open(&cfg.db_path)?);
    let repos = Arc::new(SqliteRepos::open(&cfg.db_path)?);
    let seam = Arc::new(PdsSeam::open(&cfg.actor_store_dir)?);

    let builder = Arc::new(ContextBuilder {
        policy: cfg.policy,
        managing_app: cfg.managing_app.clone(),
        members: cfg.member_dids(),
        lifecycle_url: cfg.lifecycle_url.clone(),
        lifecycle_service_did: cfg.lifecycle_service_did.clone(),
        docs: docs.clone(),
        now: now.clone(),
        jti: jti.clone(),
    });
    let registry = Arc::new(AuthorityRegistry::new());
    if let Some((authority_did, signing_key_hex)) = cfg.bootstrap_pin() {
        let signer = Signer::from_hex(signing_key_hex)?;
        let space = SpaceId::new(
            authority_did.to_string(),
            cfg.space_type().to_string(),
            cfg.space_skey().to_string(),
        );
        registry.insert(Arc::new(builder.context(space, signer)));
    }
    let factory: AuthorityFactory = {
        let builder = builder.clone();
        let seam = seam.clone();
        Arc::new(move |space: &SpaceId| {
            let signer = seam.signer(&space.authority)?;
            Ok(Arc::new(builder.context(space.clone(), signer)))
        })
    };
    for (authority_did, space_uri) in store.hosted_spaces().await? {
        let context = match registry.authority(&authority_did) {
            Ok(context) => context,
            Err(_) => {
                let built = SpaceId::parse(&space_uri)
                    .map_err(|e| e.to_string())
                    .and_then(|space| factory(&space).map_err(|e| e.to_string()));
                match built {
                    Ok(context) => registry.insert_if_absent(context),
                    Err(error) => {
                        tracing::warn!(authority = %authority_did, space = %space_uri, error = %error,
                            "cannot re-serve persisted space");
                        continue;
                    }
                }
            }
        };
        if let Err(error) = context.authority.register(&space_uri) {
            tracing::warn!(space = %space_uri, error = %error,
                "cannot re-register persisted space");
        }
    }

    let ticker = std::sync::Mutex::new(rsky_common::tid::Ticker::new());
    let state = AppState {
        registry: registry.clone(),
        authority_factory: factory,
        hosted_spaces: store.clone(),
        keys: Arc::new(DocKeyResolver::new(docs.clone())),
        docs,
        metadata: Arc::new(HttpMetadataFetcher::new()),
        jti_store: store.clone(),
        writers: store.clone(),
        registrations: store,
        dpop: Arc::new(DpopManager::new(
            None,
            Box::new(InMemoryReplayStore::default()),
        )),
        public_url: cfg.public_url.clone(),
        now,
        jti,
        registration_ttl_secs: DEFAULT_REGISTRATION_TTL_SECS,
        repos,
        commit_signer: seam,
        auth: cfg.auth_config(),
        rev: Arc::new(move || ticker.lock().expect("ticker").next(None).to_string()),
        mint_token: cfg.mint_token.clone(),
        credential_mint_services: [
            cfg.daemon_service_did.clone(),
            cfg.appview_service_did.clone(),
        ],
    };

    let listener = tokio::net::TcpListener::bind(&cfg.bind).await?;
    tracing::info!(
        authorities = registry.contexts().len(),
        bootstrap = %cfg.authority_did,
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
