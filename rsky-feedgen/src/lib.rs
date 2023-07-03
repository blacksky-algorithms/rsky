#[macro_use]
extern crate serde_derive;

extern crate serde;
extern crate serde_json;

extern crate lexicon;
extern crate lazy_static;

use rocket_sync_db_pools::{database};
use diesel::pg::PgConnection;

#[database("pg_db")]
pub struct WriteDbConn(PgConnection);

#[database("pg_read_replica")]
pub struct ReadReplicaConn(PgConnection);

pub mod schema;
pub mod models;
pub mod db;
pub mod apis;