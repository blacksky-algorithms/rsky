#[macro_use]
extern crate rocket;
use dotenvy::dotenv;
use rocket::fairing::{Fairing, Info, Kind};
use rocket::http::Header;
use rocket::{Request, Response};
use rocket::serde::json::Json;
use rocket::http::Status;
use rocket::response::status;
use rocket::figment::{
    util::map,
    value::{Map, Value},
};
use rsky_pds::DbConn;
use std::env;
use diesel::sql_types::Int4;
use diesel::prelude::*;

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
    connection: DbConn
) -> Result<
    Json<rsky_pds::models::ServerVersion>,
    status::Custom<Json<rsky_pds::models::InternalErrorMessageResponse>>,
> {
    let result = connection
        .run(move |conn| {
            diesel::select(diesel::dsl::sql::<Int4>("1")) // SELECT 1;
                .load::<i32>(conn)
                .map(|v| v.into_iter().next().expect("no results"))
        }).await;
    match result {
        Ok(_) => {
            let env_version = env::var("VERSION").unwrap_or("0.3.0-beta.3".into());
            let version = rsky_pds::models::ServerVersion {
                version: env_version
            };
            Ok(Json(version))
        }
        Err(error) => { // TO DO: Throw 503
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
fn rocket() -> _ {
    dotenv().ok();

    let db_url = env::var("DATABASE_URL").unwrap_or("".into());

    let db: Map<_, Value> = map! {
        "url" => db_url.into(),
        "pool_size" => 20.into(),
        "timeout" => 30.into(),
    };

    let figment = rocket::Config::figment().merge((
        "databases",
        map!["pg_db" => db],
    ));

    rocket::custom(figment)
        .mount(
            "/",
            routes![
                index,
                robots,
                health,
                all_options
            ],
        )
        .register(
            "/",
            catchers![
                default_catcher
            ],
        )
        .attach(CORS)
        .attach(DbConn::fairing())
}
