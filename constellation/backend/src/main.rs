use axum::response::IntoResponse;
use axum::{Router, response::Html, routing::get};
use backend::firehose;
use backend::models::{AppState, Post};
use backend::routes::{callback_handler, feed_handler, login_handler, sse_handler};
use std::convert::Infallible;
use std::env;
use std::sync::Arc;
use surrealdb::{Surreal, engine::local::RocksDb};
use tokio::sync::broadcast;
use tower::service_fn;
use tower_http::services::ServeDir;
use tower_http::cors::{Any, CorsLayer};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db_path = env::var("DB_PATH").unwrap_or_else(|_| String::from("data/surrealdb"));
    let db = Surreal::new::<RocksDb>(&db_path).await?;
    db.use_ns("cs").use_db("cs").await?;

    // --- Create a broadcast channel for posts ---
    let (tx, _rx) = broadcast::channel::<Post>(100);

    let sessions = Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
    let oauth_client = backend::auth::init_oauth();

    // --- Build shared application state ---
    let app_state = AppState {
        db: db.clone(),
        sessions: sessions.clone(),
        tx: tx.clone(),
        oauth_client: Arc::new(oauth_client),
    };

    // --- Spawn the firehose background task ---
    // This task listens for upstream events and processes/saves posts.
    tokio::spawn({
        let db_clone = db.clone();
        let tx_clone = tx.clone();
        async move {
            if let Err(e) = firehose::run_firehose(db_clone, tx_clone).await {
                tracing::error!("Firehose error: {:?}", e);
            }
        }
    });

    // --- Set up static file serving ---
    let not_found_service = service_fn(|_req| async {

        Ok::<_, Infallible>(Html("404 Not Found").into_response())
    });
    let serve_dir = ServeDir::new("./dist").not_found_service(not_found_service);

    // --- Build the Axum router ---
    let app = Router::new()
        .route("/", get(feed_handler))
        .route("/stream", get(sse_handler))
        .route("/login", get(login_handler))
        .route("/callback", get(callback_handler))
        // Serve static assets from ./dist
        .fallback_service(serve_dir)
        .layer(
            CorsLayer::new()
                .allow_origin(Any) // or be more specific in production
                .allow_methods(Any)
                .allow_headers(Any)
        )
        .with_state(app_state);

    // --- Start the server ---
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
    axum::serve(listener, app).await.unwrap();
    Ok(())
}
