use crate::oauth::SharedOAuthProvider;
use jsonwebtoken::jwk::Jwk;
use rocket::serde::json::Json;
use rocket::{get, State};

#[get("/oauth/jwks")]
pub async fn oauth_jwks(shared_oauth_provider: &State<SharedOAuthProvider>) -> Json<Vec<Jwk>> {
    unimplemented!()
    // let oauth_provider = shared_oauth_provider.oauth_provider.read().await;
    // Json(oauth_provider.get_jwks())
}
