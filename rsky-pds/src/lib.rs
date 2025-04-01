#[macro_use]
extern crate serde_derive;
extern crate core;
extern crate mailchecker;
extern crate serde;
use crate::read_after_write::viewer::{LocalViewer, LocalViewerCreator, LocalViewerCreatorParams};
use crate::sequencer::Sequencer;
use atrium_xrpc_client::reqwest::ReqwestClient;
use event_emitter_rs::EventEmitter;
use lazy_static::lazy_static;
pub mod account_manager;
pub mod actor_store;
pub mod apis;
pub mod auth_verifier;
pub mod config;
pub mod context;
pub mod crawlers;
pub mod db;
pub mod handle;
pub mod image;
pub mod lexicon;
pub mod mailer;
pub mod models;
pub mod pipethrough;
pub mod plc;
pub mod read_after_write;
pub mod repo;
pub mod schema;
pub mod sequencer;
pub mod well_known;
pub mod xrpc_server;
use crate::account_manager::AccountManager;
use crate::config::env_to_cfg;
use crate::crawlers::Crawlers;
use crate::db::DbConn;
use crate::models::{ErrorCode, ErrorMessageResponse, ServerVersion};
use diesel::prelude::*;
use rocket::{catch, catchers, get, options, routes, Build, Rocket};

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

// Use lazy_static! because the size of EventEmitter is not known at compile time
lazy_static! {
    // Export the emitter with `pub` keyword
    pub static ref EVENT_EMITTER: RwLock<EventEmitter> = RwLock::new(EventEmitter::new());
}

#[macro_use]
extern crate rocket;
use crate::apis::{app, bsky_api_get_forwarder, bsky_api_post_forwarder, com, ApiError};
use atrium_api::client::AtpServiceClient;
use atrium_xrpc_client::reqwest::ReqwestClientBuilder;
use diesel::prelude::*;
use diesel::sql_types::Int4;
use dotenvy::dotenv;
use rocket::data::{Limits, ToByteUnit};
use rocket::fairing::{Fairing, Info, Kind};
use rocket::figment::{
    util::map,
    value::{Map, Value},
};
use rocket::http::Header;
use rocket::http::Status;
use rocket::request::local_cache;
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::shield::{NoSniff, Shield};
use rocket::{Request, Response};
use rsky_common::env::env_list;
use rsky_identity::types::{DidCache, IdentityResolverOpts};
use rsky_identity::IdResolver;
use std::env;
use tokio::sync::RwLock;

pub struct CORS;

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
    connection: DbConn,
) -> Result<Json<ServerVersion>, status::Custom<Json<ErrorMessageResponse>>> {
    let result = connection
        .run(move |conn| {
            diesel::select(diesel::dsl::sql::<Int4>("1")) // SELECT 1;
                .load::<i32>(conn)
                .map(|v| v.into_iter().next().expect("no results"))
        })
        .await;
    match result {
        Ok(_) => {
            let env_version = env::var("VERSION").unwrap_or("0.3.0-beta.3".into());
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

#[tracing::instrument(skip_all)]
#[catch(default)]
async fn default_catcher(_status: Status, request: &Request<'_>) -> ApiError {
    let api_error: &Option<ApiError> = request.local_cache(|| None);
    match api_error {
        None => ApiError::RuntimeError,
        Some(error) => error.clone(),
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

pub struct RocketConfig {
    pub db_url: String,
}

pub async fn build_rocket(cfg: Option<RocketConfig>) -> Rocket<Build> {
    dotenv().ok();

    let db_url = if let Some(cfg) = cfg {
        cfg.db_url
    } else {
        env::var("DATABASE_URL").unwrap_or("".into())
    };

    let db: Map<_, Value> = map! {
        "url" => db_url.into(),
        "pool_size" => 20.into(),
        "timeout" => 30.into(),
    };

    let figment = rocket::Config::figment()
        .merge(("databases", map!["pg_db" => db]))
        .merge(("limits", Limits::default().limit("file", 100.mebibytes())));
    let cfg = env_to_cfg();

    let sequencer = SharedSequencer {
        sequencer: RwLock::new(Sequencer::new(
            Crawlers::new(cfg.service.hostname.clone(), cfg.crawlers.clone()),
            None,
        )),
    };
    let mut background_sequencer = sequencer.sequencer.write().await.clone();
    tokio::spawn(async move { background_sequencer.start().await });

    let aws_sdk_config = aws_config::from_env()
        .endpoint_url(env::var("AWS_ENDPOINT").unwrap_or("localhost".to_owned()))
        .load()
        .await;

    let id_resolver = SharedIdResolver {
        id_resolver: RwLock::new(IdResolver::new(IdentityResolverOpts {
            timeout: None,
            plc_url: Some(
                env::var("PDS_DID_PLC_URL").unwrap_or("https://plc.directory".to_owned()),
            ),
            did_cache: Some(DidCache::new(None, None)),
            backup_nameservers: Some(env_list("PDS_HANDLE_BACKUP_NAMESERVERS")),
        })),
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
            account_manager: AccountManager {},
            pds_hostname: cfg.service.hostname.clone(),
            appview_agent: match cfg.bsky_app_view {
                None => None,
                Some(ref bsky_app_view) => Some(bsky_app_view.url.clone()),
            },
            appview_did: match cfg.bsky_app_view {
                None => None,
                Some(ref bsky_app_view) => Some(bsky_app_view.did.clone()),
            },
            appview_cdn_url_pattern: match cfg.bsky_app_view {
                None => None,
                Some(ref bsky_app_view) => bsky_app_view.cdn_url_pattern.clone(),
            },
        })),
    };

    let shield = Shield::default().enable(NoSniff::Enable);

    rocket::custom(figment)
        .mount(
            "/",
            routes![
                index,
                robots,
                health,
                com::atproto::admin::delete_account::delete_account,
                com::atproto::admin::disable_account_invites::disable_account_invites,
                com::atproto::admin::disable_invite_codes::disable_invite_codes,
                com::atproto::admin::enable_account_invites::enable_account_invites,
                com::atproto::admin::get_account_info::get_account_info,
                com::atproto::admin::get_invite_codes::get_invite_codes,
                com::atproto::admin::get_subject_status::get_subject_status,
                com::atproto::admin::send_email::send_email,
                com::atproto::admin::update_account_password::update_account_password,
                com::atproto::admin::update_account_email::update_account_email,
                com::atproto::admin::update_account_handle::update_account_handle,
                com::atproto::admin::update_subject_status::update_subject_status,
                com::atproto::identity::resolve_handle::resolve_handle,
                com::atproto::identity::update_handle::update_handle,
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
                com::atproto::sync::get_blob::get_blob,
                com::atproto::sync::get_blocks::get_blocks,
                com::atproto::sync::get_latest_commit::get_latest_commit,
                com::atproto::sync::get_record::get_record,
                com::atproto::sync::get_repo::get_repo,
                com::atproto::sync::get_repo_status::get_repo_status,
                com::atproto::sync::list_blobs::list_blobs,
                com::atproto::sync::list_repos::list_repos,
                com::atproto::sync::list_repos_by_collection::list_repos_by_collection,
                com::atproto::sync::subscribe_repos::subscribe_repos,
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
                bsky_api_get_forwarder,
                bsky_api_post_forwarder,
                well_known::well_known,
                all_options
            ],
        )
        .register("/", catchers![default_catcher])
        .attach(CORS)
        .attach(DbConn::fairing())
        .attach(shield)
        .manage(sequencer)
        .manage(aws_sdk_config)
        .manage(id_resolver)
        .manage(cfg)
        .manage(local_viewer)
        .manage(app_view_agent)
}
