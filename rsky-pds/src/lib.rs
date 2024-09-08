#[macro_use]
extern crate serde_derive;
extern crate core;
extern crate mailchecker;
extern crate rocket;
extern crate serde;

use crate::read_after_write::viewer::LocalViewerCreator;
use crate::sequencer::Sequencer;
use atrium_api::client::AtpServiceClient;
use atrium_xrpc_client::reqwest::ReqwestClient;
use diesel::pg::PgConnection;
use event_emitter_rs::EventEmitter;
use lazy_static::lazy_static;
use rocket_sync_db_pools::database;
use rsky_identity::IdResolver;
use tokio::sync::RwLock;

pub static APP_USER_AGENT: &str = concat!(
    env!("CARGO_PKG_HOMEPAGE"),
    "@",
    env!("CARGO_PKG_NAME"),
    "/",
    env!("CARGO_PKG_VERSION"),
);

const INVALID_HANDLE: &'static str = "handle.invalid";

#[database("pg_db")]
pub struct DbConn(PgConnection);

pub struct SharedSequencer {
    pub sequencer: RwLock<Sequencer>,
}

pub struct SharedIdResolver {
    pub id_resolver: RwLock<IdResolver>,
}

pub struct SharedLocalViewer {
    pub local_viewer: RwLock<LocalViewerCreator>,
}

pub struct SharedATPAgent {
    pub app_view_agent: Option<RwLock<AtpServiceClient<ReqwestClient>>>,
}

// Use lazy_static! because the size of EventEmitter is not known at compile time
lazy_static! {
    // Export the emitter with `pub` keyword
    pub static ref EVENT_EMITTER: RwLock<EventEmitter> = RwLock::new(EventEmitter::new());
}

pub mod account_manager;
pub mod apis;
pub mod auth_verifier;
pub mod car;
pub mod common;
pub mod config;
pub mod context;
pub mod crawlers;
pub mod db;
pub mod image;
pub mod lexicon;
pub mod mailer;
pub mod models;
pub mod pipethrough;
pub mod plc;
pub mod read_after_write;
pub mod repo;
pub mod schema;
pub mod sequencer;
pub mod storage;
mod vendored;
pub mod well_known;
pub mod xrpc_server;
