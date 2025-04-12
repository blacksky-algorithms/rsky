use crate::account_manager::AccountManager;
use crate::oauth::{OAuthResponse, SharedOAuthProvider, SharedReplayStore};
use rocket::http::Status;
use rocket::request::FromRequest;
use rocket::{get, Request, State};
use rsky_oauth::oauth_provider::device::device_id::DeviceId;
use rsky_oauth::oauth_provider::errors::OAuthError;
use rsky_oauth::oauth_provider::lib::http::request::{validate_fetch_site, validate_referer};
use rsky_oauth::oauth_provider::lib::util::url::UrlReference;
use rsky_oauth::oauth_provider::oidc::sub::Sub;
use rsky_oauth::oauth_provider::output::send_authorize_redirect::AuthorizationResultRedirect;
use rsky_oauth::oauth_provider::request::request_uri::RequestUri;
use rsky_oauth::oauth_types::OAuthClientId;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct AuthorizeReject {
    pub device_id: DeviceId,
    pub request_uri: RequestUri,
    pub client_id: OAuthClientId,
    pub account_sub: Sub,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for AuthorizeReject {
    type Error = ();

    async fn from_request(req: &'r Request<'_>) -> rocket::request::Outcome<Self, Self::Error> {
        match validate_fetch_site(req, vec!["same-origin"]) {
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
        let csrf_token = match req.query_value::<&str>("csrf_token") {
            None => return rocket::request::Outcome::Error((Status::new(400), ())),
            Some(val) => match val {
                Ok(val) => val,
                Err(e) => return rocket::request::Outcome::Error((Status::new(400), ())),
            },
        };

        let url_reference = UrlReference {
            origin: Some(String::from("")),
            pathname: Some(String::from("/oauth/authorize")),
        };
        match validate_referer(req, url_reference) {
            Ok(_) => {}
            Err(e) => return rocket::request::Outcome::Error((Status::new(400), ())),
        }

        unimplemented!()
        // match validate_csrf_token(req, csrf_token, j, true) {
        //     Ok(_) => {}
        //     Err(e) => return rocket::request::Outcome::Error((Status::new(400), ())),
        // }
        //
        // rocket::request::Outcome::Success(Self {
        //     device_id,
        //     request_uri,
        //     client_id,
        //     account_sub,
        // })
    }
}

// Though this is a "no-cors" request, meaning that the browser will allow
// any cross-origin request, with credentials, to be sent, the handler will
// 1) validate the request origin,
// 2) validate the CSRF token,
// 3) validate the referer,
// 4) validate the sec-fetch-site header,
// 4) validate the sec-fetch-mode header,
// 5) validate the sec-fetch-dest header (see navigationHandler).
// And will error if any of these checks fail.
#[get("/oauth/authorize/reject")]
pub async fn oauth_authorize_reject(
    shared_oauth_provider: &State<SharedOAuthProvider>,
    shared_replay_store: &State<SharedReplayStore>,
    account_manager: AccountManager,
    authorize_reject: AuthorizeReject,
) -> Result<OAuthResponse<AuthorizationResultRedirect>, OAuthError> {
    let creator = shared_oauth_provider.oauth_provider.read().await;
    let x = Arc::new(RwLock::new(account_manager));
    let mut oauth_provider = creator(
        x.clone(),
        Some(x.clone()),
        x.clone(),
        x.clone(),
        Some(x.clone()),
        Some(shared_replay_store.replay_store.clone()),
    );
    let data = match oauth_provider
        .accept_request(
            authorize_reject.device_id,
            authorize_reject.request_uri,
            authorize_reject.client_id,
            authorize_reject.account_sub,
        )
        .await
    {
        Ok(data) => data,
        Err(_e) => {
            unimplemented!()
        }
    };

    Ok(OAuthResponse {
        body: data,
        status: Status::Ok,
    })
}
