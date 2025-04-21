use crate::account_manager::AccountManager;
use crate::oauth::{SharedOAuthProvider, SharedReplayStore};
use rocket::serde::json::Json;
use rocket::{get, State};
use rsky_oauth::oauth_provider::errors::OAuthError;
use rsky_oauth::oauth_types::{
    BearerMethod, OAuthAuthorizationServerMetadata, OAuthIssuerIdentifier,
    OAuthProtectedResourceMetadata, ValidUri, WebUri,
};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;

#[tracing::instrument(skip_all)]
#[get("/.well-known/oauth-authorization-server")]
pub async fn oauth_well_known(
    shared_oauth_provider: &State<SharedOAuthProvider>,
    shared_replay_store: &State<SharedReplayStore>,
    account_manager: AccountManager,
) -> Result<Json<OAuthAuthorizationServerMetadata>, OAuthError> {
    let creator = shared_oauth_provider.oauth_provider.read().await;
    let account_manager = Arc::new(RwLock::new(account_manager));
    let oauth_provider = creator(
        account_manager.clone(),
        Some(account_manager.clone()),
        account_manager.clone(),
        account_manager.clone(),
        Some(account_manager.clone()),
        Some(shared_replay_store.replay_store.clone()),
    );
    Ok(Json(oauth_provider.metadata.clone()))
}

#[tracing::instrument(skip_all)]
#[get("/.well-known/oauth-protected-resource")]
pub async fn oauth_well_known_resources() -> Json<OAuthProtectedResourceMetadata> {
    let result = OAuthProtectedResourceMetadata {
        resource: WebUri::validate("https://pds.ripperoni.com").unwrap(),
        authorization_servers: Some(vec![OAuthIssuerIdentifier::from_str(
            "https://pds.ripperoni.com",
        )
        .unwrap()]),
        jwks_uri: None,
        scopes_supported: Some(vec![]),
        bearer_methods_supported: Some(vec![BearerMethod::Header]),
        resource_signing_alg_values_supported: None,
        resource_documentation: Some(WebUri::validate("https://atproto.com").unwrap()),
        resource_policy_uri: None,
        resource_tos_uri: None,
    };
    Json(result)
}
