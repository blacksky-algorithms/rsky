use crate::account_manager::AccountManager;
use crate::config::ServerConfig;
use anyhow::Result;
use rocket::http::Status;
use rocket::request::{FromRequest, Outcome};
use rocket::response::status;
use rocket::{Request, State};

pub struct HostHeader(pub String);

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
) -> Result<String, status::Custom<String>> {
    let handle = host.0;
    let supported_handle = cfg
        .identity
        .service_handle_domains
        .iter()
        .find(|host| handle.ends_with(host.as_str()) || handle == host[1..])
        .is_some();
    if !supported_handle {
        return Err(status::Custom(
            Status::NotFound,
            "User not found".to_string(),
        ));
    }
    match AccountManager::get_account(&handle, None).await {
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
