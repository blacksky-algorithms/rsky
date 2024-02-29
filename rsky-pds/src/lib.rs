#[macro_use]
extern crate serde_derive;
extern crate rocket;
extern crate serde;
extern crate core;

use diesel::pg::PgConnection;
use rocket_sync_db_pools::database;

#[database("pg_db")]
pub struct DbConn(PgConnection);

pub mod db;
pub mod models;
pub mod schema;
pub mod apis;
pub mod mst;
pub mod common;
pub mod storage;