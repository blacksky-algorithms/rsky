use crate::account_manager::AccountManager;
use crate::oauth::OAuthResponse;
use crate::oauth::{SharedOAuthProvider, SharedReplayStore};
use http::header;
use rocket::data::{FromData, ToByteUnit};
use rocket::http::Status;
use rocket::{post, Data, Request, State};
use rsky_oauth::oauth_provider::errors::OAuthError;
use rsky_oauth::oauth_types::{OAuthClientCredentials, OAuthTokenRequest, OAuthTokenResponse};
use std::num::NonZeroU64;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct OAuthTokenRequestBody {
    pub oauth_client_credentials: OAuthClientCredentials,
    pub oauth_token_request: OAuthTokenRequest,
    pub method: String,
    pub dpop: String,
    pub url: String,
}

#[rocket::async_trait]
impl<'r> FromData<'r> for OAuthTokenRequestBody {
    type Error = OAuthError;

    #[tracing::instrument(skip_all)]
    async fn from_data(
        req: &'r Request<'_>,
        data: Data<'r>,
    ) -> rocket::data::Outcome<'r, Self, Self::Error> {
        let headers = req.headers();
        let dpop = match headers.get_one("dpop") {
            None => {
                let error = OAuthError::RuntimeError("test".to_string());
                req.local_cache(|| Some(error.clone()));
                return rocket::data::Outcome::Error((Status::BadRequest, error));
            }
            Some(dpop) => dpop,
        };
        match headers.get_one(header::CONTENT_TYPE.as_ref()) {
            None => {
                let error = OAuthError::RuntimeError("test".to_string());
                req.local_cache(|| Some(error.clone()));
                return rocket::data::Outcome::Error((Status::BadRequest, error));
            }
            Some(content_type) => {
                if content_type == "application/x-www-form-urlencoded" {
                    match req.headers().get_one(header::CONTENT_LENGTH.as_ref()) {
                        None => {
                            let error = OAuthError::RuntimeError("test".to_string());
                            req.local_cache(|| Some(error.clone()));
                            rocket::data::Outcome::Error((Status::BadRequest, error))
                        }
                        Some(res) => match res.parse::<NonZeroU64>() {
                            Ok(content_length) => {
                                let datastream = data
                                    .open(content_length.get().bytes())
                                    .into_string()
                                    .await
                                    .unwrap()
                                    .value;
                                let oauth_client_credentials: OAuthClientCredentials =
                                    match serde_urlencoded::from_str(datastream.as_str()) {
                                        Ok(res) => res,
                                        Err(_) => {
                                            let error =
                                                OAuthError::RuntimeError("test".to_string());
                                            req.local_cache(|| Some(error.clone()));
                                            return rocket::data::Outcome::Error((
                                                Status::BadRequest,
                                                error,
                                            ));
                                        }
                                    };
                                let oauth_token_request: OAuthTokenRequest =
                                    match serde_urlencoded::from_str(datastream.as_str()) {
                                        Ok(res) => res,
                                        Err(_) => {
                                            let error =
                                                OAuthError::RuntimeError("test".to_string());
                                            req.local_cache(|| Some(error.clone()));
                                            return rocket::data::Outcome::Error((
                                                Status::BadRequest,
                                                error,
                                            ));
                                        }
                                    };
                                rocket::data::Outcome::Success(OAuthTokenRequestBody {
                                    oauth_client_credentials,
                                    oauth_token_request,
                                    method: "POST".to_string(),
                                    dpop: dpop.to_string(),
                                    url: req.uri().to_string(),
                                })
                            }
                            Err(_error) => {
                                tracing::error!(
                                    "{}",
                                    format!("Error parsing content-length\n{_error}")
                                );
                                let error = OAuthError::RuntimeError(
                                    "Error parsing content-length".to_string(),
                                );
                                req.local_cache(|| Some(error.clone()));
                                rocket::data::Outcome::Error((Status::BadRequest, error))
                            }
                        },
                    }
                } else if content_type == "application/json" {
                    match req.headers().get_one(header::CONTENT_LENGTH.as_ref()) {
                        None => {
                            let error = OAuthError::RuntimeError("test".to_string());
                            req.local_cache(|| Some(error.clone()));
                            rocket::data::Outcome::Error((Status::BadRequest, error))
                        }
                        Some(res) => match res.parse::<NonZeroU64>() {
                            Ok(content_length) => {
                                let datastream = data
                                    .open(content_length.get().bytes())
                                    .into_string()
                                    .await
                                    .unwrap()
                                    .value;
                                let oauth_client_credentials: OAuthClientCredentials =
                                    match serde_json::from_str(datastream.as_str()) {
                                        Ok(res) => res,
                                        Err(_) => {
                                            let error =
                                                OAuthError::RuntimeError("test".to_string());
                                            req.local_cache(|| Some(error.clone()));
                                            return rocket::data::Outcome::Error((
                                                Status::BadRequest,
                                                error,
                                            ));
                                        }
                                    };
                                let oauth_token_request: OAuthTokenRequest =
                                    match serde_json::from_str(datastream.as_str()) {
                                        Ok(res) => res,
                                        Err(_) => {
                                            let error =
                                                OAuthError::RuntimeError("test".to_string());
                                            req.local_cache(|| Some(error.clone()));
                                            return rocket::data::Outcome::Error((
                                                Status::BadRequest,
                                                error,
                                            ));
                                        }
                                    };
                                rocket::data::Outcome::Success(OAuthTokenRequestBody {
                                    oauth_client_credentials,
                                    oauth_token_request,
                                    method: "POST".to_string(),
                                    dpop: dpop.to_string(),
                                    url: req.uri().to_string(),
                                })
                            }
                            Err(_error) => {
                                tracing::error!(
                                    "{}",
                                    format!("Error parsing content-length\n{_error}")
                                );
                                let error = OAuthError::RuntimeError(
                                    "Error parsing content-length".to_string(),
                                );
                                req.local_cache(|| Some(error.clone()));
                                rocket::data::Outcome::Error((Status::BadRequest, error))
                            }
                        },
                    }
                } else {
                    let error = OAuthError::RuntimeError("test".to_string());
                    req.local_cache(|| Some(error.clone()));
                    rocket::data::Outcome::Error((Status::BadRequest, error))
                }
            }
        }
    }
}

#[post("/oauth/token", data = "<body>")]
pub async fn oauth_token(
    shared_oauth_provider: &State<SharedOAuthProvider>,
    shared_replay_store: &State<SharedReplayStore>,
    body: OAuthTokenRequestBody,
    account_manager: AccountManager,
) -> Result<OAuthResponse<OAuthTokenResponse>, OAuthError> {
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
    let dpop_jkt = oauth_provider
        .oauth_verifier
        .check_dpop_proof(
            body.dpop.as_str(),
            body.method.as_str(),
            body.url.as_str(),
            None,
        )
        .await?;
    let data = oauth_provider
        .token(
            body.oauth_client_credentials,
            body.oauth_token_request,
            Some(dpop_jkt),
        )
        .await?;
    Ok(OAuthResponse {
        body: data,
        status: Status::Ok,
    })
}
