use crate::oauth::SharedOAuthProvider;
use http::header;
use rocket::data::{FromData, ToByteUnit};
use rocket::http::Status;
use rocket::request::FromRequest;
use rocket::{get, post, Data, Request, State};
use rsky_oauth::oauth_provider::errors::OAuthError;
use rsky_oauth::oauth_types::{OAuthTokenIdentification, TokenTypeHint};
use std::num::NonZeroU64;

pub struct OAuthRevokeGetRequestBody {
    pub oauth_token_identification: OAuthTokenIdentification,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for OAuthRevokeGetRequestBody {
    type Error = OAuthError;

    #[tracing::instrument(skip_all)]
    async fn from_request(req: &'r Request<'_>) -> rocket::request::Outcome<Self, Self::Error> {
        let token = req.query_value::<String>("token").unwrap().unwrap();
        // let token_type_hint = req.query_value::<Option<TokenTypeHint>>("token_type_hint").unwrap().unwrap();
        let hint = req
            .query_value::<String>("token_type_hint")
            .unwrap()
            .unwrap();
        let token_type_hint: Option<TokenTypeHint> = Some(hint.parse().unwrap());
        let body = OAuthRevokeGetRequestBody {
            oauth_token_identification: OAuthTokenIdentification {
                token,
                token_type_hint,
            },
        };
        rocket::request::Outcome::Success(body)
    }
}

#[get("/oauth/revoke")]
pub async fn get_oauth_revoke(
    shared_oauth_provider: &State<SharedOAuthProvider>,
    body: OAuthRevokeGetRequestBody,
) -> Result<(), OAuthError> {
    unimplemented!()
    // let mut oauth_provider = shared_oauth_provider.oauth_provider.write().await;
    // match oauth_provider
    //     .revoke(&body.oauth_token_identification)
    //     .await
    // {
    //     Ok(res) => Ok(()),
    //     Err(e) => Err(e),
    // }
}

pub struct OAuthRevokeRequestBody {
    pub oauth_token_identification: OAuthTokenIdentification,
}

#[rocket::async_trait]
impl<'r> FromData<'r> for OAuthRevokeRequestBody {
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
                                let oauth_token_identification: OAuthTokenIdentification =
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
                                rocket::data::Outcome::Success(OAuthRevokeRequestBody {
                                    oauth_token_identification,
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
                                rocket::data::Outcome::Success(OAuthRevokeRequestBody {
                                    oauth_token_identification,
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

#[post("/oauth/revoke", data = "<body>")]
pub async fn post_oauth_revoke(
    shared_oauth_provider: &State<SharedOAuthProvider>,
    body: OAuthRevokeRequestBody,
) -> Result<(), OAuthError> {
    unimplemented!()
    // let mut creator = shared_oauth_provider.oauth_provider.write().await;
    // let x = creator(d);
    // match oauth_provider
    //     .revoke(&body.oauth_token_identification)
    //     .await
    // {
    //     Ok(res) => Ok(()),
    //     Err(e) => Err(e),
    // }
}
