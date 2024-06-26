#![feature(async_closure)]
#[macro_use]
extern crate serde_derive;
extern crate core;
extern crate mailchecker;
extern crate rocket;
extern crate serde;

use crate::sequencer::Sequencer;
use diesel::pg::PgConnection;
use rocket_sync_db_pools::database;
use rsky_identity::IdResolver;
use tokio::sync::RwLock;

static APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_HOMEPAGE"), "@", env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);

#[database("pg_db")]
pub struct DbConn(PgConnection);

pub struct SharedSequencer {
    pub sequencer: RwLock<Sequencer>,
}

pub struct SharedIdResolver {
    pub id_resolver: RwLock<IdResolver>,
}

pub mod account_manager;
pub mod apis;
pub mod auth_verifier;
pub mod car;
pub mod common;
pub mod crawlers;
pub mod db;
pub mod image;
pub mod lexicons;
pub mod mailer;
pub mod models;
pub mod plc;
pub mod repo;
pub mod schema;
pub mod sequencer;
pub mod storage;
mod vendored;
pub mod xrpc_server;
