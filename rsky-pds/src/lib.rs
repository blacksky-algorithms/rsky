#[macro_use]
extern crate serde_derive;
extern crate core;
extern crate mailchecker;
extern crate rocket;
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
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::shield::{NoSniff, Shield};
use rocket::{catch, catchers, get, options, routes, Build, Request, Response, Rocket};
use rsky_common::env::env_list;
use rsky_identity::types::{DidCache, IdentityResolverOpts};
use rsky_identity::IdResolver;
use std::env;
use tokio::sync::RwLock;

pub struct CORS;

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

#[get("/")]
pub async fn index() -> &'static str {
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
pub async fn robots() -> &'static str {
    "# Hello!\n\n# Crawling the public API is allowed\nUser-agent: *\nAllow: /"
}

#[tracing::instrument(skip_all)]
#[get("/xrpc/_health")]
pub async fn health(
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

#[catch(default)]
async fn default_catcher() -> Json<ErrorMessageResponse> {
    let internal_error = ErrorMessageResponse {
        code: Some(ErrorCode::InternalServerError),
        message: Some("Internal error.".to_string()),
    };
    Json(internal_error)
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

pub async fn build_rocket() -> Rocket<Build> {
    dotenv().ok();

    let subscriber = tracing_subscriber::FmtSubscriber::new();
    tracing::subscriber::set_global_default(subscriber).unwrap();

    let db_url = env::var("DATABASE_URL").unwrap_or("".into());

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
                crate::apis::com::atproto::admin::delete_account::delete_account,
                crate::apis::com::atproto::admin::disable_account_invites::disable_account_invites,
                crate::apis::com::atproto::admin::disable_invite_codes::disable_invite_codes,
                crate::apis::com::atproto::admin::enable_account_invites::enable_account_invites,
                crate::apis::com::atproto::admin::get_account_info::get_account_info,
                crate::apis::com::atproto::admin::get_invite_codes::get_invite_codes,
                crate::apis::com::atproto::admin::get_subject_status::get_subject_status,
                crate::apis::com::atproto::admin::send_email::send_email,
                crate::apis::com::atproto::admin::update_account_password::update_account_password,
                crate::apis::com::atproto::admin::update_account_email::update_account_email,
                crate::apis::com::atproto::admin::update_account_handle::update_account_handle,
                crate::apis::com::atproto::admin::update_subject_status::update_subject_status,
                crate::apis::com::atproto::identity::resolve_handle::resolve_handle,
                crate::apis::com::atproto::identity::update_handle::update_handle,
                crate::apis::com::atproto::repo::apply_writes::apply_writes,
                crate::apis::com::atproto::repo::create_record::create_record,
                crate::apis::com::atproto::repo::delete_record::delete_record,
                crate::apis::com::atproto::repo::describe_repo::describe_repo,
                crate::apis::com::atproto::repo::get_record::get_record,
                crate::apis::com::atproto::repo::import_repo::import_repo,
                crate::apis::com::atproto::repo::list_records::list_records,
                crate::apis::com::atproto::repo::list_missing_blobs::list_missing_blobs,
                crate::apis::com::atproto::repo::put_record::put_record,
                crate::apis::com::atproto::repo::upload_blob::upload_blob,
                crate::apis::com::atproto::server::confirm_email::confirm_email,
                crate::apis::com::atproto::server::create_account::server_create_account,
                crate::apis::com::atproto::server::create_app_password::create_app_password,
                crate::apis::com::atproto::server::create_invite_code::create_invite_code,
                crate::apis::com::atproto::server::create_invite_codes::create_invite_codes,
                crate::apis::com::atproto::server::create_session::create_session,
                crate::apis::com::atproto::server::deactivate_account::deactivate_account,
                crate::apis::com::atproto::server::delete_account::delete_account,
                crate::apis::com::atproto::server::delete_session::delete_session,
                crate::apis::com::atproto::server::describe_server::describe_server,
                crate::apis::com::atproto::server::check_account_status::check_account_status,
                crate::apis::com::atproto::server::activate_account::activate_account,
                crate::apis::com::atproto::server::get_service_auth::get_service_auth,
                crate::apis::com::atproto::server::get_account_invite_codes::get_account_invite_codes,
                crate::apis::com::atproto::server::get_session::get_session,
                crate::apis::com::atproto::server::list_app_passwords::list_app_passwords,
                crate::apis::com::atproto::server::refresh_session::refresh_session,
                crate::apis::com::atproto::server::request_account_delete::request_account_delete,
                crate::apis::com::atproto::server::request_email_confirmation::request_email_confirmation,
                crate::apis::com::atproto::server::request_email_update::request_email_update,
                crate::apis::com::atproto::server::request_password_reset::request_password_reset,
                crate::apis::com::atproto::server::reset_password::reset_password,
                crate::apis::com::atproto::server::revoke_app_password::revoke_app_password,
                crate::apis::com::atproto::server::update_email::update_email,
                crate::apis::com::atproto::server::reserve_signing_key::reserve_signing_key,
                crate::apis::com::atproto::sync::get_blob::get_blob,
                crate::apis::com::atproto::sync::get_blocks::get_blocks,
                crate::apis::com::atproto::sync::get_latest_commit::get_latest_commit,
                crate::apis::com::atproto::sync::get_record::get_record,
                crate::apis::com::atproto::sync::get_repo::get_repo,
                crate::apis::com::atproto::sync::get_repo_status::get_repo_status,
                crate::apis::com::atproto::sync::list_blobs::list_blobs,
                crate::apis::com::atproto::sync::list_repos::list_repos,
                crate::apis::com::atproto::sync::subscribe_repos::subscribe_repos,
                crate::apis::app::bsky::actor::get_preferences::get_preferences,
                crate::apis::app::bsky::actor::get_profile::get_profile,
                crate::apis::app::bsky::actor::get_profiles::get_profiles,
                crate::apis::app::bsky::actor::put_preferences::put_preferences,
                crate::apis::app::bsky::feed::get_actor_likes::get_actor_likes,
                crate::apis::app::bsky::feed::get_author_feed::get_author_feed,
                crate::apis::app::bsky::feed::get_feed::get_feed,
                crate::apis::app::bsky::feed::get_post_thread::get_post_thread,
                crate::apis::app::bsky::feed::get_timeline::get_timeline,
                crate::apis::app::bsky::notification::register_push::register_push,
                crate::apis::bsky_api_get_forwarder,
                crate::apis::bsky_api_post_forwarder,
                crate::well_known::well_known,
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
