use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::oauth::routes::csrf_cookie;
use crate::oauth::{OAuthOptions, OAuthResponse, SharedOAuthProvider, SharedReplayStore};
use rocket::data::{FromData, ToByteUnit};
use rocket::http::Status;
use rocket::{post, Data, Request, State};
use rsky_oauth::oauth_provider::account::account_store::SignInCredentials;
use rsky_oauth::oauth_provider::device::device_id::DeviceId;
use rsky_oauth::oauth_provider::device::device_manager::DeviceManager;
use rsky_oauth::oauth_provider::errors::OAuthError;
use rsky_oauth::oauth_provider::lib::http::request::{
    validate_csrf_token, validate_fetch_mode, validate_fetch_site, validate_referer,
    validate_same_origin,
};
use rsky_oauth::oauth_provider::lib::util::url::UrlReference;
use rsky_oauth::oauth_provider::oauth_provider::SignInResponse;
use rsky_oauth::oauth_provider::oidc::sub::Sub;
use rsky_oauth::oauth_provider::request::request_uri::RequestUri;
use rsky_oauth::oauth_types::OAuthClientId;
use std::env;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct SignIn {
    pub device_id: DeviceId,
    pub request_uri: RequestUri,
    pub client_id: OAuthClientId,
    pub credentials: SignInCredentials,
}

#[derive(Serialize, Deserialize)]
pub struct SignInPayload {
    csrf_token: String,
    request_uri: RequestUri,
    client_id: OAuthClientId,
    credentials: SignInCredentials,
}

#[rocket::async_trait]
impl<'r> FromData<'r> for SignIn {
    type Error = ();

    #[tracing::instrument(skip_all)]
    async fn from_data(
        req: &'r Request<'_>,
        data: Data<'r>,
    ) -> rocket::data::Outcome<'r, Self, Self::Error> {
        match validate_fetch_mode(req, vec!["same-origin"]) {
            Ok(_) => {}
            Err(e) => {
                let error = ApiError::InvalidRequest("Invalid fetch mode header".to_string());
                req.local_cache(|| Some(error.clone()));
                return rocket::data::Outcome::Error((Status::new(400), ()));
            }
        }
        match validate_fetch_site(req, vec!["same-origin"]) {
            Ok(_) => {}
            Err(e) => {
                let error = ApiError::InvalidRequest("Invalid fetch site header".to_string());
                req.local_cache(|| Some(error.clone()));
                return rocket::data::Outcome::Error((Status::new(400), ()));
            }
        }
        match validate_same_origin(req, env::var("OAuthIssuerIdentifier").unwrap().as_str()) {
            Ok(_) => {}
            Err(e) => {
                let error = ApiError::InvalidRequest("Invalid same-origin header".to_string());
                req.local_cache(|| Some(error.clone()));
                return rocket::data::Outcome::Error((Status::new(400), ()));
            }
        }

        let input = data.open(100000.bytes());
        let datastream = input.into_string().await.unwrap().value;
        println!("{}", datastream);
        let sign_in_payload: SignInPayload = serde_json::from_str(datastream.as_str()).unwrap();

        let url_reference = UrlReference {
            origin: Some(env::var("OAuthIssuerIdentifier").unwrap()),
            pathname: Some(String::from("/oauth/authorize")),
        };
        match validate_referer(req, url_reference) {
            Ok(_) => {}
            Err(e) => {
                let error = ApiError::InvalidRequest("Invalid referer".to_string());
                req.local_cache(|| Some(error.clone()));
                return rocket::data::Outcome::Error((Status::new(400), ()));
            }
        }
        match validate_csrf_token(
            req,
            sign_in_payload.csrf_token.as_str(),
            csrf_cookie(&sign_in_payload.request_uri).as_str(),
            false,
        ) {
            Ok(_) => {}
            Err(e) => {
                let error = ApiError::InvalidRequest("Invalid csrf token".to_string());
                req.local_cache(|| Some(error.clone()));
                return rocket::data::Outcome::Error((Status::new(400), ()));
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
                return rocket::data::Outcome::Error((Status::new(500), ()));
            }
            rocket::request::Outcome::Forward(_) => {
                let error = ApiError::RuntimeError;
                req.local_cache(|| Some(error.clone()));
                return rocket::data::Outcome::Error((Status::new(500), ()));
            }
        };

        let mut device_manager = DeviceManager::new(Arc::new(RwLock::new(account_manager)), None);
        let device_id = match device_manager.load(req, true).await {
            Ok(device_id) => device_id,
            Err(errror) => {
                let error = ApiError::RuntimeError;
                req.local_cache(|| Some(error.clone()));
                return rocket::data::Outcome::Error((Status::new(500), ()));
            }
        };

        rocket::data::Outcome::Success(Self {
            device_id,
            request_uri: sign_in_payload.request_uri,
            client_id: sign_in_payload.client_id,
            credentials: sign_in_payload.credentials,
        })
    }
}

#[post("/oauth/authorize/sign-in", data = "<sign_in>")]
pub async fn oauth_authorize_sign_in(
    shared_oauth_provider: &State<SharedOAuthProvider>,
    shared_replay_store: &State<SharedReplayStore>,
    account_manager: AccountManager,
    sign_in: SignIn,
) -> Result<OAuthResponse<SignInResponse>, OAuthError> {
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
    let res = oauth_provider
        .sign_in(
            sign_in.device_id,
            sign_in.request_uri,
            sign_in.client_id,
            sign_in.credentials,
        )
        .await?;
    let dpop_nonce = oauth_provider.oauth_verifier.next_dpop_nonce().await;
    Ok(OAuthResponse {
        body: res,
        status: Status::Ok,
        dpop_nonce,
    })
}

#[tracing::instrument(skip_all)]
#[rocket::options("/oauth/authorize/sign-in")]
pub async fn oauth_signin_options() -> OAuthOptions {
    OAuthOptions {}
}
