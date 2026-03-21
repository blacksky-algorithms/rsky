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
use crate::account_manager::{AccountManager, SharedAccountManager};
use crate::config::env_to_cfg;
use crate::config::ServerConfig;
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

extern crate rocket;
use crate::apis::{app, bsky_api_get_forwarder, bsky_api_post_forwarder, com, ApiError};
use atrium_api::client::AtpServiceClient;
use atrium_xrpc_client::reqwest::ReqwestClientBuilder;
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
use rocket::{Request, Response};
use rsky_identity::types::{DidCache, IdentityResolverOpts};
use rsky_identity::IdResolver;
use std::env;
use std::time::Duration;
use tokio::sync::RwLock;

pub struct CORS;

#[get("/")]
async fn index() -> rocket::response::content::RawHtml<&'static str> {
    rocket::response::content::RawHtml(r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>rsky-pds — ATProto Personal Data Server</title>
<style>
:root { --primary: #27C58B; --bg: #1b1b1b; --card: #2d2d2d; --text: #fff; --muted: #999; --code: #41444e; }
* { box-sizing: border-box; margin: 0; padding: 0; }
body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; background: var(--bg); color: var(--text); line-height: 1.6; }
.container { max-width: 700px; margin: 0 auto; padding: 2rem; }
h1 { color: var(--primary); margin-bottom: 0.3rem; font-size: 1.8rem; }
h2 { color: var(--primary); margin-top: 1.5rem; font-size: 1.1rem; }
.subtitle { color: var(--muted); margin-bottom: 1.5rem; }
a { color: var(--primary); }
code { background: var(--code); padding: 2px 6px; border-radius: 3px; font-size: 0.9em; }
.endpoint { margin: 0.3rem 0; }
.card { background: var(--card); border-radius: 8px; padding: 1rem; margin: 1rem 0; }
input, button { font-size: 1rem; padding: 0.5rem 0.8rem; border-radius: 6px; border: 1px solid #555; }
input { background: var(--code); color: var(--text); width: 100%; margin-bottom: 0.5rem; }
button { background: var(--primary); color: #000; border: none; cursor: pointer; font-weight: 600; }
button:hover { background: #1fa06f; }
#result { background: var(--code); padding: 1rem; border-radius: 6px; margin-top: 0.5rem; white-space: pre-wrap; word-break: break-all; font-family: monospace; font-size: 0.85rem; max-height: 400px; overflow-y: auto; display: none; }
.footer { margin-top: 2rem; color: var(--muted); font-size: 0.85rem; border-top: 1px solid #333; padding-top: 1rem; }
</style>
</head>
<body>
<div class="container">
<h1>rsky-pds</h1>
<p class="subtitle">ATProto Personal Data Server — powered by <a href="https://github.com/blacksky-algorithms/rsky">rsky</a></p>

<h2>Query This PDS</h2>
<div class="card">
<input type="text" id="did" placeholder="Enter DID or handle (e.g., did:plc:... or user.example.com)">
<button onclick="query()">Look Up</button>
<div id="result"></div>
</div>

<h2>XRPC Endpoints</h2>
<div class="endpoint"><code>GET</code> <a href="/xrpc/_health">/xrpc/_health</a> — Health check</div>
<div class="endpoint"><code>GET</code> <a href="/xrpc/com.atproto.server.describeServer">/xrpc/com.atproto.server.describeServer</a> — Server info</div>
<div class="endpoint"><code>POST</code> /xrpc/com.atproto.server.createSession — Authenticate</div>
<div class="endpoint"><code>POST</code> /xrpc/com.atproto.server.createAccount — Create account</div>
<div class="endpoint"><code>GET</code> /xrpc/com.atproto.sync.listRepos — List hosted repos</div>
<div class="endpoint"><code>GET</code> /xrpc/com.atproto.repo.listRecords — List records in a repo</div>
<div class="endpoint"><code>GET</code> /xrpc/com.atproto.repo.getRecord — Get a single record</div>

<h2>About</h2>
<p>This PDS hosts ATProto repositories for users. It stores identity documents, posts, profiles, and other ATProto records. Data is synchronized with the broader AT Protocol network via the <a href="https://atproto.com">ATProto</a> federation protocol.</p>

<div class="footer">
<a href="https://atproto.com">ATProto</a> · <a href="https://github.com/blacksky-algorithms/rsky">rsky on GitHub</a> · <a href="/xrpc/_health">Health</a>
</div>
</div>
<script>
async function query() {
  const input = document.getElementById('did').value.trim();
  const el = document.getElementById('result');
  if (!input) return;
  el.style.display = 'block';
  el.textContent = 'Loading...';
  try {
    // Try listRecords for posts
    const url = input.startsWith('did:')
      ? `/xrpc/com.atproto.repo.listRecords?repo=${encodeURIComponent(input)}&collection=app.bsky.feed.post&limit=10`
      : `/xrpc/com.atproto.identity.resolveHandle?handle=${encodeURIComponent(input)}`;
    const resp = await fetch(url);
    const data = await resp.json();
    if (data.did && !input.startsWith('did:')) {
      // Resolved handle to DID, now fetch posts
      el.textContent = `Handle: ${input}\nDID: ${data.did}\n\nFetching posts...\n`;
      const postsResp = await fetch(`/xrpc/com.atproto.repo.listRecords?repo=${data.did}&collection=app.bsky.feed.post&limit=10`);
      const posts = await postsResp.json();
      let out = `Handle: ${input}\nDID: ${data.did}\n\nPosts (${(posts.records||[]).length}):\n`;
      for (const r of (posts.records || [])) {
        const v = r.value?.value || r.value || {};
        out += `\n  ${v.createdAt || '?'}\n  ${v.text || '(no text)'}\n  URI: ${r.uri}\n`;
      }
      el.textContent = out;
    } else {
      el.textContent = JSON.stringify(data, null, 2);
    }
  } catch (e) {
    el.textContent = `Error: ${e.message}`;
  }
}
document.getElementById('did').addEventListener('keypress', e => { if (e.key === 'Enter') query(); });
</script>
</body>
</html>"#)
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

fn build_id_resolver(cfg: &ServerConfig) -> SharedIdResolver {
    SharedIdResolver {
        id_resolver: RwLock::new(IdResolver::new(IdentityResolverOpts {
            timeout: Some(Duration::from_millis(cfg.identity.resolver_timeout)),
            plc_url: Some(cfg.identity.plc_url.clone()),
            did_cache: Some(DidCache::new(
                Some(Duration::from_millis(cfg.identity.cache_state_ttl)),
                Some(Duration::from_millis(cfg.identity.cache_max_ttl)),
            )),
            backup_nameservers: cfg.identity.handle_backup_name_servers.clone(),
        })),
    }
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

    let id_resolver = build_id_resolver(&cfg);

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
    let account_manager = SharedAccountManager {
        account_manager: RwLock::new(AccountManager::creator()),
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
        .manage(account_manager)
}

#[cfg(test)]
mod tests {
    use crate::build_id_resolver;
    use crate::config::{CoreConfig, IdentityConfig, InvitesConfig, ServerConfig, SubscriptionConfig};
    use rsky_identity::did::did_resolver::ResolverKind;
    use std::time::Duration;

    #[test]
    fn build_id_resolver_uses_identity_config_timeout_and_cache_ttls() {
        let cfg = ServerConfig {
            service: CoreConfig {
                port: 8000,
                hostname: "pds.staging.dvines.org".to_string(),
                public_url: "https://pds.staging.dvines.org".to_string(),
                did: "did:web:pds.staging.dvines.org".to_string(),
                version: None,
                privacy_policy_url: None,
                terms_of_service_url: None,
                accepting_imports: true,
                blob_upload_limit: 1024,
                contact_email_address: None,
                dev_mode: false,
            },
            mod_service: None,
            report_service: None,
            bsky_app_view: None,
            subscription: SubscriptionConfig {
                max_buffer: 100,
                repo_backfill_limit_ms: 1000,
            },
            invites: InvitesConfig {
                required: false,
                interval: None,
                epoch: None,
            },
            identity: IdentityConfig {
                plc_url: "https://plc.directory".to_string(),
                resolver_timeout: 30_000,
                cache_state_ttl: 60_000,
                cache_max_ttl: 120_000,
                recovery_did_key: None,
                service_handle_domains: vec![".staging.dvines.org".to_string()],
                handle_backup_name_servers: Some(vec!["1.1.1.1".to_string()]),
                enable_did_doc_with_session: false,
            },
            crawlers: vec![],
        };

        let id_resolver = build_id_resolver(&cfg).id_resolver.into_inner();

        match id_resolver.did.methods.get("plc") {
            Some(ResolverKind::Plc(plc)) => {
                assert_eq!(plc.plc_url, "https://plc.directory");
                assert_eq!(plc.timeout, Duration::from_millis(30_000));
            }
            other => panic!("unexpected plc resolver: {other:?}"),
        }

        let cache = id_resolver.did.cache.expect("did cache should be configured");
        assert_eq!(cache.stale_ttl, Duration::from_millis(60_000));
        assert_eq!(cache.max_ttl, Duration::from_millis(120_000));
    }
}
