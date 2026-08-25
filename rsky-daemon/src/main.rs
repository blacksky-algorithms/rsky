//! Syncer daemon entrypoint: wire config, index, credentials, the notify
//! listener, and the run loop, with graceful shutdown on ctrl-c.

use clap::Parser;
use rsky_daemon::config::Config;
use rsky_daemon::engine::CommitKeyResolver;
use rsky_daemon::runner::SpaceWorkerParts;
use rsky_daemon::{
    notify_router, run_multi, AppviewProjector, CombinedSource, CredentialSource, DaemonError,
    FeedsProjector, HttpProjectionIngress, HttpRepoHost, HttpSpaceHost, HttpSpaceSource,
    InMemoryIndex, InternalCredentialProvider, JournalConsumer, MultiRunnerOptions, NotifyState,
    Result, Router, SharedJournalConsumer, SpaceCredentialSource, SpaceIndex, SpaceLifecycleAcker,
    SpaceRegistry, SqliteIndex, StaticCredential, StaticSpaces,
};
use rsky_identity::did::atproto_data::{get_did_key_from_multibase, VerificationMaterial};
use rsky_identity::types::{IdentityResolverOpts, MemoryCache};
use rsky_identity::IdResolver;
use rsky_space::space_id::SpaceId;
use std::sync::Arc;
use tokio::sync::{mpsc, watch};

/// Resolves an account's `#atproto` signing key from its DID document.
struct DidKeyResolver {
    resolver: tokio::sync::Mutex<IdResolver>,
}

impl DidKeyResolver {
    fn new(plc_url: Option<String>) -> Self {
        Self {
            resolver: tokio::sync::Mutex::new(IdResolver::new(IdentityResolverOpts {
                timeout: None,
                plc_url,
                did_cache: Some(std::sync::Arc::new(MemoryCache::new(None, None))),
                backup_nameservers: None,
            })),
        }
    }
}

#[async_trait::async_trait]
impl CommitKeyResolver for DidKeyResolver {
    async fn signing_key(&self, did: &str) -> Result<String> {
        let doc = self
            .resolver
            .lock()
            .await
            .did
            .ensure_resolve(&did.to_string(), None)
            .await
            .map_err(|e| DaemonError::KeyResolution(e.to_string()))?;
        let method = doc
            .verification_method
            .unwrap_or_default()
            .into_iter()
            .find(|m| m.id == format!("{did}#atproto") || m.id == "#atproto")
            .ok_or_else(|| DaemonError::KeyResolution(format!("no #atproto key for {did}")))?;
        let multibase = method.public_key_multibase.ok_or_else(|| {
            DaemonError::KeyResolution(format!("no publicKeyMultibase for {did}"))
        })?;
        get_did_key_from_multibase(VerificationMaterial {
            r#type: method.r#type,
            public_key_multibase: multibase,
        })
        .map_err(|e| DaemonError::KeyResolution(e.to_string()))?
        .ok_or_else(|| DaemonError::KeyResolution(format!("unsupported key type for {did}")))
    }
}

/// The projection destinations this process was configured with, if any.
struct ProjectionConfig {
    service_identity: String,
    signing_key_hex: String,
    denial_park_after: u32,
    feeds: Option<(String, String)>,
    appview: Option<(String, String)>,
}

type ProjectionParts = (
    Vec<SharedJournalConsumer>,
    Option<Arc<dyn SpaceLifecycleAcker>>,
);

impl ProjectionConfig {
    fn consumers(&self, space: &str) -> Result<ProjectionParts> {
        if self.feeds.is_none() && self.appview.is_none() {
            return Ok((Vec::new(), None));
        }
        let space_id = SpaceId::parse(space)?;
        let mut consumers: Vec<SharedJournalConsumer> = Vec::new();
        let mut acker: Option<Arc<dyn SpaceLifecycleAcker>> = None;
        if let Some((url, audience)) = &self.feeds {
            let ingress = Arc::new(HttpProjectionIngress::new(
                "feeds",
                url,
                &self.service_identity,
                audience,
                &self.signing_key_hex,
            )?);
            acker = Some(ingress.clone());
            consumers.push(Arc::new(
                JournalConsumer::new(
                    Router::new(space_id.clone(), space_id.authority.clone()),
                    Box::new(FeedsProjector::new(ingress, space)),
                )
                .with_denial_park_after(self.denial_park_after),
            ));
        }
        if let Some((url, audience)) = &self.appview {
            let ingress = HttpProjectionIngress::new(
                "appview",
                url,
                &self.service_identity,
                audience,
                &self.signing_key_hex,
            )?;
            consumers.push(Arc::new(
                JournalConsumer::new(
                    Router::new(space_id.clone(), space_id.authority.clone()),
                    Box::new(AppviewProjector::new(ingress, space)),
                )
                .with_denial_park_after(self.denial_park_after),
            ));
        }
        Ok((consumers, acker))
    }
}

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cfg = Config::parse();
    cfg.validate()?;
    let authority_filter = cfg.authority_filter();
    // One proof-of-possession key for the process: the credential it mints is
    // bound to it, and every host it is presented to checks that binding.
    if cfg.dpop_key_path.is_empty() {
        return Err("DAEMON_DPOP_KEY_PATH is required".into());
    }
    let dpop = Arc::new(rsky_daemon::dpop::DpopSigner::load_or_generate(
        &cfg.dpop_key_path,
    )?);
    let host = Arc::new(HttpSpaceHost::new(&cfg.space_host_url, dpop.clone()));
    let keys: Arc<dyn CommitKeyResolver> = Arc::new(DidKeyResolver::new(cfg.plc_url()));

    let db = if cfg.index_db_path.is_empty() {
        None
    } else {
        Some(Arc::new(SqliteIndex::open(&cfg.index_db_path)?))
    };

    let shared_creds = if cfg.static_credential.is_empty() {
        if cfg.space_host_mint_token.is_empty() || cfg.service_signing_key_hex.is_empty() {
            return Err(
                "DAEMON_SPACE_HOST_MINT_TOKEN and DAEMON_SERVICE_SIGNING_KEY_HEX are required"
                    .into(),
            );
        }
        Some(Arc::new(InternalCredentialProvider::new(
            &cfg.space_uri,
            &cfg.space_host_mint_token,
            rsky_daemon::service_jwt::ServiceJwtIssuer::from_hex(
                &cfg.service_identity,
                &cfg.service_signing_key_hex,
            )?,
            host.clone(),
        )))
    } else {
        None
    };

    let mut sources: Vec<Box<dyn rsky_daemon::SpaceSource>> = Vec::new();
    if !cfg.space_uri.is_empty() {
        sources.push(Box::new(StaticSpaces::new([cfg.space_uri.clone()])));
    }
    if !cfg.spaces_url.is_empty() {
        sources.push(Box::new(HttpSpaceSource::new(
            &cfg.spaces_url,
            &cfg.spaces_api_key,
            authority_filter.clone(),
            &cfg.space_type,
        )));
    }
    let source = Arc::new(CombinedSource(sources));
    let registry = SpaceRegistry::new();

    let (notify_tx, notify_rx) = mpsc::channel(1024);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let notify_state = NotifyState {
        space_uri: cfg.space_uri.clone(),
        registry: registry.clone(),
        service_identity: cfg.service_identity.clone(),
        resolver: keys.clone(),
        index: Arc::new(InMemoryIndex::new()),
        tx: notify_tx,
        now_fn: rsky_daemon::unix_now,
    };
    let listener = tokio::net::TcpListener::bind(&cfg.notify_bind).await?;
    tracing::info!(
        authority = %authority_filter.as_deref().unwrap_or("(any)"),
        host = %cfg.space_host_url,
        notify_bind = %cfg.notify_bind,
        sweep_secs = cfg.sweep_interval_secs,
        "daemon starting"
    );

    let mut serve_shutdown = shutdown_rx.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, notify_router(notify_state))
            .with_graceful_shutdown(async move {
                let _ = serve_shutdown.changed().await;
            })
            .await
    });

    let repo_host_base = cfg.repo_host_url().to_string();
    let static_credential = cfg.static_credential.clone();
    let db_for_factory = db.clone();
    let dpop_for_factory = dpop.clone();
    let shared_for_factory = shared_creds.clone();
    let projection = ProjectionConfig {
        service_identity: cfg.service_identity.clone(),
        signing_key_hex: cfg.service_signing_key_hex.clone(),
        denial_park_after: cfg.denial_park_after,
        feeds: cfg
            .feeds_projection()
            .map(|(url, aud)| (url.to_string(), aud.to_string())),
        appview: cfg
            .appview_projection()
            .map(|(url, aud)| (url.to_string(), aud.to_string())),
    };
    let factory = Arc::new(move |space: &str| -> Result<SpaceWorkerParts> {
        let creds: Arc<dyn CredentialSource> = match &shared_for_factory {
            Some(provider) => Arc::new(SpaceCredentialSource::new(provider.clone(), space)),
            None => Arc::new(StaticCredential(static_credential.clone())),
        };
        let index: Arc<dyn SpaceIndex> = match &db_for_factory {
            Some(db) => Arc::new(db.for_space(space)),
            None => Arc::new(InMemoryIndex::new()),
        };
        let base = repo_host_base.clone();
        let proof = dpop_for_factory.clone();
        let (projectors, acker) = projection.consumers(space)?;
        Ok((
            creds,
            Box::new(move |credential| {
                Arc::new(HttpRepoHost::new(base.clone(), credential, proof.clone()))
            }),
            index,
            projectors,
            acker,
        ))
    });
    let opts = MultiRunnerOptions {
        refresh_interval_secs: cfg.sweep_interval_secs,
        sweep_interval_secs: cfg.sweep_interval_secs,
        notify_endpoint: cfg.notify_endpoint(),
        service_identity: cfg.service_identity.clone(),
        now_fn: rsky_daemon::unix_now,
    };
    let runner = tokio::spawn(run_multi(
        opts,
        source,
        registry,
        factory,
        host,
        keys,
        notify_rx,
        shutdown_rx,
    ));

    tokio::signal::ctrl_c().await?;
    tracing::info!("ctrl-c received; shutting down");
    let _ = shutdown_tx.send(true);
    runner.await?;
    server.await??;
    Ok(())
}
