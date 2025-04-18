use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::oauth::{OAuthResponse, SharedOAuthProvider, SharedReplayStore};
use http::header;
use rocket::data::{FromData, ToByteUnit};
use rocket::http::Status;
use rocket::{post, Data, Request, State};
use rsky_oauth::oauth_provider::errors::OAuthError;
use rsky_oauth::oauth_types::{
    OAuthAuthorizationRequestPar, OAuthClientCredentials, OAuthParResponse,
};
use std::num::NonZeroU64;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct OAuthParRequestBody {
    pub oauth_client_credentials: OAuthClientCredentials,
    pub oauth_authorization_request_par: OAuthAuthorizationRequestPar,
    pub dpop: String,
    pub url: String,
    pub method: String,
}

#[rocket::async_trait]
impl<'r> FromData<'r> for OAuthParRequestBody {
    type Error = ApiError;

    #[tracing::instrument(skip_all)]
    async fn from_data(
        req: &'r Request<'_>,
        data: Data<'r>,
    ) -> rocket::data::Outcome<'r, Self, Self::Error> {
        let headers = req.headers();
        let dpop = match headers.get_one("dpop") {
            None => {
                let error = ApiError::InvalidRequest("Missing dpop header".to_string());
                req.local_cache(|| Some(error.clone()));
                return rocket::data::Outcome::Error((Status::BadRequest, error));
            }
            Some(dpop) => dpop,
        };
        match headers.get_one(header::CONTENT_TYPE.as_ref()) {
            None => {
                let error = ApiError::InvalidRequest("Missing content-type header".to_string());
                req.local_cache(|| Some(error.clone()));
                rocket::data::Outcome::Error((Status::BadRequest, error))
            }
            Some(content_type) => {
                if content_type == "application/x-www-form-urlencoded" {
                    match req.headers().get_one(header::CONTENT_LENGTH.as_ref()) {
                        None => {
                            let error = ApiError::InvalidRequest(
                                "application/x-www-form-urlencoded".to_string(),
                            );
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
                                        Err(e) => {
                                            let error = ApiError::InvalidRequest(
                                                "serde_urlencoded::from_str".to_string(),
                                            );
                                            req.local_cache(|| Some(error.clone()));
                                            return rocket::data::Outcome::Error((
                                                Status::BadRequest,
                                                error,
                                            ));
                                        }
                                    };
                                let oauth_authorization_request_par: OAuthAuthorizationRequestPar =
                                    match serde_urlencoded::from_str(datastream.as_str()) {
                                        Ok(res) => res,
                                        Err(e) => {
                                            let error = ApiError::InvalidRequest(
                                                "serde_urlencoded::from_str".to_string(),
                                            );
                                            req.local_cache(|| Some(error.clone()));
                                            return rocket::data::Outcome::Error((
                                                Status::BadRequest,
                                                error,
                                            ));
                                        }
                                    };
                                rocket::data::Outcome::Success(OAuthParRequestBody {
                                    oauth_client_credentials,
                                    oauth_authorization_request_par,
                                    dpop: dpop.to_string(),
                                    url: req.uri().to_string(),
                                    method: "POST".to_string(),
                                })
                            }
                            Err(_error) => {
                                let error = ApiError::InvalidRequest(
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
                            let error = ApiError::InvalidRequest("application/json".to_string());
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
                                        Err(e) => {
                                            tracing::error!(
                                                "{}",
                                                format!("Error parsing client credentials\n{e}")
                                            );
                                            let error = ApiError::InvalidRequest(
                                                "Error parsing client credentials".to_string(),
                                            );
                                            req.local_cache(|| Some(error.clone()));
                                            return rocket::data::Outcome::Error((
                                                Status::BadRequest,
                                                error,
                                            ));
                                        }
                                    };
                                let oauth_authorization_request_par: OAuthAuthorizationRequestPar =
                                    match serde_json::from_str(datastream.as_str()) {
                                        Ok(res) => res,
                                        Err(e) => {
                                            tracing::error!(
                                                "{}",
                                                format!("Error parsing authorization request\n{e}")
                                            );
                                            let error = ApiError::InvalidRequest(
                                                "Error parsing authorization request".to_string(),
                                            );
                                            req.local_cache(|| Some(error.clone()));
                                            return rocket::data::Outcome::Error((
                                                Status::BadRequest,
                                                error,
                                            ));
                                        }
                                    };
                                rocket::data::Outcome::Success(OAuthParRequestBody {
                                    oauth_client_credentials,
                                    oauth_authorization_request_par,
                                    dpop: dpop.to_string(),
                                    url: req.uri().to_string(),
                                    method: "POST".to_string(),
                                })
                            }
                            Err(_error) => {
                                tracing::error!(
                                    "{}",
                                    format!("Error parsing content-length\n{_error}")
                                );
                                let error = ApiError::InvalidRequest(
                                    "Error parsing content-length".to_string(),
                                );
                                req.local_cache(|| Some(error.clone()));
                                rocket::data::Outcome::Error((Status::BadRequest, error))
                            }
                        },
                    }
                } else {
                    let error = ApiError::InvalidRequest("Invalid content-type".to_string());
                    req.local_cache(|| Some(error.clone()));
                    rocket::data::Outcome::Error((Status::BadRequest, error))
                }
            }
        }
    }
}

#[post("/oauth/par", data = "<body>")]
pub async fn oauth_par(
    shared_oauth_provider: &State<SharedOAuthProvider>,
    shared_replay_store: &State<SharedReplayStore>,
    account_manager: AccountManager,
    body: OAuthParRequestBody,
) -> Result<OAuthResponse<OAuthParResponse>, OAuthError> {
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
    let dpop_jkt = oauth_provider
        .oauth_verifier
        .check_dpop_proof(
            body.dpop.as_str(),
            body.method.as_str(),
            ("https://pds.ripperoni.com".to_string() + body.url.as_str()).as_str(),
            None,
        )
        .await?;
    let res = oauth_provider
        .pushed_authorization_request(
            body.oauth_client_credentials.clone(),
            body.oauth_authorization_request_par.clone(),
            Some(dpop_jkt),
        )
        .await?;
    Ok(OAuthResponse {
        body: res,
        status: Status::Created,
    })
}
