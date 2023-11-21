#[macro_use]
extern crate serde_derive;

extern crate serde;
use diesel::pg::PgConnection;
use rocket_sync_db_pools::database;

#[database("pg_db")]
pub struct DbConn(PgConnection);

pub mod db;
pub mod models;
pub mod schema;