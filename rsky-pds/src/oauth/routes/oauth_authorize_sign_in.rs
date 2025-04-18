use crate::account_manager::AccountManager;
use crate::oauth::{OAuthResponse, SharedOAuthProvider, SharedReplayStore};
use rocket::data::{FromData, ToByteUnit};
use rocket::http::Status;
use rocket::{post, Data, Request, State};
use rsky_oauth::oauth_provider::account::account_store::SignInCredentials;
use rsky_oauth::oauth_provider::device::device_id::DeviceId;
use rsky_oauth::oauth_provider::errors::OAuthError;
use rsky_oauth::oauth_provider::lib::http::request::{
    validate_fetch_mode, validate_fetch_site, validate_referer, validate_same_origin,
};
use rsky_oauth::oauth_provider::lib::util::url::UrlReference;
use rsky_oauth::oauth_provider::oauth_provider::SignInResponse;
use rsky_oauth::oauth_provider::oidc::sub::Sub;
use rsky_oauth::oauth_provider::request::request_uri::RequestUri;
use rsky_oauth::oauth_types::OAuthClientId;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct SignIn {
    pub device_id: DeviceId,
    pub request_uri: RequestUri,
    pub client_id: OAuthClientId,
    pub account_sub: Sub,
    pub credentials: SignInCredentials,
}

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
            Err(e) => return rocket::data::Outcome::Error((Status::new(400), ())),
        }
        match validate_fetch_site(req, vec!["same-origin"]) {
            Ok(_) => {}
            Err(e) => return rocket::data::Outcome::Error((Status::new(400), ())),
        }
        match validate_same_origin(req, "same-origin") {
            Ok(_) => {}
            Err(e) => return rocket::data::Outcome::Error((Status::new(400), ())),
        }

        let input = data.open(10000.bytes());
        let x = input.into_string().await.unwrap().value;
        let sign_in_payload: SignInPayload;

        let url_reference = UrlReference {
            origin: Some(String::from("")),
            pathname: Some(String::from("/oauth/authorize")),
        };
        match validate_referer(req, url_reference) {
            Ok(_) => {}
            Err(e) => return rocket::data::Outcome::Error((Status::new(400), ())),
        }
        unimplemented!()
        // match validate_csrf_token(
        //     req,
        //     sign_in_payload.csrf_token.as_str(),
        //     csrf_cookie(&sign_in_payload.request_uri).as_str(),
        //     true,
        // ) {
        //     Ok(_) => {}
        //     Err(e) => return rocket::data::Outcome::Error((Status::new(400), ())),
        // }

        // rocket::data::Outcome::Success(Self {
        //     device_id: sign_in_payload.device_id,
        //     request_uri: sign_in_payload.request_uri,
        //     client_id: sign_in_payload.client_id,
        //     account_sub: sign_in_payload.account_sub,
        // })
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
    Ok(OAuthResponse {
        body: res,
        status: Status::Ok,
    })
}
