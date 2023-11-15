#[macro_use]
extern crate rocket;
use dotenvy::dotenv;
use rocket::fairing::{Fairing, Info, Kind};
use rocket::http::Header;
use rocket::{Request, Response};
use rocket::figment::{
    util::map,
    value::{Map, Value},
};
use rsky_pds::DbConn;
use std::env;

pub struct CORS;

#[get("/")]
fn index() -> &'static str {
    "This is an AT Protocol Personal Data Server (PDS): https://github.com/bluesky-social/atproto\n\nMost API routes are under /xrpc/"
}

#[get("/robots.txt")]
fn robots() -> &'static str {
    "# Hello!\n\n# Crawling the public API is allowed\nUser-agent: *\nAllow: /"
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
                all_options
            ],
        )
        .attach(CORS)
        .attach(DbConn::fairing())
}
