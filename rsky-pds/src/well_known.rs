use crate::account_manager::AccountManager;
use crate::config::ServerConfig;
use anyhow::Result;
use rocket::http::Status;
use rocket::request::{FromRequest, Outcome};
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::{Request, State};
use serde::Serialize;

pub struct HostHeader(pub String);

#[derive(Serialize)]
pub struct OAuthProtectedResourceMetadata {
    pub resource: String,
    pub authorization_servers: Vec<String>,
}
#[rocket::async_trait]
impl<'r> FromRequest<'r> for HostHeader {
    type Error = ();

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match req.headers().get_one("Host") {
            Some(h) => Outcome::Success(HostHeader(h.to_string())),
            None => Outcome::Forward(Status::InternalServerError),
        }
    }
}

#[rocket::get("/.well-known/atproto-did")]
pub async fn well_known(
    host: HostHeader,
    cfg: &State<ServerConfig>,
    account_manager: AccountManager,
) -> Result<String, status::Custom<String>> {
    let handle = host.0;
    let supported_handle = cfg
        .identity
        .service_handle_domains
        .iter()
        .any(|host| handle.ends_with(host.as_str()) || handle == host[1..]);
    if !supported_handle {
        return Err(status::Custom(
            Status::NotFound,
            "User not found".to_string(),
        ));
    }
    match account_manager.get_account(&handle, None).await {
        Ok(user) => {
            let did: Option<String> = match user {
                Some(user) => Some(user.did),
                None => None,
            };
            match did {
                None => Err(status::Custom(
                    Status::NotFound,
                    "User not found".to_string(),
                )),
                Some(did) => Ok(did),
            }
        }
        Err(_) => Err(status::Custom(
            Status::InternalServerError,
            "Internal Server Error".to_string(),
        )),
    }
}

#[rocket::get("/.well-known/oauth-protected-resource")]
pub async fn oauth_protected_resource(
    cfg: &State<ServerConfig>,
) -> Result<Json<OAuthProtectedResourceMetadata>, status::Custom<String>> {
    match cfg.identity.oauth_authorization_server.clone() {
        Some(authorization_server) => Ok(Json(OAuthProtectedResourceMetadata {
            resource: cfg.service.public_url.clone(),
            authorization_servers: vec![authorization_server],
        })),
        None => Err(status::Custom(
            Status::NotFound,
            "OAuth protected resource metadata not configured".to_string(),
        )),
    }
}
