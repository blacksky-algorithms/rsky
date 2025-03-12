use std::collections::HashMap;
// models.rs
use crate::auth::HickoryDnsTxtResolver;
use crate::vendored::atrium_oauth_client::store::state::MemoryStateStore;
use crate::vendored::atrium_oauth_client::{DefaultHttpClient, OAuthClient};
use atrium_identity::did::CommonDidResolver;
use atrium_identity::handle::AtprotoHandleResolver;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use tokio::sync::broadcast;

#[derive(Clone, Debug)]
pub struct SessionInfo {
    pub did: String,
    pub token: String,
}

#[derive(Clone)]
pub struct AppState {
    pub db: Surreal<Db>,
    pub sessions: Arc<Mutex<HashMap<String, SessionInfo>>>, // <-- use SessionInfo here
    pub tx: broadcast::Sender<Post>,
    pub oauth_client: Arc<
        OAuthClient<
            MemoryStateStore,
            CommonDidResolver<DefaultHttpClient>,
            AtprotoHandleResolver<HickoryDnsTxtResolver, DefaultHttpClient>,
        >,
    >,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Post {
    // Primary key (we'll also use this as SurrealDB record ID)
    pub uri: String,
    pub cid: String,
    pub reply_parent: Option<String>,
    pub reply_root: Option<String>,
    pub indexed_at: DateTime<Utc>,
    pub prev: Option<String>,
    pub sequence: i64,
    pub text: String,
    pub langs: Option<Vec<String>>,
    pub author: String,
    pub external_uri: Option<String>,
    pub external_title: Option<String>,
    pub external_description: Option<String>,
    pub external_thumb: Option<String>,
    pub quote_uri: Option<String>,
    pub quote_cid: Option<String>,
    pub created_at: DateTime<Utc>,
    pub labels: Option<Vec<String>>,
    pub local_only: bool,
}
