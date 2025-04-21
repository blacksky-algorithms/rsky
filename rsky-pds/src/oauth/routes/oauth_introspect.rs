use crate::account_manager::AccountManager;
use crate::oauth::OAuthResponse;
use crate::oauth::{SharedOAuthProvider, SharedReplayStore};
use http::header;
use rocket::data::{FromData, ToByteUnit};
use rocket::http::Status;
use rocket::{post, Data, Request, State};
use rsky_oauth::oauth_provider::errors::OAuthError;
use rsky_oauth::oauth_types::{
    OAuthClientCredentials, OAuthIntrospectionResponse, OAuthTokenIdentification,
};
use std::num::NonZeroU64;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct OAuthIntrospectRequestBody {
    pub client_credentials: OAuthClientCredentials,
    pub token_identification: OAuthTokenIdentification,
}

#[rocket::async_trait]
impl<'r> FromData<'r> for OAuthIntrospectRequestBody {
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
                    let datastream = data.open(1000.bytes()).into_string().await.unwrap().value;
                    let client_credentials: OAuthClientCredentials =
                        match serde_urlencoded::from_str(datastream.as_str()) {
                            Ok(res) => res,
                            Err(e) => {
                                let error = OAuthError::RuntimeError("test".to_string());
                                req.local_cache(|| Some(error.clone()));
                                return rocket::data::Outcome::Error((Status::BadRequest, error));
                            }
                        };
                    let token_identification: OAuthTokenIdentification =
                        match serde_urlencoded::from_str(datastream.as_str()) {
                            Ok(res) => res,
                            Err(e) => {
                                let error = OAuthError::RuntimeError("test".to_string());
                                req.local_cache(|| Some(error.clone()));
                                return rocket::data::Outcome::Error((Status::BadRequest, error));
                            }
                        };
                    rocket::data::Outcome::Success(OAuthIntrospectRequestBody {
                        token_identification,
                        client_credentials,
                    })
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
                                let client_credentials: OAuthClientCredentials =
                                    match serde_urlencoded::from_str(datastream.as_str()) {
                                        Ok(res) => res,
                                        Err(e) => {
                                            let error =
                                                OAuthError::RuntimeError("test".to_string());
                                            req.local_cache(|| Some(error.clone()));
                                            return rocket::data::Outcome::Error((
                                                Status::BadRequest,
                                                error,
                                            ));
                                        }
                                    };
                                let token_identification: OAuthTokenIdentification =
                                    match serde_json::from_str(datastream.as_str()) {
                                        Ok(res) => res,
                                        Err(e) => {
                                            let error =
                                                OAuthError::RuntimeError("test".to_string());
                                            req.local_cache(|| Some(error.clone()));
                                            return rocket::data::Outcome::Error((
                                                Status::BadRequest,
                                                error,
                                            ));
                                        }
                                    };
                                rocket::data::Outcome::Success(OAuthIntrospectRequestBody {
                                    token_identification,
                                    client_credentials,
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

#[post("/oauth/introspect", data = "<body>")]
pub async fn oauth_introspect(
    shared_oauth_provider: &State<SharedOAuthProvider>,
    shared_replay_store: &State<SharedReplayStore>,
    account_manager: AccountManager,
    body: OAuthIntrospectRequestBody,
) -> Result<OAuthResponse<OAuthIntrospectionResponse>, OAuthError> {
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
    let body = oauth_provider
        .introspect(body.client_credentials, body.token_identification)
        .await?;
    let dpop_nonce = oauth_provider.oauth_verifier.next_dpop_nonce().await;
    Ok(OAuthResponse {
        body,
        status: Status::Ok,
        dpop_nonce,
    })
}
