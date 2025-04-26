use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::oauth::routes::{csrf_cookie, OAuthAuthorizeResponse};
use crate::oauth::{OAuthResponse, SharedOAuthProvider, SharedReplayStore};
use rocket::http::Status;
use rocket::request::FromRequest;
use rocket::{get, Request, State};
use rsky_oauth::oauth_provider::device::device_id::DeviceId;
use rsky_oauth::oauth_provider::device::device_manager::DeviceManager;
use rsky_oauth::oauth_provider::errors::OAuthError;
use rsky_oauth::oauth_provider::lib::http::request::{
    validate_csrf_token, validate_fetch_site, validate_referer,
};
use rsky_oauth::oauth_provider::lib::util::url::UrlReference;
use rsky_oauth::oauth_provider::oidc::sub::Sub;
use rsky_oauth::oauth_provider::output::send_authorize_redirect::{
    AuthorizationResult, AuthorizationResultRedirect,
};
use rsky_oauth::oauth_provider::request::request_uri::RequestUri;
use rsky_oauth::oauth_types::OAuthClientId;
use std::env;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct AuthorizeReject {
    pub device_id: DeviceId,
    pub request_uri: RequestUri,
    pub client_id: OAuthClientId,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for AuthorizeReject {
    type Error = ();

    async fn from_request(req: &'r Request<'_>) -> rocket::request::Outcome<Self, Self::Error> {
        match validate_fetch_site(req, vec!["same-origin"]) {
            Ok(_) => {}
            Err(e) => {
                let error = ApiError::InvalidRequest("Invalid fetch-site".to_string());
                req.local_cache(|| Some(error.clone()));
                return rocket::request::Outcome::Error((Status::new(400), ()));
            }
        }

        let request_uri = match req.query_value::<&str>("request_uri") {
            None => {
                let error = ApiError::InvalidRequest("Missing request_uri".to_string());
                req.local_cache(|| Some(error.clone()));
                return rocket::request::Outcome::Error((Status::new(400), ()));
            }
            Some(val) => match val {
                Ok(val) => match RequestUri::new(val) {
                    Ok(request_uri) => request_uri,
                    Err(e) => {
                        let error = ApiError::InvalidRequest("Invalid request_uri".to_string());
                        req.local_cache(|| Some(error.clone()));
                        return rocket::request::Outcome::Error((Status::new(400), ()));
                    }
                },
                Err(e) => {
                    let error = ApiError::InvalidRequest("Invalid request_uri".to_string());
                    req.local_cache(|| Some(error.clone()));
                    return rocket::request::Outcome::Error((Status::new(400), ()));
                }
            },
        };
        let client_id = match req.query_value::<&str>("client_id") {
            None => {
                let error = ApiError::InvalidRequest("Missing client_id".to_string());
                req.local_cache(|| Some(error.clone()));
                return rocket::request::Outcome::Error((Status::new(400), ()));
            }
            Some(val) => match val {
                Ok(val) => match OAuthClientId::new(val) {
                    Ok(client_id) => client_id,
                    Err(e) => {
                        let error = ApiError::InvalidRequest("Invalid client_id".to_string());
                        req.local_cache(|| Some(error.clone()));
                        return rocket::request::Outcome::Error((Status::new(400), ()));
                    }
                },
                Err(e) => {
                    let error = ApiError::InvalidRequest("Invalid client_id".to_string());
                    req.local_cache(|| Some(error.clone()));
                    return rocket::request::Outcome::Error((Status::new(400), ()));
                }
            },
        };
        let csrf_token = match req.query_value::<&str>("csrf_token") {
            None => {
                let error = ApiError::InvalidRequest("Missing csrf_token".to_string());
                req.local_cache(|| Some(error.clone()));
                return rocket::request::Outcome::Error((Status::new(400), ()));
            }
            Some(val) => match val {
                Ok(val) => val,
                Err(e) => {
                    let error = ApiError::InvalidRequest("Invalid csrf_token".to_string());
                    req.local_cache(|| Some(error.clone()));
                    return rocket::request::Outcome::Error((Status::new(400), ()));
                }
            },
        };

        let url_reference = UrlReference {
            origin: Some(env::var("OAUTH_ISSUER_IDENTIFIER").unwrap()),
            pathname: Some(String::from("/oauth/authorize")),
        };
        match validate_referer(req, url_reference) {
            Ok(_) => {}
            Err(e) => {
                let error = ApiError::InvalidRequest("Invalid referer".to_string());
                req.local_cache(|| Some(error.clone()));
                return rocket::request::Outcome::Error((Status::new(400), ()));
            }
        }

        match validate_csrf_token(req, csrf_token, csrf_cookie(&request_uri).as_str(), true) {
            Ok(_) => {}
            Err(e) => {
                let error = ApiError::InvalidRequest("Invalid csrf_token".to_string());
                req.local_cache(|| Some(error.clone()));
                return rocket::request::Outcome::Error((Status::new(400), ()));
            }
        }

        let account_manager = match req
            .guard::<AccountManager>()
            .await
            .map(|account_manager| account_manager)
        {
            rocket::request::Outcome::Success(account_manager) => account_manager,
            rocket::request::Outcome::Error(_) => {
                let error = ApiError::RuntimeError;
                req.local_cache(|| Some(error.clone()));
                return rocket::request::Outcome::Error((Status::new(500), ()));
            }
            rocket::request::Outcome::Forward(_) => {
                let error = ApiError::RuntimeError;
                req.local_cache(|| Some(error.clone()));
                return rocket::request::Outcome::Error((Status::new(500), ()));
            }
        };

        let mut device_manager = DeviceManager::new(Arc::new(RwLock::new(account_manager)), None);
        let device_id = match device_manager.load(req, true).await {
            Ok(device_id) => device_id,
            Err(error) => {
                let error = ApiError::RuntimeError;
                req.local_cache(|| Some(error.clone()));
                return rocket::request::Outcome::Error((Status::new(500), ()));
            }
        };

        rocket::request::Outcome::Success(Self {
            device_id,
            request_uri,
            client_id,
        })
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
) -> Result<OAuthAuthorizeResponse, OAuthError> {
    let creator = shared_oauth_provider.oauth_provider.read().await;
    let account_manager_lock = Arc::new(RwLock::new(account_manager));
    let mut oauth_provider = creator(
        account_manager_lock.clone(),
        Some(account_manager_lock.clone()),
        account_manager_lock.clone(),
        account_manager_lock.clone(),
        Some(account_manager_lock.clone()),
        Some(shared_replay_store.replay_store.clone()),
    );
    let data = oauth_provider
        .reject_request(
            authorize_reject.device_id,
            authorize_reject.request_uri,
            authorize_reject.client_id,
        )
        .await?;
    Ok(OAuthAuthorizeResponse::Redirect(data))
}
