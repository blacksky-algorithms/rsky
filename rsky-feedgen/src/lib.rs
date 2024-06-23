#[macro_use]
extern crate serde_derive;

extern crate serde;
extern crate serde_json;

extern crate lazy_static;
extern crate rsky_lexicon;

use diesel::pg::PgConnection;
use rocket_sync_db_pools::database;

#[database("pg_db")]
pub struct WriteDbConn(PgConnection);

#[database("pg_read_replica")]
pub struct ReadReplicaConn(PgConnection);

pub mod apis;
pub mod auth;
pub mod db;
pub mod models;
pub mod schema;
