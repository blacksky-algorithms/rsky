#[macro_use]
extern crate serde_derive;

extern crate serde;
extern crate serde_json;

extern crate rsky_lexicon;

use diesel::pg::PgConnection;
use rocket_sync_db_pools::database;

#[database("pg_db")]
pub struct WriteDbConn(PgConnection);

#[database("pg_read_replica")]
pub struct ReadReplicaConn(PgConnection);

#[derive(Clone)]
pub struct FeedGenConfig {
    pub show_sponsored_post: bool,
    pub sponsored_post_uri: String,
    pub sponsored_post_probability: f64,
}

pub mod apis;
pub mod auth;
pub mod db;
pub mod explicit_slurs;
pub mod models;
pub mod routes;
pub mod schema;
