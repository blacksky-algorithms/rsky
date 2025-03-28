use crate::oauth_provider::errors::OAuthError;
use crate::oauth_provider::routes::SharedOAuthProvider;
use crate::oauth_types::OAuthIntrospectionResponse;
use crate::oauth_types::{OAuthClientCredentials, OAuthTokenIdentification};
use http::header;
use rocket::data::{FromData, ToByteUnit};
use rocket::http::Status;
use rocket::serde::json::Json;
use rocket::{post, Data, Request, State};
use std::num::NonZeroU64;

pub struct OAuthIntrospectRequestBody {
    pub oauth_client_credentials: OAuthClientCredentials,
    pub oauth_token_identification: OAuthTokenIdentification,
}

#[rocket::async_trait]
impl<'r> FromData<'r> for OAuthIntrospectRequestBody {
    type Error = OAuthError;

    #[tracing::instrument(skip_all)]
    async fn from_data(
        req: &'r Request<'_>,
        data: Data<'r>,
    ) -> rocket::data::Outcome<'r, Self, Self::Error> {
        //TODO Separate JSON from URL Encoded Later
        match req.headers().get_one(header::CONTENT_TYPE.as_ref()) {
            None => {
                let error = OAuthError::RuntimeError("test".to_string());
                req.local_cache(|| Some(error.clone()));
                rocket::data::Outcome::Error((Status::BadRequest, error))
            }
            Some(content_type) => {
                if content_type == "application/x-www-form-urlencoded" {
                    let datastream = data.open(1000.bytes()).into_string().await.unwrap().value;
                    let oauth_client_credentials: OAuthClientCredentials =
                        match serde_urlencoded::from_str(datastream.as_str()) {
                            Ok(res) => res,
                            Err(e) => {
                                let error = OAuthError::RuntimeError("test".to_string());
                                req.local_cache(|| Some(error.clone()));
                                return rocket::data::Outcome::Error((Status::BadRequest, error));
                            }
                        };
                    let oauth_token_identification: OAuthTokenIdentification =
                        match serde_urlencoded::from_str(datastream.as_str()) {
                            Ok(res) => res,
                            Err(e) => {
                                let error = OAuthError::RuntimeError("test".to_string());
                                req.local_cache(|| Some(error.clone()));
                                return rocket::data::Outcome::Error((Status::BadRequest, error));
                            }
                        };
                    rocket::data::Outcome::Success(OAuthIntrospectRequestBody {
                        oauth_token_identification,
                        oauth_client_credentials,
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
                                let oauth_client_credentials: OAuthClientCredentials =
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
                                let oauth_token_identification: OAuthTokenIdentification =
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
                                    oauth_token_identification,
                                    oauth_client_credentials,
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
    body: OAuthIntrospectRequestBody,
) -> Result<Json<OAuthIntrospectionResponse>, OAuthError> {
    unimplemented!();
    // let mut oauth_provider = shared_oauth_provider.oauth_provider.write().await;
    // let res = oauth_provider
    //     .introspect(
    //         body.oauth_client_credentials,
    //         body.oauth_token_identification,
    //     )
    //     .await?;
    // Ok(Json(res))
}
