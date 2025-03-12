use crate::models::{AppState, SessionInfo};
use crate::vendored::atrium_oauth_client::{AuthorizeOptions, CallbackParams, KnownScope, Scope};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive};
use axum::response::{IntoResponse, Redirect, Response, Sse};
use axum_extra::TypedHeader;
use futures::Stream;
use futures::StreamExt;
use headers::Cookie;
use serde::Deserialize;
use std::convert::Infallible;
use tokio_stream::wrappers::BroadcastStream;

pub async fn login_handler(State(app_state): State<AppState>) -> Redirect {
    let oauth = &app_state.oauth_client;

    // Determine the user's server (for multi-tenant, we might use a query param or config; here use env or default)
    let server_url =
        std::env::var("ATP_AUTH_BASE").unwrap_or_else(|_| "https://bsky.social".to_string());
    // Generate the authorization URL
    let auth_url = oauth
        .authorize(server_url, AuthorizeOptions {
            // (Atrium uses preconfigured scopes; we can pass none to use default set in config)
            scopes: vec![
                Scope::Known(KnownScope::Atproto),
                Scope::Known(KnownScope::TransitionGeneric),
            ],
            ..Default::default()
        })
        .await
        .expect("Failed to get authorization URL");
    Redirect::temporary(auth_url.as_str())
}

#[derive(Deserialize)]
pub struct AuthQuery {
    code: String,
    state: Option<String>,
    iss: String,
}

pub async fn callback_handler(
    State(app_state): State<AppState>,
    Query(query): Query<AuthQuery>,
) -> Result<Response, (StatusCode, String)> {
    let oauth = &app_state.oauth_client;
    let session_store = &app_state.sessions;

    // Construct CallbackParams (Atrium expects all params including state and iss)
    let params = CallbackParams {
        code: query.code.clone(),
        state: query.state.clone(),
        iss: Some(query.iss.clone()),
    };
    // Exchange the code for tokens
    let token_set = oauth.callback(params).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("OAuth callback error: {}", e),
        )
    })?;
    // token_set contains access_token, refresh_token, etc.
    // For this app, we mainly care about the user's DID (which should be token_set.sub)
    let user_did = token_set.sub.clone();
    // (Optionally, fetch the user's handle/profile using the access_token if needed)
    // Store session info in our in-memory session store
    let session_id = uuid::Uuid::new_v4().to_string();
    session_store
        .lock()
        .unwrap()
        .insert(session_id.clone(), SessionInfo {
            did: user_did,
            token: token_set.access_token.clone(),
        });
    // Set a session cookie (we’ll use a simple cookie with the session ID)
    let cookie = format!("session={}; Path=/; HttpOnly", session_id);
    // Redirect to the home page after login
    let mut redirect = Redirect::temporary("/").into_response();
    redirect
        .headers_mut()
        .append(axum::http::header::SET_COOKIE, cookie.parse().unwrap());
    Ok(redirect)
}

pub async fn feed_handler(
    State(app_state): State<AppState>,
    TypedHeader(cookies): TypedHeader<Cookie>,
) -> impl IntoResponse {
    if let Some(session_id) = cookies.get("session") {
        let sessions = app_state.sessions.lock().unwrap();
        if sessions.contains_key(session_id) {
            let index_html = std::fs::read_to_string("./dist/index.html").unwrap();
            return axum::response::Html(index_html).into_response();
        }
    }
    Redirect::temporary("/login").into_response()
}

pub async fn sse_handler(
    State(app_state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = app_state.tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| async move {
        match result {
            Ok(post) => {
                let json = serde_json::to_string(&post).unwrap();
                Some(Ok(Event::default().data(json)))
            }
            Err(_) => None,
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}
