#[macro_use]
extern crate rocket;
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
use rocket::{Request, Response};
use rsky_identity::did::did_resolver::DidResolver;
use rsky_identity::types::{DidCache, DidResolverOpts, IdentityResolverOpts};
use rsky_identity::IdResolver;
use rsky_pds::apis::*;
use rsky_pds::common::env::env_list;
use rsky_pds::crawlers::Crawlers;
use rsky_pds::sequencer::Sequencer;
use rsky_pds::SharedSequencer;
use rsky_pds::{DbConn, SharedIdResolver};
use std::env;
use tokio::sync::RwLock;

pub struct CORS;

#[get("/")]
async fn index() -> &'static str {
    "This is an AT Protocol Personal Data Server (PDS): https://github.com/blacksky-algorithms/rsky\n\nMost API routes are under /xrpc/"
}

#[get("/robots.txt")]
async fn robots() -> &'static str {
    "# Hello!\n\n# Crawling the public API is allowed\nUser-agent: *\nAllow: /"
}

#[get("/xrpc/_health")]
async fn health(
    connection: DbConn,
) -> Result<
    Json<rsky_pds::models::ServerVersion>,
    status::Custom<Json<rsky_pds::models::InternalErrorMessageResponse>>,
> {
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
            let version = rsky_pds::models::ServerVersion {
                version: env_version,
            };
            Ok(Json(version))
        }
        Err(error) => {
            // TO DO: Throw 503
            eprintln!("Internal Error: {error}");
            let internal_error = rsky_pds::models::InternalErrorMessageResponse {
                code: Some(rsky_pds::models::InternalErrorCode::InternalError),
                message: Some(error.to_string()),
            };
            Err(status::Custom(
                Status::InternalServerError,
                Json(internal_error),
            ))
        }
    }
}

#[catch(default)]
async fn default_catcher() -> Json<rsky_pds::models::InternalErrorMessageResponse> {
    let internal_error = rsky_pds::models::InternalErrorMessageResponse {
        code: Some(rsky_pds::models::InternalErrorCode::InternalError),
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

#[launch]
async fn rocket() -> _ {
    dotenv().ok();

    let db_url = env::var("DATABASE_URL").unwrap_or("".into());

    let db: Map<_, Value> = map! {
        "url" => db_url.into(),
        "pool_size" => 20.into(),
        "timeout" => 30.into(),
    };

    let figment = rocket::Config::figment()
        .merge(("databases", map!["pg_db" => db]))
        .merge(("limits", Limits::default().limit("file", 100.mebibytes())));

    let sequencer = SharedSequencer {
        sequencer: RwLock::new(Sequencer::new(
            Crawlers::new(
                env::var("PDS_HOSTNAME").unwrap_or("localhost".to_owned()),
                vec![env::var("PDS_CRAWLER").unwrap_or("https://bgs.bsky-sandbox.dev".to_owned())],
            ),
            None,
        )),
    };

    let config = aws_config::from_env()
        .endpoint_url(env::var("AWS_ENDPOINT").unwrap_or("localhost".to_owned()))
        .load()
        .await;

    let id_resolver = SharedIdResolver {
        id_resolver: RwLock::new(IdResolver::new(IdentityResolverOpts {
            timeout: None,
            plc_url: Some(format!(
                "https://{}",
                env::var("PLC_SERVER").unwrap_or("plc.directory".to_owned())
            )),
            did_cache: Some(DidCache::new(None, None)),
            backup_nameservers: Some(env_list("PDS_HANDLE_BACKUP_NAMESERVERS")),
        })),
    };

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
                com::atproto::label::query_labels::query_labels,
                com::atproto::label::subscribe_labels::subscribe_labels,
                com::atproto::moderation::create_report::create_report,
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
                com::atproto::sync::list_blobs::list_blobs,
                com::atproto::sync::list_repos::list_repos,
                com::atproto::sync::subscribe_repos::subscribe_repos,
                com::atproto::sync::get_checkout::get_checkout,
                com::atproto::sync::get_head::get_head,
                com::atproto::sync::notify_of_update::notify_of_update,
                com::atproto::sync::request_crawl::request_crawl,
                app::bsky::actor::get_preferences::get_preferences,
                app::bsky::actor::get_profile::get_profile,
                app::bsky::actor::get_profiles::get_profiles,
                app::bsky::actor::get_suggestions::get_suggestions,
                app::bsky::actor::put_preferences::put_preferences,
                app::bsky::actor::search_actors::search_actors,
                app::bsky::actor::search_actors_typeahead::search_actors_typeahead,
                app::bsky::feed::describe_feed_generator::describe_feed_generator,
                app::bsky::feed::get_actor_feeds::get_actor_feeds,
                app::bsky::feed::get_actor_likes::get_actor_likes,
                app::bsky::feed::get_author_feed::get_author_feed,
                app::bsky::feed::get_feed::get_feed,
                app::bsky::feed::get_feed_generator::get_feed_generator,
                app::bsky::feed::get_feed_generators::get_feed_generators,
                app::bsky::feed::get_likes::get_likes,
                app::bsky::feed::get_list_feed::get_list_feed,
                app::bsky::feed::get_post_thread::get_post_thread,
                app::bsky::feed::get_posts::get_posts,
                app::bsky::feed::get_reposted_by::get_reposted_by,
                app::bsky::feed::get_suggested_feeds::get_suggested_feeds,
                app::bsky::feed::get_timeline::get_timeline,
                app::bsky::feed::search_posts::search_posts,
                app::bsky::graph::get_blocks::get_blocks,
                app::bsky::graph::get_followers::get_followers,
                app::bsky::graph::get_follows::get_follows,
                app::bsky::graph::get_list::get_list,
                app::bsky::graph::get_list_blocks::get_list_blocks,
                app::bsky::graph::get_list_mutes::get_list_mutes,
                app::bsky::graph::get_lists::get_lists,
                app::bsky::graph::get_mutes::get_mutes,
                app::bsky::graph::get_suggested_follows_by_actor::get_suggested_follows_by_actor,
                app::bsky::notification::get_unread_count::get_unread_count,
                app::bsky::notification::list_notifications::list_notifications,
                app::bsky::notification::register_push::register_push,
                app::bsky::notification::update_seen::update_seen,
                app::bsky::unspecced::get_popular_feed_generators::get_popular_feed_generators,
                all_options
            ],
        )
        .register("/", catchers![default_catcher])
        .attach(CORS)
        .attach(DbConn::fairing())
        .manage(sequencer)
        .manage(config)
        .manage(id_resolver)
}
