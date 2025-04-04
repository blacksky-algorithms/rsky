use crate::oauth_provider::device::device_manager::DeviceManager;
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_provider::lib::http::request::{
    validate_csrf_token, validate_fetch_site, validate_referer,
};
use crate::oauth_provider::request::request_uri::RequestUri;
use crate::oauth_types::{
    OAuthAuthorizationRequestPar, OAuthClientCredentials, OAuthClientId, OAuthTokenIdentification,
    OAuthTokenRequest, TokenTypeHint,
};
use http::header;
use rocket::data::{FromData, ToByteUnit};
use rocket::http::Status;
use rocket::request::FromRequest;
use rocket::{Data, Request, State};
use std::num::NonZeroU64;
use std::time::SystemTime;

pub mod access_token;
pub mod account;
pub mod client;
pub mod constants;
pub mod device;
pub mod dpop;
pub mod errors;
pub mod lib;
pub mod metadata;
pub mod oauth_hooks;
pub mod oauth_provider;
pub mod oauth_verifier;
pub mod oidc;
pub mod output;
pub mod replay;
pub mod request;
pub mod signer;
pub mod token;

pub fn now_as_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("timestamp in micros since UNIX epoch")
        .as_secs()
}

pub struct OAuthAuthorizeRequestBody(Option<String>);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for OAuthAuthorizeRequestBody {
    type Error = ();

    async fn from_request(req: &'r Request<'_>) -> rocket::request::Outcome<Self, Self::Error> {
        match req.headers().get_one("dpop") {
            None => rocket::request::Outcome::Success(OAuthAuthorizeRequestBody(None)),
            Some(res) => {
                rocket::request::Outcome::Success(OAuthAuthorizeRequestBody(Some(res.to_string())))
            }
        }
    }
}

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

pub struct OAuthTokenRequestBody {
    pub oauth_client_credentials: OAuthClientCredentials,
    pub oauth_token_request: OAuthTokenRequest,
}

#[rocket::async_trait]
impl<'r> FromData<'r> for OAuthTokenRequestBody {
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
                                let oauth_token_request: OAuthTokenRequest =
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
                                rocket::data::Outcome::Success(OAuthTokenRequestBody {
                                    oauth_client_credentials,
                                    oauth_token_request,
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
                                let oauth_token_request: OAuthTokenRequest =
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
                                rocket::data::Outcome::Success(OAuthTokenRequestBody {
                                    oauth_client_credentials,
                                    oauth_token_request,
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

pub struct AuthorizeReject {
    pub request_uri: RequestUri,
    pub client_id: OAuthClientId,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for AuthorizeReject {
    type Error = ();

    async fn from_request(req: &'r Request<'_>) -> rocket::request::Outcome<Self, Self::Error> {
        match validate_fetch_site(req, vec!["same-origin"]) {
            Ok(_) => {}
            Err(e) => return rocket::request::Outcome::Error((Status::new(400), ())),
        }

        let query = req.query_fields();
        let csrf_token = "".to_string();
        let request_uri = RequestUri::new("").unwrap();
        let client_id = OAuthClientId::new("").unwrap();

        validate_referer(req);

        validate_csrf_token(req, csrf_token.as_str(), "", true);

        let device_manager = req.guard::<&State<DeviceManager>>().await.unwrap();

        rocket::request::Outcome::Success(Self {
            request_uri,
            client_id,
        })
    }
}
