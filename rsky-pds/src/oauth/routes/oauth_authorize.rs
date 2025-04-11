use crate::account_manager::AccountManager;
use crate::oauth::{SharedOAuthProvider, SharedReplayStore};
use rocket::http::Status;
use rocket::request::FromRequest;
use rocket::{get, Request, State};
use rsky_oauth::oauth_provider::device::device_id::DeviceId;
use rsky_oauth::oauth_provider::device::device_manager::DeviceManager;
use rsky_oauth::oauth_provider::lib::http::request::{
    validate_csrf_token, validate_fetch_site, validate_referer,
};
use rsky_oauth::oauth_provider::lib::util::url::UrlReference;
use rsky_oauth::oauth_provider::oidc::sub::Sub;
use rsky_oauth::oauth_provider::output::send_authorize_redirect::AuthorizationResult;
use rsky_oauth::oauth_provider::request::request_uri::RequestUri;
use rsky_oauth::oauth_types::{
    OAuthAuthorizationRequestQuery, OAuthClientCredentials, OAuthClientId,
};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Serialize, Deserialize)]
pub enum OAuthAuthorizeResponse {}

pub struct OAuthAuthorizeRequestBody {
    pub device_id: DeviceId,
    pub credentials: OAuthClientCredentials,
    pub authorization_request: OAuthAuthorizationRequestQuery,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for OAuthAuthorizeRequestBody {
    type Error = ();

    async fn from_request(req: &'r Request<'_>) -> rocket::request::Outcome<Self, Self::Error> {
        match validate_fetch_site(req, vec!["cross-site", "none"]) {
            Ok(_) => {}
            Err(e) => return rocket::request::Outcome::Error((Status::new(400), ())),
        }

        let request_uri = match req.query_value::<&str>("request_uri") {
            None => return rocket::request::Outcome::Error((Status::new(400), ())),
            Some(val) => match val {
                Ok(val) => match RequestUri::new(val) {
                    Ok(request_uri) => request_uri,
                    Err(e) => return rocket::request::Outcome::Error((Status::new(400), ())),
                },
                Err(e) => return rocket::request::Outcome::Error((Status::new(400), ())),
            },
        };
        let client_id = match req.query_value::<&str>("client_id") {
            None => return rocket::request::Outcome::Error((Status::new(400), ())),
            Some(val) => match val {
                Ok(val) => match OAuthClientId::new(val) {
                    Ok(client_id) => client_id,
                    Err(e) => return rocket::request::Outcome::Error((Status::new(400), ())),
                },
                Err(e) => return rocket::request::Outcome::Error((Status::new(400), ())),
            },
        };
        let sub = match req.query_value::<&str>("account_sub") {
            None => return rocket::request::Outcome::Error((Status::new(400), ())),
            Some(val) => match val {
                Ok(val) => match Sub::new(val) {
                    Ok(sub) => sub,
                    Err(e) => return rocket::request::Outcome::Error((Status::new(400), ())),
                },
                Err(e) => return rocket::request::Outcome::Error((Status::new(400), ())),
            },
        };
        unimplemented!()
    }
}

#[get("/oauth/authorize")]
pub async fn oauth_authorize(
    shared_oauth_provider: &State<SharedOAuthProvider>,
    shared_replay_store: &State<SharedReplayStore>,
    body: OAuthAuthorizeRequestBody,
    account_manager: AccountManager,
) {
    unimplemented!()
    // let creator = shared_oauth_provider.oauth_provider.read().await;
    // let x = Arc::new(RwLock::new(account_manager));
    // let mut oauth_provider = creator(
    //     x.clone(),
    //     Some(x.clone()),
    //     x.clone(),
    //     x.clone(),
    //     Some(x.clone()),
    //     Some(shared_replay_store.replay_store.clone()),
    // );
    // let result = match oauth_provider
    //     .authorize(device_id, &body.credentials, &body.authorization_request)
    //     .await
    // {
    //     Ok(data) => data,
    //     Err(e) => {
    //         unimplemented!()
    //     }
    // };
    // match result {
    //     AuthorizationResult::Redirect(redirect) => {}
    //     AuthorizationResult::Authorize(authorize) => {}
    // }
}
