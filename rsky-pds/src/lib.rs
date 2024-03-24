#[macro_use]
extern crate serde_derive;
extern crate core;
extern crate rocket;
extern crate serde;

use tokio::sync::RwLock;
use diesel::pg::PgConnection;
use rocket_sync_db_pools::database;
use crate::sequencer::Sequencer;

#[database("pg_db")]
pub struct DbConn(PgConnection);

pub struct SharedSequencer {
    pub sequencer: RwLock<Sequencer>
}

pub mod account_manager;
pub mod apis;
pub mod auth_verifier;
pub mod car;
pub mod common;
pub mod crawlers;
pub mod db;
pub mod models;
pub mod repo;
pub mod schema;
pub mod sequencer;
pub mod storage;
mod vendored;
