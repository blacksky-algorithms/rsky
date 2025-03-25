use crate::oauth_provider::errors::OAuthError;
use crate::oauth_provider::routes::SharedOAuthProvider;
use crate::oauth_types::OAuthAuthorizationServerMetadata;
use rocket::serde::json::Json;
use rocket::{get, State};

#[get("/.well-known/oauth-authorization-server")]
pub async fn oauth_well_known(
    shared_oauth_provider: &State<SharedOAuthProvider>,
) -> Result<Json<OAuthAuthorizationServerMetadata>, OAuthError> {
    let oauth_provider = shared_oauth_provider.oauth_provider.read().await;
    Ok(Json(oauth_provider.metadata.clone()))
}
