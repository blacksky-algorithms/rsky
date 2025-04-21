use rocket::http::Status;
use rocket::response::Responder;
use rocket::{response, Request, Response};
use rsky_oauth::jwk::Keyset;
use rsky_oauth::oauth_provider::oauth_provider::OAuthProviderCreator;
use rsky_oauth::oauth_provider::replay::replay_store::ReplayStore;
use std::io::Cursor;
use std::sync::Arc;
use tokio::sync::RwLock;

pub mod detailed_account_store;
pub mod models;
pub mod provider;
pub mod routes;

pub struct SharedOAuthProvider {
    pub oauth_provider: Arc<RwLock<OAuthProviderCreator>>,
    pub keyset: Arc<RwLock<Keyset>>,
}

impl SharedOAuthProvider {
    pub fn new(
        oauth_provider: Arc<RwLock<OAuthProviderCreator>>,
        keyset: Arc<RwLock<Keyset>>,
    ) -> Self {
        Self {
            oauth_provider,
            keyset,
        }
    }
}

pub struct SharedReplayStore {
    pub replay_store: Arc<RwLock<dyn ReplayStore>>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OAuthResponse<T: serde::Serialize> {
    pub body: T,
    pub status: Status,
    pub dpop_nonce: Option<String>,
}

impl<'r, T: serde::Serialize> Responder<'r, 'static> for OAuthResponse<T> {
    fn respond_to(self, request: &'r Request<'_>) -> response::Result<'static> {
        let mut response = Response::build();

        response.raw_header("Access-Control-Allow-Origin", "*");
        response.raw_header("Access-Control-Allow-Headers", "*");

        // https://www.rfc-editor.org/rfc/rfc6749.html#section-5.1
        response.raw_header("Cache-Control", "no-store");
        response.raw_header("Pragma", "no-cache");

        // https://datatracker.ietf.org/doc/html/rfc9449#section-8.2
        if let Some(dpop_nonce) = self.dpop_nonce {
            response.raw_header("DPoP-Nonce", dpop_nonce);
            response.raw_header_adjoin("Access-Control-Expose-Headers", "DPoP-Nonce");
        }

        match request.headers().get_one("Accept") {
            None => {
                let mut response = Response::build();
                response.status(Status { code: 406u16 });
                return response.ok();
            }
            Some(accept_header) => {
                if accept_header != "application/json" && accept_header != "*/*" {
                    let mut response = Response::build();
                    response.status(Status { code: 406u16 });
                    return response.ok();
                }
            }
        }

        let y = match serde_json::to_string(&self.body) {
            Ok(y) => y,
            Err(e) => {
                let mut response = Response::build();
                response.status(Status { code: 500u16 });
                return response.ok();
            }
        };
        response.sized_body(y.len(), Cursor::new(y));
        response.status(self.status);
        Ok(response.finalize())
    }
}
