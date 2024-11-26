#[macro_use]
extern crate rocket;
use dotenvy::dotenv;
use rocket::fairing::{Fairing, Info, Kind};
use rocket::figment::{
    util::map,
    value::{Map, Value},
};
use rocket::http::Header;
use rocket::serde::json::Json;
use rocket::{Request, Response};
use rsky_feedgen::routes::*;
use rsky_feedgen::{FeedGenConfig, ReadReplicaConn1, ReadReplicaConn2, WriteDbConn};
use std::env;

pub struct CORS;

#[catch(404)]
async fn not_found() -> Json<rsky_feedgen::models::PathUnknownErrorMessageResponse> {
    let path_error = rsky_feedgen::models::PathUnknownErrorMessageResponse {
        code: Some(rsky_feedgen::models::NotFoundErrorCode::UndefinedEndpoint),
        message: Some("Not Found".to_string()),
    };
    Json(path_error)
}

#[catch(422)]
async fn unprocessable_entity() -> Json<rsky_feedgen::models::ValidationErrorMessageResponse> {
    let validation_error = rsky_feedgen::models::ValidationErrorMessageResponse {
        code: Some(rsky_feedgen::models::ErrorCode::ValidationError),
        message: Some(
            "The request was well-formed but was unable to be followed due to semantic errors."
                .to_string(),
        ),
    };
    Json(validation_error)
}

#[catch(400)]
async fn bad_request() -> Json<rsky_feedgen::models::ValidationErrorMessageResponse> {
    let validation_error = rsky_feedgen::models::ValidationErrorMessageResponse {
        code: Some(rsky_feedgen::models::ErrorCode::ValidationError),
        message: Some("The request was improperly formed.".to_string()),
    };
    Json(validation_error)
}

#[catch(401)]
async fn unauthorized() -> Json<rsky_feedgen::models::InternalErrorMessageResponse> {
    let internal_error = rsky_feedgen::models::InternalErrorMessageResponse {
        code: Some(rsky_feedgen::models::InternalErrorCode::Unavailable),
        message: Some("Request could not be processed.".to_string()),
    };
    Json(internal_error)
}

#[catch(default)]
async fn default_catcher() -> Json<rsky_feedgen::models::InternalErrorMessageResponse> {
    let internal_error = rsky_feedgen::models::InternalErrorMessageResponse {
        code: Some(rsky_feedgen::models::InternalErrorCode::InternalError),
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
fn rocket() -> _ {
    dotenv().ok();

    let write_database_url = env::var("DATABASE_URL").unwrap_or("".into());
    let read_database_url_1 = env::var("READ_REPLICA_URL_1").unwrap_or("".into());
    let read_database_url_2 = env::var("READ_REPLICA_URL_2").unwrap_or("".into());

    let write_db: Map<_, Value> = map! {
        "url" => write_database_url.into(),
        "pool_size" => 20.into(),
        "timeout" => 30.into(),
    };

    let read_db_1: Map<_, Value> = map! {
        "url" => read_database_url_1.into(),
        "pool_size" => 20.into(),
        "timeout" => 30.into(),
    };

    let read_db_2: Map<_, Value> = map! {
        "url" => read_database_url_2.into(),
        "pool_size" => 20.into(),
        "timeout" => 30.into(),
    };

    let figment = rocket::Config::figment().merge((
        "databases",
        map![
            "pg_read_replica_1" => read_db_1,
            "pg_read_replica_2" => read_db_2,
            "pg_db" => write_db
        ],
    ));

    let feedgen_config = FeedGenConfig {
        show_sponsored_post: env::var("SHOW_SPONSORED_POST").unwrap_or("0".to_string()) == "1",
        sponsored_post_uri: env::var("SPONSORED_POST_URI").unwrap_or("".to_string()),
        sponsored_post_probability: match env::var("SPONSORED_POST_PROBABILITY") {
            Err(_) => 0.05,
            Ok(probability) => match probability.parse::<f64>() {
                Err(_) => 0.05,
                Ok(probability) => probability,
            },
        },
        trending_percentile_min: match env::var("TRENDING_PERCENTILE") {
            Err(_) => 0.9,
            Ok(percentile) => match percentile.parse::<f64>() {
                Err(_) => 0.9,
                Ok(percentile) => percentile,
            },
        },
    };

    rocket::custom(figment)
        .mount(
            "/",
            routes![
                index,
                queue_creation,
                queue_deletion,
                well_known,
                get_cursor,
                update_cursor,
                all_options
            ],
        )
        .register(
            "/",
            catchers![
                default_catcher,
                unprocessable_entity,
                bad_request,
                not_found,
                unauthorized
            ],
        )
        .attach(CORS)
        .attach(WriteDbConn::fairing())
        .attach(ReadReplicaConn1::fairing())
        .attach(ReadReplicaConn2::fairing())
        .manage(feedgen_config)
}
