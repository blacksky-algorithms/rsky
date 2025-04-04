use crate::account_manager::AccountManager;
use crate::oauth::routes::DpopJkt;
use crate::oauth::{SharedOAuthProvider, SharedReplayStore};
use http::header;
use rocket::data::{FromData, ToByteUnit};
use rocket::http::Status;
use rocket::serde::json::Json;
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
    pub url: String,
}

#[rocket::async_trait]
impl<'r> FromData<'r> for OAuthParRequestBody {
    type Error = OAuthError;

    #[tracing::instrument(skip_all)]
    async fn from_data(
        req: &'r Request<'_>,
        data: Data<'r>,
    ) -> rocket::data::Outcome<'r, Self, Self::Error> {
        match req.headers().get_one(header::CONTENT_TYPE.as_ref()) {
            None => {
                let error = OAuthError::RuntimeError("test".to_string());
                req.local_cache(|| Some(error.clone()));
                rocket::data::Outcome::Error((Status::BadRequest, error))
            }
            Some(content_type) => {
                if content_type == "application/x-www-form-urlencoded" {
                    match req.headers().get_one(header::CONTENT_LENGTH.as_ref()) {
                        None => {
                            let error = OAuthError::RuntimeError(
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
                                            let error = OAuthError::RuntimeError(
                                                "serde_urlencoded::from_str(d".to_string(),
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
                                            print!("{}", e.to_string());
                                            let error = OAuthError::RuntimeError(
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
                                    url: "".to_string(),
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
                            let error = OAuthError::RuntimeError("application/json".to_string());
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
                                            let error = OAuthError::RuntimeError(
                                                " serde_json::from_str".to_string(),
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
                                            let error = OAuthError::RuntimeError(
                                                " serde_json::from_str".to_string(),
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
                                    url: "".to_string(),
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
                    let error = OAuthError::RuntimeError("else".to_string());
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
    dpop_jkt: DpopJkt,
) -> Result<Json<OAuthParResponse>, OAuthError> {
    let creator = shared_oauth_provider.oauth_provider.read().await;
    let x = Arc::new(RwLock::new(account_manager));
    let mut oauth_provider = creator(
        x.clone(),
        x.clone(),
        x.clone(),
        x.clone(),
        x.clone(),
        shared_replay_store.replay_store.clone(),
    );
    let dpop_jkt = oauth_provider
        .oauth_verifier
        .check_dpop_proof(
            dpop_jkt.0.unwrap().as_str(),
            "POST",
            body.url.as_str(),
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
    Ok(Json(res))
}
