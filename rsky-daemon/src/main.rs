//! Syncer daemon entrypoint: wire config, index, credentials, the notify
//! listener, and the run loop, with graceful shutdown on ctrl-c.

use clap::Parser;
use rsky_daemon::config::Config;
use rsky_daemon::engine::CommitKeyResolver;
use rsky_daemon::{
    notify_router, run, CredentialProvider, CredentialSource, DaemonError, HttpRepoHost,
    HttpSpaceHost, InMemoryIndex, NotifyState, PdsDelegationSource, Result, RunnerOptions,
    SpaceIndex, SqliteIndex, StaticCredential,
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
    fn new() -> Self {
        Self {
            resolver: tokio::sync::Mutex::new(IdResolver::new(IdentityResolverOpts {
                timeout: None,
                plc_url: None,
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

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cfg = Config::parse();
    let space = SpaceId::parse(&cfg.space_uri)?;
    let host = Arc::new(HttpSpaceHost::new(&cfg.space_host_url));
    let keys: Arc<dyn CommitKeyResolver> = Arc::new(DidKeyResolver::new());

    let index: Arc<dyn SpaceIndex> = if cfg.index_db_path.is_empty() {
        tracing::warn!("no DAEMON_INDEX_DB_PATH set; using a non-persistent in-memory index");
        Arc::new(InMemoryIndex::new())
    } else {
        Arc::new(Arc::new(SqliteIndex::open(&cfg.index_db_path)?).for_space(&cfg.space_uri))
    };

    let creds: Arc<dyn CredentialSource> = if cfg.static_credential.is_empty() {
        Arc::new(CredentialProvider::new(
            &cfg.space_uri,
            Box::new(PdsDelegationSource::new(
                &cfg.pds_url,
                &cfg.pds_access_token,
            )),
            host.clone(),
        ))
    } else {
        tracing::warn!("using a static space credential (dev mode)");
        Arc::new(StaticCredential(cfg.static_credential.clone()))
    };

    let (notify_tx, notify_rx) = mpsc::channel(1024);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let notify_state = NotifyState {
        space_uri: cfg.space_uri.clone(),
        authority_did: space.authority.clone(),
        service_identity: cfg.service_identity.clone(),
        resolver: keys.clone(),
        index: index.clone(),
        tx: notify_tx,
        now_fn: rsky_daemon::unix_now,
    };
    let listener = tokio::net::TcpListener::bind(&cfg.notify_bind).await?;
    tracing::info!(
        space = %cfg.space_uri,
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
    let opts = RunnerOptions {
        space_uri: cfg.space_uri.clone(),
        sweep_interval_secs: cfg.sweep_interval_secs,
        notify_endpoint: cfg.notify_endpoint(),
        now_fn: rsky_daemon::unix_now,
    };
    let runner = tokio::spawn(run(
        opts,
        host,
        creds,
        Box::new(move |credential| Arc::new(HttpRepoHost::new(repo_host_base.clone(), credential))),
        index,
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
