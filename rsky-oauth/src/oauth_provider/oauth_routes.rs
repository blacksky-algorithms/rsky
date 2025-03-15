use crate::oauth_provider::account::account_store::SignInCredentials;
use crate::oauth_provider::client::client_id::ClientId;
use crate::oauth_provider::device::device_id::DeviceId;
use crate::oauth_provider::device::device_manager::DeviceManager;
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_provider::lib::http::request::{
    validate_csrf_token, validate_fetch_site, validate_referer,
};
use crate::oauth_provider::oauth_provider::OAuthProvider;
use crate::oauth_provider::oidc::sub::Sub;
use crate::oauth_provider::output::send_authorize_redirect::AuthorizationResult;
use crate::oauth_provider::request::request_uri::RequestUri;
use crate::oauth_types::{
    OAuthAuthorizationRequestPar, OAuthAuthorizationRequestQuery, OAuthAuthorizationServerMetadata,
    OAuthClientCredentials, OAuthClientId, OAuthIntrospectionResponse, OAuthParResponse,
    OAuthRequestUri, OAuthTokenIdentification, OAuthTokenRequest, OAuthTokenResponse,
    TokenTypeHint,
};
use http::header;
use jsonwebtoken::jwk::Jwk;
use rocket::data::{FromData, ToByteUnit};
use rocket::futures::TryFutureExt;
use rocket::http::Status;
use rocket::request::FromRequest;
use rocket::serde::json::Json;
use rocket::{get, post, routes, Data, Request, Response, Route, State};
use std::env;
use std::num::NonZeroU64;
use tokio::sync::RwLock;

pub struct SignInPayload {
    csrf_token: String,
    request_uri: OAuthRequestUri,
    client_id: OAuthClientId,
    credentials: SignInCredentials,
}

pub struct AcceptQuery {
    pub csrf_token: String,
    pub request_uri: RequestUri,
    pub client_id: OAuthClientId,
    pub account_sub: String,
}

pub struct OAuthTokenRequestBody {
    pub oauth_client_credentials: OAuthClientCredentials,
    pub oauth_token_request: OAuthTokenRequest,
}

pub struct OAuthAcceptRequestBody {
    pub oauth_client_credentials: OAuthClientCredentials,
    pub oauth_token_identification: OAuthTokenIdentification,
}

pub struct OAuthRejectRequestBody {
    pub oauth_client_credentials: OAuthClientCredentials,
    pub oauth_token_identification: OAuthTokenIdentification,
}

pub struct OAuthSigninRequestBody {
    pub device_id: DeviceId,
    pub credentials: OAuthClientCredentials,
    pub authorization_request: OAuthAuthorizationRequestQuery,
}

pub struct DpopJkt(Option<String>);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for DpopJkt {
    type Error = ();

    async fn from_request(req: &'r Request<'_>) -> rocket::request::Outcome<Self, Self::Error> {
        match req.headers().get_one("dpop") {
            None => rocket::request::Outcome::Success(DpopJkt(None)),
            Some(res) => rocket::request::Outcome::Success(DpopJkt(Some(res.to_string()))),
        }
    }
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

pub struct SharedOAuthProvider {
    pub oauth_provider: RwLock<OAuthProvider>,
}

pub struct SharedDeviceManager {
    pub device_manager: RwLock<DeviceManager>,
}

pub fn get_routes() -> Vec<Route> {
    routes![
        oauth_well_known,
        oauth_jwks,
        oauth_par,
        oauth_token,
        post_oauth_revoke,
        oauth_introspect,
        oauth_authorize,
        // oauth_authorize_signin,
        oauth_authorize_accept,
        oauth_authorize_reject
    ]
}

#[get("/.well-known/oauth-authorization-server")]
pub async fn oauth_well_known(
    shared_oauth_provider: &State<SharedOAuthProvider>,
) -> Result<Json<OAuthAuthorizationServerMetadata>, OAuthError> {
    let oauth_provider = shared_oauth_provider.oauth_provider.read().await;
    Ok(Json(oauth_provider.metadata.clone()))
}

#[get("/oauth/jwks")]
pub async fn oauth_jwks(shared_oauth_provider: &State<SharedOAuthProvider>) -> Json<Vec<Jwk>> {
    let oauth_provider = shared_oauth_provider.oauth_provider.read().await;
    Json(oauth_provider.get_jwks())
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

#[post("/oauth/par", data = "<body>")]
pub async fn oauth_par(
    shared_oauth_provider: &State<SharedOAuthProvider>,
    body: OAuthParRequestBody,
    dpop_jkt: DpopJkt,
) -> Result<Json<OAuthParResponse>, OAuthError> {
    let mut oauth_provider = shared_oauth_provider.oauth_provider.write().await;
    let dpop_jkt = oauth_provider
        .oauth_verifier
        .check_dpop_proof(
            dpop_jkt.0.unwrap().as_str(),
            "POST",
            body.url.as_str(),
            None,
        )
        .await;
    let res = oauth_provider
        .pushed_authorization_request(
            body.oauth_client_credentials.clone(),
            body.oauth_authorization_request_par.clone(),
            dpop_jkt,
        )
        .await?;
    Ok(Json(res))
}

#[post("/oauth/token", data = "<body>")]
pub async fn oauth_token(
    shared_oauth_provider: &State<SharedOAuthProvider>,
    body: OAuthTokenRequestBody,
    dpop_jkt: DpopJkt,
) -> Result<Json<OAuthTokenResponse>, OAuthError> {
    let mut oauth_provider = shared_oauth_provider.oauth_provider.write().await;
    let dpop_jkt = match dpop_jkt.0 {
        None => None,
        Some(res) => Some(res),
    };
    Ok(Json(
        oauth_provider
            .token(
                body.oauth_client_credentials,
                body.oauth_token_request,
                dpop_jkt,
            )
            .await?,
    ))
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
    let oauth_provider = shared_oauth_provider.oauth_provider.write().await;
    match oauth_provider
        .revoke(&body.oauth_token_identification)
        .await
    {
        Ok(res) => Ok(()),
        Err(e) => Err(e),
    }
}

#[post("/oauth/revoke", data = "<body>")]
pub async fn post_oauth_revoke(
    shared_oauth_provider: &State<SharedOAuthProvider>,
    body: OAuthRevokeRequestBody,
) -> Result<(), OAuthError> {
    let oauth_provider = shared_oauth_provider.oauth_provider.write().await;
    match oauth_provider
        .revoke(&body.oauth_token_identification)
        .await
    {
        Ok(res) => Ok(()),
        Err(e) => Err(e),
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

#[post("/oauth/introspect", data = "<body>")]
pub async fn oauth_introspect(
    shared_oauth_provider: &State<SharedOAuthProvider>,
    body: OAuthIntrospectRequestBody,
) -> Result<Json<OAuthIntrospectionResponse>, OAuthError> {
    let mut oauth_provider = shared_oauth_provider.oauth_provider.write().await;
    let res = oauth_provider
        .introspect(
            body.oauth_client_credentials,
            body.oauth_token_identification,
        )
        .await?;
    Ok(Json(res))
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

#[get("/oauth/authorize")]
pub async fn oauth_authorize(
    shared_oauth_provider: &State<SharedOAuthProvider>,
    body: OAuthAuthorizeRequestBody,
) {
    unimplemented!()
}

// #[post("/oauth/authorize/sign-in")]
// pub async fn oauth_authorize_signin(
//     shared_oauth_provider: &State<SharedOAuthProvider>,
//     shared_device_manager: &State<SharedDeviceManager>,
//     body: OAuthSigninRequestBody,
// ) {
//     let oauth_provider = shared_oauth_provider.oauth_provider.write().await;
//     let device_manager = shared_device_manager.device_manager.read().await;
//     // device_manager.load()
//
//     let data = oauth_provider
//         .authorize(
//             &body.device_id,
//             &body.credentials,
//             &body.authorization_request,
//         )
//         .await;
//
//     match data {
//         Ok(data) => match data {
//             AuthorizationResult::Redirect => {}
//             AuthorizationResult::Authorize => {}
//         },
//         Err(e) => {}
//     }
// }

pub struct AuthorizeAccept {
    // pub device_id: DeviceId,
    // pub request_uri: RequestUri,
    // pub client_id:  OAuthClientId,
    // pub account_sub: Sub
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for AuthorizeAccept {
    type Error = ();

    async fn from_request(req: &'r Request<'_>) -> rocket::request::Outcome<Self, Self::Error> {
        match validate_fetch_site(req, vec!["same-origin"]) {
            Ok(_) => rocket::request::Outcome::Success(Self {}),
            Err(e) => rocket::request::Outcome::Error((Status::new(400), ())),
        }
        //
        // let request_uri = req.query_value("request_uri");
        // let client_id = req.query_value("client_id");
        // let sub = req.query_value("account_sub");
        // // let csrf_cookie =
    }
}

// Though this is a "no-cors" request, meaning that the browser will allow
// any cross-origin request, with credentials, to be sent, the handler will
// 1) validate the request origin,
// 2) validate the CSRF token,
// 3) validate the referer,
// 4) validate the sec-fetch-site header,
// 4) validate the sec-fetch-mode header,
// 5) validate the sec-fetch-dest header (see navigationHandler).
// And will error if any of these checks fail.
#[get("/oauth/authorize/accept")]
pub async fn oauth_authorize_accept(
    shared_oauth_provider: &State<SharedOAuthProvider>,
    authorize_accept: AuthorizeAccept,
) {
    unimplemented!()
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

// Though this is a "no-cors" request, meaning that the browser will allow
// any cross-origin request, with credentials, to be sent, the handler will
// 1) validate the request origin,
// 2) validate the CSRF token,
// 3) validate the referer,
// 4) validate the sec-fetch-site header,
// 4) validate the sec-fetch-mode header,
// 5) validate the sec-fetch-dest header (see navigationHandler).
// And will error if any of these checks fail.
#[get("/oauth/authorize/reject")]
pub async fn oauth_authorize_reject(
    shared_oauth_provider: &State<SharedOAuthProvider>,
    authorize_reject: AuthorizeReject,
) {
    unimplemented!()
}
