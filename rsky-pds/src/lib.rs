#[macro_use]
extern crate serde_derive;
extern crate core;
extern crate mailchecker;
extern crate serde;
use crate::read_after_write::viewer::{LocalViewer, LocalViewerCreator, LocalViewerCreatorParams};
use crate::sequencer::Sequencer;
use atrium_xrpc_client::reqwest::ReqwestClient;
use rsky_common::env::{env_bool, env_int};

pub mod account;
pub mod account_manager;
pub mod actor_store;
pub mod admission;
pub mod apis;
pub mod auth_verifier;
pub mod background;
pub mod blob_attempts;
pub mod blob_generations;
pub mod cli;
pub mod collector;
pub mod config;
pub mod context;
pub mod convergence;
pub mod crawlers;
pub mod custom_routes;
pub mod db;
pub mod did_cache;
pub mod drain;
pub mod exports;
pub mod frontier;
pub mod handle;
pub mod image;
pub mod lexicon;
pub mod lifecycle;
pub mod locks;
pub mod logging;
pub mod mailer;
pub mod metrics;
pub mod models;
pub mod oauth;
pub mod oauth_scope;
pub mod outbound;
pub mod permission_set;
pub mod pipethrough;
pub mod plc;
pub mod publication;
pub mod rate_limits;
pub mod read_after_write;
pub mod repair;
pub mod repo;
pub mod rotate_keys;
pub mod sequencer;
pub mod space_auth;
pub mod space_scope;
pub mod spool;
pub mod telemetry;
pub mod ui;
pub mod well_known;
pub mod xrpc_server;
use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::ActorStore;
use crate::background::BackgroundQueue;
use crate::config::{env_to_cfg, ServiceDbConfig};
use crate::crawlers::Crawlers;
use crate::did_cache::DidSqliteCache;
use crate::models::{ErrorCode, ErrorMessageResponse, ServerVersion};
use rocket::{catch, catchers, get, options, routes, Build, Rocket};
use std::sync::Arc;

pub static APP_USER_AGENT: &str = concat!(
    env!("CARGO_PKG_HOMEPAGE"),
    "@",
    env!("CARGO_PKG_NAME"),
    "/",
    env!("CARGO_PKG_VERSION"),
);

pub struct SharedSequencer {
    pub sequencer: RwLock<Sequencer>,
}

pub struct SharedIdResolver {
    pub id_resolver: RwLock<IdResolver>,
}

pub struct SharedLocalViewer {
    pub local_viewer: RwLock<LocalViewerCreator>,
}

pub struct SharedATPAgent {
    pub app_view_agent: Option<RwLock<AtpServiceClient<ReqwestClient>>>,
}

extern crate rocket;
use crate::apis::{app, bsky_api_get_forwarder, bsky_api_post_forwarder, com, community, ApiError};
use atrium_api::client::AtpServiceClient;
use atrium_xrpc_client::reqwest::ReqwestClientBuilder;
use dotenvy::dotenv;
use rocket::data::{Limits, ToByteUnit};
use rocket::fairing::{Fairing, Info, Kind};
use rocket::http::Header;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::shield::{NoSniff, Shield};
use rocket::{Request, Response, State};
use rsky_identity::types::IdentityResolverOpts;
use rsky_identity::IdResolver;
use std::env;
use tokio::sync::RwLock;

pub struct CORS;

/// Records every request in the metrics and the request log, and marks
/// the process as draining when shutdown is triggered.
pub struct Telemetry;

/// When the request arrived and its number in this process.
#[derive(Clone, Copy)]
struct RequestStart {
    at: std::time::Instant,
    id: u64,
}

static REQUEST_IDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

impl RequestStart {
    fn now() -> Self {
        RequestStart {
            at: std::time::Instant::now(),
            id: REQUEST_IDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        }
    }
}

#[rocket::async_trait]
impl Fairing for Telemetry {
    fn info(&self) -> Info {
        Info {
            name: "Request metrics, request log, and drain state",
            kind: Kind::Request | Kind::Response | Kind::Shutdown,
        }
    }

    async fn on_request(&self, request: &mut Request<'_>, _data: &mut rocket::Data<'_>) {
        request.local_cache(RequestStart::now);
    }

    async fn on_response<'r>(&self, request: &'r Request<'_>, response: &mut Response<'r>) {
        let start = *request.local_cache(RequestStart::now);
        let elapsed = start.at.elapsed();
        let route = request
            .route()
            .map(|route| route.uri.to_string())
            .unwrap_or_else(|| "unrouted".to_string());
        let status = response.status().code;
        metrics::METRICS.record_request(
            &route,
            request.method().as_str(),
            status,
            elapsed.as_secs_f64(),
        );
        if request.route().is_some() {
            metrics::METRICS.record_xrpc(
                request.uri().path().as_str(),
                status,
                elapsed.as_secs_f64(),
            );
        }
        tracing::info!(
            req.id = start.id,
            req.method = request.method().as_str(),
            req.url = %request.uri(),
            req.route = %route,
            req.remoteAddress = request.client_ip().map(|ip| ip.to_string()).unwrap_or_default(),
            res.statusCode = status,
            responseTime = elapsed.as_secs_f64() * 1000.0,
            "request completed"
        );
    }

    async fn on_shutdown(&self, rocket: &rocket::Rocket<rocket::Orbit>) {
        metrics::METRICS.begin_shutdown();
        tracing::warn!("shutdown requested; draining");
        if let Some(sequencer) = rocket.state::<SharedSequencer>() {
            sequencer.sequencer.write().await.destroy().await;
        }
        if let Some(actor_store) = rocket.state::<ActorStore>() {
            actor_store.background_queue.process_all().await;
        }
        tracing::warn!("drained; exiting");
    }
}

/// The reference PDS's per-address budget, charged before routing. An
/// exhausted budget sends the request to the route that answers 429.
pub struct GlobalRateLimit;

#[rocket::async_trait]
impl Fairing for GlobalRateLimit {
    fn info(&self) -> Info {
        Info {
            name: "Global per-address rate limit",
            kind: Kind::Request,
        }
    }

    async fn on_request(&self, request: &mut Request<'_>, _data: &mut rocket::Data<'_>) {
        let Some(limits) = request.rocket().state::<rate_limits::RateLimits>() else {
            return;
        };
        if !limits.enabled() || !rate_limits::global_limit_applies(request.uri().path().as_str()) {
            return;
        }
        let ip = rate_limits::client_ip(request);
        if limits.bypasses(request.headers().get_one("x-ratelimit-bypass"), ip) {
            return;
        }
        let key = ip.map(|ip| ip.to_string()).unwrap_or_default();
        if let Err(status) = limits.consume(&rate_limits::GLOBAL_IP, &key, 1).await {
            request.local_cache(|| Some(ApiError::RateLimitExceeded(status)));
            request.set_method(rocket::http::Method::Get);
            request
                .set_uri(rocket::http::uri::Origin::parse("/_rate_limited").expect("static uri"));
        }
    }
}

#[get("/_rate_limited")]
async fn rate_limited(request_error: RateLimited) -> ApiError {
    request_error.0
}

/// The refusal the global limiter cached for this request.
pub struct RateLimited(ApiError);

#[rocket::async_trait]
impl<'r> rocket::request::FromRequest<'r> for RateLimited {
    type Error = ();

    async fn from_request(req: &'r Request<'_>) -> rocket::request::Outcome<Self, Self::Error> {
        let cached: &Option<ApiError> = req.local_cache(|| None);
        match cached {
            Some(error) => rocket::request::Outcome::Success(RateLimited(error.clone())),
            None => rocket::request::Outcome::Forward(Status::NotFound),
        }
    }
}

#[get("/metrics")]
async fn metrics_route(
    actor_store: &State<ActorStore>,
    lifecycle: &State<lifecycle::LifecycleStore>,
    repairs: &State<repair::RepairStore>,
    sequencer: &State<SharedSequencer>,
) -> Result<String, ApiError> {
    let sequencer = sequencer.sequencer.read().await;
    metrics::METRICS
        .refresh(actor_store, lifecycle, repairs, &sequencer)
        .await?;
    Ok(metrics::METRICS.render()?)
}

#[get("/")]
async fn index() -> &'static str {
    r#"
    .------..------..------..------.
    |R.--. ||S.--. ||K.--. ||Y.--. |
    | :(): || :/\: || :/\: || (\/) |
    | ()() || :\/: || :\/: || :\/: |
    | '--'R|| '--'S|| '--'K|| '--'Y|
    `------'`------'`------'`------'
    .------..------..------.
    |P.--. ||D.--. ||S.--. |
    | :/\: || :/\: || :/\: |
    | (__) || (__) || :\/: |
    | '--'P|| '--'D|| '--'S|
    `------'`------'`------'
    
    This is an atproto [https://atproto.com] Personal Data Server (PDS) running the rsky-pds codebase [https://github.com/blacksky-algorithms/rsky]

    Most API routes are under /xrpc/
    "#
}

#[get("/robots.txt")]
async fn robots() -> &'static str {
    "# Hello!\n\n# Crawling the public API is allowed\nUser-agent: *\nAllow: /"
}

#[tracing::instrument(skip_all)]
#[get("/xrpc/_health")]
async fn health(
    account_manager: AccountManager,
) -> Result<Json<ServerVersion>, status::Custom<Json<ErrorMessageResponse>>> {
    if metrics::METRICS.is_shutting_down() {
        return Err(status::Custom(
            Status::ServiceUnavailable,
            Json(ErrorMessageResponse {
                code: Some(ErrorCode::ServiceUnavailable),
                message: Some("shutting_down".to_string()),
            }),
        ));
    }
    let result = account_manager
        .db
        .run(|conn| Ok(conn.query_row("SELECT 1", [], |row| row.get::<_, i32>(0))?))
        .await;
    match result {
        Ok(_) => {
            let env_version =
                env::var("VERSION").unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_owned());
            let version = ServerVersion {
                version: env_version,
            };
            Ok(Json(version))
        }
        Err(error) => {
            tracing::error!("Internal Error: {error}");
            let internal_error = ErrorMessageResponse {
                code: Some(ErrorCode::ServiceUnavailable),
                message: Some(error.to_string()),
            };
            Err(status::Custom(
                Status::ServiceUnavailable,
                Json(internal_error),
            ))
        }
    }
}

// Pure liveness probe: must never touch the database
#[get("/xrpc/_health/live")]
async fn health_live() -> &'static str {
    "ok"
}

/// Present only while the server serves reads only; a mutating request
/// that reaches it is refused before any handler runs.
pub struct ReadOnlyGate;

#[rocket::async_trait]
impl<'r> rocket::request::FromRequest<'r> for ReadOnlyGate {
    type Error = ();

    async fn from_request(req: &'r Request<'_>) -> rocket::request::Outcome<Self, Self::Error> {
        let read_only = req
            .rocket()
            .state::<config::ServerConfig>()
            .is_some_and(|cfg| cfg.service.read_only);
        if read_only {
            rocket::request::Outcome::Success(ReadOnlyGate)
        } else {
            rocket::request::Outcome::Forward(Status::NotFound)
        }
    }
}

#[rocket::post("/<_..>")]
async fn read_only_post(_gate: ReadOnlyGate) -> ApiError {
    ApiError::ReadOnly
}

#[rocket::put("/<_..>")]
async fn read_only_put(_gate: ReadOnlyGate) -> ApiError {
    ApiError::ReadOnly
}

#[rocket::delete("/<_..>")]
async fn read_only_delete(_gate: ReadOnlyGate) -> ApiError {
    ApiError::ReadOnly
}

#[rocket::patch("/<_..>")]
async fn read_only_patch(_gate: ReadOnlyGate) -> ApiError {
    ApiError::ReadOnly
}

/// What an actor still owes this process; the router and the hand-back
/// tooling read it before moving the actor to another writer.
#[tracing::instrument(skip_all)]
#[get("/xrpc/_drain_status?<did>")]
async fn drain_status(
    _admin: auth_verifier::AdminToken,
    did: String,
    actor_store: &State<ActorStore>,
    repairs: &State<repair::RepairStore>,
) -> Result<Json<drain::DrainStatus>, ApiError> {
    Ok(Json(drain::drain_status(actor_store, repairs, &did).await?))
}

/// The SQLite build this process links and the synchronous modes it uses,
/// for the stage gate's per-process report.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SqliteReport {
    pub version: &'static str,
    pub source_id: String,
    pub synchronous_writes: &'static str,
    pub synchronous_reads: &'static str,
}

#[tracing::instrument(skip_all)]
#[get("/xrpc/_sqlite")]
async fn sqlite_report(
    _admin: auth_verifier::AdminToken,
    account_manager: AccountManager,
) -> Result<Json<SqliteReport>, ApiError> {
    let source_id = account_manager
        .db
        .run(|conn| {
            Ok(conn.query_row("SELECT sqlite_source_id()", [], |row| {
                row.get::<_, String>(0)
            })?)
        })
        .await?;
    Ok(Json(SqliteReport {
        version: rusqlite::version(),
        source_id,
        synchronous_writes: "FULL",
        synchronous_reads: "NORMAL",
    }))
}

#[tracing::instrument(skip_all)]
#[catch(default)]
async fn default_catcher(status: Status, request: &Request<'_>) -> ApiError {
    let api_error: &Option<ApiError> = request.local_cache(|| None);
    match api_error {
        Some(error) => error.clone(),
        None if status == Status::NotFound => ApiError::NotFound,
        None => ApiError::RuntimeError,
    }
}

/// Catches all OPTION requests in order to get the CORS related Fairing triggered.
#[options("/<_..>")]
async fn all_options() {
    /* Intentionally left empty */
}

#[rocket::async_trait]
impl Fairing for CORS {
    fn info(&self) -> Info {
        Info {
            name: "Add CORS headers to responses",
            kind: Kind::Response,
        }
    }

    async fn on_response<'r>(&self, _request: &'r Request<'_>, response: &mut Response<'r>) {
        response.set_header(Header::new("Access-Control-Allow-Origin", "*"));
        response.set_header(Header::new(
            "Access-Control-Allow-Methods",
            "POST, GET, PATCH, OPTIONS, DELETE",
        ));
        response.set_header(Header::new("Access-Control-Allow-Headers", "*"));
        response.set_header(Header::new("Access-Control-Allow-Credentials", "true"));
    }
}

/// Optional overrides for the on-disk service databases, used by tests
/// to point a rocket instance at temporary sqlite files.
#[derive(Debug, Clone, Default)]
pub struct RocketConfig {
    pub service_db: Option<ServiceDbConfig>,
    pub actor_store_directory: Option<String>,
}

pub async fn build_rocket(rocket_cfg: Option<RocketConfig>) -> Rocket<Build> {
    dotenv().ok();

    let mut cfg = env_to_cfg();
    // the reference PDS listens on PDS_PORT on every interface
    // SIGTERM and SIGINT stop accepting connections and let in-flight
    // requests finish within the grace period, as the reference PDS's
    // stop timeout does
    let shutdown = rocket::config::Shutdown {
        ctrlc: true,
        signals: [rocket::config::Sig::Term].into_iter().collect(),
        grace: cfg.service.shutdown_grace_secs,
        mercy: 15,
        force: true,
        ..Default::default()
    };
    let figment = rocket::Config::figment()
        .merge(("port", cfg.service.port as u16))
        .merge(("address", "0.0.0.0"))
        .merge(("shutdown", shutdown))
        .merge(("limits", Limits::default().limit("file", 100.mebibytes())));
    if let Some(rocket_cfg) = rocket_cfg {
        if let Some(service_db) = rocket_cfg.service_db {
            cfg.service_db = service_db;
        }
        if let Some(actor_store_directory) = rocket_cfg.actor_store_directory {
            cfg.actor_store.directory = actor_store_directory;
        }
    }

    let read_only = cfg.service.read_only;
    // read-only: every user and service database is opened so that nothing
    // can change it, and no migration runs; the control journals below
    // stay writable for the watermarks reads record
    let (account_db, sequencer_db, did_cache_db) = if read_only {
        (
            db::sqlite::Db::open_read_only(&cfg.service_db.account_db_location)
                .expect("Failed to open account database"),
            db::sqlite::Db::open_read_only(&cfg.service_db.sequencer_db_location)
                .expect("Failed to open sequencer database"),
            db::sqlite::Db::open_read_only(&cfg.service_db.did_cache_db_location)
                .expect("Failed to open did cache database"),
        )
    } else {
        (
            account_manager::db::get_migrated_db(&cfg.service_db.account_db_location)
                .await
                .expect("Failed to open account database"),
            sequencer::db::get_migrated_db(&cfg.service_db.sequencer_db_location)
                .await
                .expect("Failed to open sequencer database"),
            did_cache::get_migrated_db(&cfg.service_db.did_cache_db_location)
                .await
                .expect("Failed to open did cache database"),
        )
    };
    let lifecycle = lifecycle::LifecycleStore::open(&cfg.service_db.lifecycle_db_location)
        .await
        .expect("Failed to open lifecycle database");

    let background_queue = BackgroundQueue::default();
    let shared_oauth_provider = oauth::SharedOAuthProvider::new(
        account_db.clone(),
        cfg.service.public_url.clone(),
        cfg.service.did.clone(),
        cfg.service.account_ui_sessions_since,
    )
    .await;
    let account_manager = AccountManager::new(account_db);
    let branding = ui::branding::Branding::from_env(&cfg.service.hostname)
        .expect("the branding environment must parse");
    let ui_state = ui::UiState::new(&branding, &cfg.service.public_url, &cfg.service.hostname);

    let sequencer = SharedSequencer {
        sequencer: RwLock::new(Sequencer::with_broadcast_capacity(
            sequencer_db,
            Crawlers::new(cfg.service.hostname.clone(), cfg.crawlers.clone()),
            None,
            cfg.subscription.max_buffer as usize,
        )),
    };
    let mut background_sequencer = sequencer.sequencer.write().await.clone();
    tokio::spawn(async move { background_sequencer.start().await });

    let blob_attempts = blob_attempts::AttemptJournal::open(
        &cfg.service_db.blob_attempts_db_location,
        cfg.service.coexistence,
    )
    .await
    .expect("Failed to open the blob attempt journal");
    let generations =
        blob_generations::Generations::open(&cfg.service_db.blob_generations_db_location)
            .await
            .expect("Failed to open the blob generation registry");
    let blobstore_factory = BlobstoreFactory::from_config(cfg.blobstore.clone())
        .await
        .with_attempts(blob_attempts)
        .with_generations(generations);
    if env_bool("PDS_BLOB_GC_ENABLED").unwrap_or(false) {
        assert!(
            !cfg.service.coexistence,
            "PDS_BLOB_GC_ENABLED is set while PDS_COEXISTENCE is true; the collector never runs while another implementation shares the store"
        );
        if !read_only {
            let interval = std::time::Duration::from_secs(
                env_int("PDS_BLOB_GC_INTERVAL_SECS")
                    .map(|secs| secs.max(60) as u64)
                    .unwrap_or(3600),
            );
            tokio::spawn(async move {
                loop {
                    match cli::Maintenance::from_env().await {
                        Ok(maintenance) => match maintenance.collect_all().await {
                            Ok(reports) => tracing::info!(
                                actors = reports.len(),
                                "blob collector pass complete"
                            ),
                            Err(error) => tracing::error!(%error, "blob collector pass failed"),
                        },
                        Err(error) => {
                            tracing::error!(%error, "blob collector could not open the data directory")
                        }
                    }
                    tokio::time::sleep(interval).await;
                }
            });
        }
    }

    let id_resolver = SharedIdResolver {
        id_resolver: RwLock::new(
            IdResolver::new(IdentityResolverOpts {
                timeout: Some(std::time::Duration::from_millis(
                    cfg.identity.resolver_timeout,
                )),
                plc_url: Some(cfg.identity.plc_url.clone()),
                did_cache: Some(Arc::new(
                    DidSqliteCache::new(
                        did_cache_db,
                        background_queue.clone(),
                        std::time::Duration::from_millis(cfg.identity.cache_state_ttl),
                        std::time::Duration::from_millis(cfg.identity.cache_max_ttl),
                    )
                    .with_read_only(read_only),
                )),
                backup_nameservers: cfg.identity.handle_backup_name_servers.clone(),
            })
            .with_network(outbound::policy()),
        ),
    };

    // Keeping unused for other config purposes for now.
    let app_view_agent = match cfg.bsky_app_view {
        None => SharedATPAgent {
            app_view_agent: None,
        },
        Some(ref bsky_app_view) => {
            let client = ReqwestClientBuilder::new(bsky_app_view.url.clone())
                .client(
                    reqwest::ClientBuilder::new()
                        .user_agent(APP_USER_AGENT)
                        .timeout(std::time::Duration::from_millis(1000))
                        .build()
                        .unwrap(),
                )
                .build();
            SharedATPAgent {
                app_view_agent: Some(RwLock::new(AtpServiceClient::new(client))),
            }
        }
    };
    let local_viewer = SharedLocalViewer {
        local_viewer: RwLock::new(LocalViewer::creator(LocalViewerCreatorParams {
            pds_hostname: cfg.service.hostname.clone(),
            appview_agent: cfg
                .bsky_app_view
                .as_ref()
                .map(|bsky_app_view| bsky_app_view.url.clone()),
            appview_did: cfg
                .bsky_app_view
                .as_ref()
                .map(|bsky_app_view| bsky_app_view.did.clone()),
            appview_cdn_url_pattern: match cfg.bsky_app_view {
                None => None,
                Some(ref bsky_app_view) => bsky_app_view.cdn_url_pattern.clone(),
            },
        })),
    };
    let repairs = repair::RepairStore::open(&cfg.service_db.repair_db_location)
        .await
        .expect("Failed to open the repair journal");
    let admission = Arc::new(match &cfg.service.write_allowlist_file {
        Some(path) => {
            admission::Admission::from_file(path).expect("Failed to load the write allowlist")
        }
        None => admission::Admission::unrestricted(),
    });
    admission.spawn_reloader(std::time::Duration::from_secs(2));
    let lock_dir =
        locks::LockDir::new(&cfg.service_db.lock_dir).expect("Failed to create the lock directory");
    let account_manager = account_manager.with_admission(admission.clone());
    let actor_store = ActorStore::new(&cfg.actor_store, background_queue, lifecycle.clone())
        .with_coexistence(cfg.service.coexistence)
        .with_admission(admission.clone())
        .with_lock_dir(lock_dir)
        .with_read_only(read_only);
    if !read_only {
        let resumed = lifecycle::resume_deletions(&lifecycle::DeletionContext {
            lifecycle: &lifecycle,
            account_manager: &account_manager,
            sequencer: &sequencer,
            actor_store: &actor_store,
            blobstore: None,
        })
        .await
        .expect("Failed to resume incomplete account deletions");
        if !resumed.is_empty() {
            tracing::warn!(
                count = resumed.len(),
                "resumed incomplete account deletions"
            );
        }
        let republished =
            publication::resume_pending_work(&actor_store, &sequencer, &account_manager, |did| {
                blobstore_factory.blobstore(did.to_owned())
            })
            .await
            .expect("Failed to resume publication");
        if !republished.is_empty() {
            tracing::warn!(count = republished.len(), "resumed publication");
        }
    }

    // the gate must outrank every mounted mutating route; explicit ranks
    // in the attribute cannot be negative, so they are set here
    let read_only_gate: Vec<rocket::Route> = rocket::routes![
        read_only_post,
        read_only_put,
        read_only_delete,
        read_only_patch
    ]
    .into_iter()
    .map(|mut route| {
        route.rank = -100;
        route
    })
    .collect();

    let shield = Shield::default().enable(NoSniff::Enable);
    let account_pages: Vec<rocket::Route> = if cfg.service.account_ui_enabled {
        account::routes::routes()
    } else {
        Vec::new()
    };

    rocket::custom(figment)
        .mount("/", read_only_gate)
        .mount("/", account_pages)
        .mount(
            "/",
            routes![
                index,
                robots,
                health,
                health_live,
                metrics_route,
                rate_limited,
                drain_status,
                sqlite_report,
                custom_routes::tls_check,
                custom_routes::custom_well_known_did,
                custom_routes::custom_resolve_handle,
                com::atproto::admin::delete_account::delete_account,
                com::atproto::admin::disable_account_invites::disable_account_invites,
                com::atproto::admin::disable_invite_codes::disable_invite_codes,
                com::atproto::admin::enable_account_invites::enable_account_invites,
                com::atproto::admin::get_account_info::get_account_info,
                com::atproto::admin::get_account_infos::get_account_infos,
                com::atproto::admin::get_invite_codes::get_invite_codes,
                com::atproto::admin::get_subject_status::get_subject_status,
                com::atproto::admin::send_email::send_email,
                com::atproto::admin::update_account_password::update_account_password,
                com::atproto::admin::update_account_email::update_account_email,
                com::atproto::admin::update_account_handle::update_account_handle,
                com::atproto::admin::update_subject_status::update_subject_status,
                com::atproto::identity::refresh_identity::refresh_identity,
                com::atproto::identity::resolve_did::resolve_did,
                com::atproto::identity::resolve_handle::resolve_handle,
                com::atproto::identity::resolve_identity::resolve_identity,
                com::atproto::identity::update_handle::update_handle,
                com::atproto::moderation::create_report::create_report,
                com::atproto::identity::sign_plc_operation::sign_plc_operation,
                com::atproto::identity::get_recommended_did_credentials::get_recommended_did_credentials,
                com::atproto::identity::request_plc_operation_signature::request_plc_operation_signature,
                com::atproto::identity::submit_plc_operation::submit_plc_operation,
                com::atproto::repo::apply_writes::apply_writes,
                com::atproto::repo::create_record::create_record,
                com::atproto::repo::delete_record::delete_record,
                com::atproto::repo::describe_repo::describe_repo,
                com::atproto::repo::get_record::get_record,
                com::atproto::repo::import_repo::import_repo,
                com::atproto::repo::list_records::list_records,
                com::atproto::repo::list_missing_blobs::list_missing_blobs,
                com::atproto::repo::put_record::put_record,
                com::atproto::repo::upload_blob::upload_blob,
                com::atproto::server::confirm_email::confirm_email,
                com::atproto::server::create_account::server_create_account,
                com::atproto::server::create_app_password::create_app_password,
                com::atproto::server::create_invite_code::create_invite_code,
                com::atproto::server::create_invite_codes::create_invite_codes,
                com::atproto::server::create_session::create_session,
                com::atproto::server::deactivate_account::deactivate_account,
                com::atproto::server::delete_account::delete_account,
                com::atproto::server::delete_session::delete_session,
                com::atproto::server::describe_server::describe_server,
                com::atproto::server::check_account_status::check_account_status,
                com::atproto::server::activate_account::activate_account,
                com::atproto::server::get_service_auth::get_service_auth,
                com::atproto::server::get_account_invite_codes::get_account_invite_codes,
                com::atproto::server::get_session::get_session,
                com::atproto::server::list_app_passwords::list_app_passwords,
                com::atproto::server::refresh_session::refresh_session,
                com::atproto::server::request_account_delete::request_account_delete,
                com::atproto::server::request_email_confirmation::request_email_confirmation,
                com::atproto::server::request_email_update::request_email_update,
                com::atproto::server::request_password_reset::request_password_reset,
                com::atproto::server::reset_password::reset_password,
                com::atproto::server::revoke_app_password::revoke_app_password,
                com::atproto::server::update_email::update_email,
                com::atproto::server::reserve_signing_key::reserve_signing_key,
                com::atproto::simplespace::add_member::simplespace_add_member,
                com::atproto::simplespace::create_space::simplespace_create_space,
                com::atproto::simplespace::delete_space::simplespace_delete_space,
                com::atproto::simplespace::list_members::simplespace_list_members,
                com::atproto::simplespace::remove_member::simplespace_remove_member,
                com::atproto::simplespace::update_space::simplespace_update_space,
                com::atproto::space::apply_writes::space_apply_writes,
                com::atproto::space::create_record::space_create_record,
                com::atproto::space::delete_record::space_delete_record,
                com::atproto::space::get_blob::space_get_blob,
                com::atproto::space::get_delegation_token::space_get_delegation_token,
                com::atproto::space::get_latest_commit::space_get_latest_commit,
                com::atproto::space::get_record::space_get_record,
                com::atproto::space::get_repo::space_get_repo,
                com::atproto::space::get_repo_state::space_get_repo_state,
                com::atproto::space::get_space::space_get_space,
                com::atproto::space::get_space_credential::space_get_space_credential,
                com::atproto::space::list_blobs::space_list_blobs,
                com::atproto::space::list_records::space_list_records,
                com::atproto::space::list_repo_ops::space_list_repo_ops,
                com::atproto::space::list_repos::space_list_repos,
                com::atproto::space::list_spaces::space_list_spaces,
                com::atproto::space::notify_space_deleted::space_notify_space_deleted,
                com::atproto::space::notify_write::space_notify_write,
                com::atproto::space::put_record::space_put_record,
                com::atproto::space::register_notify::space_register_notify,
                com::atproto::space::unregister_notify::space_unregister_notify,
                com::atproto::sync::get_blob::get_blob,
                com::atproto::sync::get_blocks::get_blocks,
                com::atproto::sync::get_checkout::get_checkout,
                com::atproto::sync::get_head::get_head,
                com::atproto::sync::get_latest_commit::get_latest_commit,
                com::atproto::sync::get_record::get_record,
                com::atproto::sync::get_repo::get_repo,
                com::atproto::sync::get_repo_status::get_repo_status,
                com::atproto::sync::list_blobs::list_blobs,
                com::atproto::sync::list_repos::list_repos,
                com::atproto::sync::subscribe_repos::subscribe_repos,
                com::atproto::temp::check_signup_queue::check_signup_queue,
                app::bsky::actor::get_preferences::get_preferences,
                app::bsky::actor::get_profile::get_profile,
                app::bsky::actor::get_profiles::get_profiles,
                app::bsky::actor::put_preferences::put_preferences,
                app::bsky::feed::get_actor_likes::get_actor_likes,
                app::bsky::feed::get_author_feed::get_author_feed,
                app::bsky::feed::get_feed::get_feed,
                app::bsky::feed::get_post_thread::get_post_thread,
                app::bsky::feed::get_timeline::get_timeline,
                app::bsky::notification::register_push::register_push,
                app::bsky::notification::unregister_push::unregister_push,
                bsky_api_get_forwarder,
                bsky_api_post_forwarder,
                community::blacksky::pds::get_convergence::get_convergence,
                community::blacksky::pds::get_publication_frontier::get_publication_frontier,
                community::lexicon::service::describe::service_describe,
                well_known::well_known,
                oauth::routes::oauth_par,
                oauth::routes::oauth_token,
                oauth::routes::oauth_revoke,
                oauth::routes::oauth_jwks,
                oauth::routes::oauth_authorization_server_metadata,
                oauth::routes::oauth_protected_resource_metadata,
                oauth::routes::oauth_authorize,
                oauth::routes::oauth_authorize_sign_in,
                oauth::routes::oauth_authorize_select,
                oauth::routes::oauth_authorize_accept,
                oauth::routes::oauth_authorize_reject,
                oauth::routes::oauth_authorize_reactivate,
                ui::assets::ui_asset,
                all_options
            ],
        )
        .register("/", catchers![default_catcher])
        .attach(Telemetry)
        .attach(GlobalRateLimit)
        .attach(CORS)
        .attach(oauth::OAuthHeaders)
        .attach(shield)
        .manage(sequencer)
        .manage(blobstore_factory)
        .manage(id_resolver)
        .manage(cfg)
        .manage(local_viewer)
        .manage(app_view_agent)
        .manage(account_manager)
        .manage(shared_oauth_provider)
        .manage(ui_state)
        .manage(crate::space_auth::SharedSpaceDpop::default())
        .manage(crate::permission_set::SharedPermissionSets::default())
        .manage(actor_store)
        .manage(exports::Exports::from_env())
        .manage(rate_limits::RateLimits::connect_from_env().await)
        .manage(lifecycle)
        .manage(admission)
        .manage(repairs)
}
