#[macro_use]
extern crate serde_derive;
extern crate core;
extern crate rocket;
extern crate serde;

use diesel::pg::PgConnection;
use rocket_sync_db_pools::database;

#[database("pg_db")]
pub struct DbConn(PgConnection);

pub mod account_manager;
pub mod apis;
pub mod auth_verifier;
pub mod common;
pub mod db;
pub mod models;
pub mod repo;
pub mod schema;
pub mod storage;
