use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::oauth::routes::{csrf_cookie, OAuthAuthorizeResponse};
use crate::oauth::{SharedOAuthProvider, SharedReplayStore};
use rocket::http::{Header, Status};
use rocket::request::FromRequest;
use rocket::response::{content, Responder};
use rocket::{get, response, Request, Response, State};
use rsky_oauth::oauth_provider::device::device_id::DeviceId;
use rsky_oauth::oauth_provider::device::device_manager::DeviceManager;
use rsky_oauth::oauth_provider::errors::OAuthError;
use rsky_oauth::oauth_provider::lib::http::request::{
    setup_csrf_token, validate_csrf_token, validate_fetch_site, validate_referer,
};
use rsky_oauth::oauth_provider::lib::util::url::UrlReference;
use rsky_oauth::oauth_provider::oidc::sub::Sub;
use rsky_oauth::oauth_provider::output::build_authorize_data::AuthorizationResultAuthorize;
use rsky_oauth::oauth_provider::output::send_authorize_redirect::{
    AuthorizationResult, AuthorizationResultRedirect,
};
use rsky_oauth::oauth_provider::request::request_uri::RequestUri;
use rsky_oauth::oauth_types::{
    OAuthAuthorizationRequestQuery, OAuthAuthorizationRequestUri, OAuthClientCredentials,
    OAuthClientCredentialsNone, OAuthClientId, OAuthRedirectUri, OAuthRequestUri, ResponseMode,
};
use serde_json::json;
use std::io::Cursor;
use std::sync::Arc;
use tokio::sync::RwLock;
use url::Url;

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
            Err(e) => {
                let error = ApiError::InvalidRequest("Invalid fetch site header".to_string());
                req.local_cache(|| Some(error.clone()));
                return rocket::request::Outcome::Error((Status::new(400), ()));
            }
        }

        let request_uri = match req.query_value::<&str>("request_uri") {
            None => return rocket::request::Outcome::Error((Status::new(400), ())),
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
            None => return rocket::request::Outcome::Error((Status::new(400), ())),
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
            Err(errror) => {
                let error = ApiError::RuntimeError;
                req.local_cache(|| Some(error.clone()));
                return rocket::request::Outcome::Error((Status::new(500), ()));
            }
        };
        rocket::request::Outcome::Success(Self {
            device_id,
            credentials: OAuthClientCredentials::None(OAuthClientCredentialsNone::new(client_id)),
            authorization_request: OAuthAuthorizationRequestQuery::from_uri(
                OAuthAuthorizationRequestUri::new(
                    OAuthRequestUri::new(request_uri.into_inner()).unwrap(),
                ),
            ),
        })
    }
}

#[get("/oauth/authorize")]
pub async fn oauth_authorize(
    shared_oauth_provider: &State<SharedOAuthProvider>,
    shared_replay_store: &State<SharedReplayStore>,
    body: OAuthAuthorizeRequestBody,
    account_manager: AccountManager,
) -> Result<OAuthAuthorizeResponse, OAuthError> {
    let creator = shared_oauth_provider.oauth_provider.read().await;
    let account_manager = Arc::new(RwLock::new(account_manager));
    let mut oauth_provider = creator(
        account_manager.clone(),
        Some(account_manager.clone()),
        account_manager.clone(),
        account_manager.clone(),
        Some(account_manager.clone()),
        Some(shared_replay_store.replay_store.clone()),
    );
    let result = oauth_provider
        .authorize(
            &body.device_id,
            &body.credentials,
            &body.authorization_request,
        )
        .await?;
    match result {
        AuthorizationResult::Redirect(redirect) => Ok(OAuthAuthorizeResponse::Redirect(redirect)),
        AuthorizationResult::Authorize(authorize) => Ok(OAuthAuthorizeResponse::Page(authorize)),
    }
}
