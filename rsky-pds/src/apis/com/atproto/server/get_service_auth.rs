use crate::account_manager::helpers::auth::{create_service_jwt, ServiceJwtParams};
use crate::auth_verifier::AccessFull;
use crate::common::time::{from_micros_to_utc, HOUR, MINUTE};
use crate::models::{ErrorCode, ErrorMessageResponse};
use crate::pipethrough::{PRIVILEGED_METHODS, PROTECTED_METHODS};
use anyhow::{bail, Result};
use chrono::offset::Utc as UtcOffset;
use chrono::DateTime;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::GetServiceAuthOutput;
use secp256k1::SecretKey;
use std::env;
use std::time::SystemTime;

pub async fn inner_get_service_auth(
    aud: String,
    exp: Option<u64>,
    lxm: Option<String>,
    auth: AccessFull,
) -> Result<String> {
    let credentials = auth.access.credentials.unwrap();
    let did = credentials.clone().did.unwrap();
    // We just use the repo signing key
    let private_key = env::var("PDS_REPO_SIGNING_KEY_K256_PRIVATE_KEY_HEX").unwrap();
    let keypair = SecretKey::from_slice(&hex::decode(private_key.as_bytes()).unwrap()).unwrap();
    let exp = match exp {
        None => None,
        Some(exp) => Some(exp * 1000),
    };
    if let Some(exp) = exp {
        let system_time = SystemTime::now();
        let now: DateTime<UtcOffset> = system_time.into();
        let diff = from_micros_to_utc(exp as i64) - now;
        if diff.num_milliseconds() < 0 {
            bail!("BadExpiration: expiration is in past");
        } else if diff.num_milliseconds() > HOUR as i64 {
            bail!("BadExpiration: cannot request a token with an expiration more than an hour in the future");
        } else if lxm.is_none() && diff.num_milliseconds() > MINUTE as i64 {
            bail!("BadExpiration: cannot request a method-less token with an expiration more than a minute in the future");
        }
    }
    if let Some(ref lxm) = lxm {
        if PROTECTED_METHODS.contains(lxm.as_str()) {
            bail!("cannot request a service auth token for the following protected method: {lxm}");
        }
        if credentials.is_privileged.unwrap_or(false) && PRIVILEGED_METHODS.contains(lxm.as_str()) {
            bail!("insufficient access to request a service auth token for the following method: {lxm}");
        }
    }
    create_service_jwt(ServiceJwtParams {
        iss: did,
        aud,
        exp: None,
        lxm,
        jti: None,
        keypair,
    })
    .await
}

/// Get a signed token on behalf of the requesting DID for the requested service.
#[rocket::get("/xrpc/com.atproto.server.getServiceAuth?<aud>&<exp>&<lxm>")]
pub async fn get_service_auth(
    // The DID of the service that the token will be used to authenticate with
    aud: String,
    // The time in Unix Epoch seconds that the JWT expires. Defaults to 60 seconds in the future.
    // The service may enforce certain time bounds on tokens depending on the requested scope.
    exp: Option<u64>,
    // Lexicon (XRPC) method to bind the requested token to
    lxm: Option<String>,
    auth: AccessFull,
) -> Result<Json<GetServiceAuthOutput>, status::Custom<Json<ErrorMessageResponse>>> {
    match inner_get_service_auth(aud, exp, lxm, auth).await {
        Ok(token) => Ok(Json(GetServiceAuthOutput { token })),
        Err(error) => {
            eprintln!("Internal Error: {error}");
            let internal_error = ErrorMessageResponse {
                code: Some(ErrorCode::InternalServerError),
                message: Some("Internal error".to_string()),
            };
            return Err(status::Custom(
                Status::InternalServerError,
                Json(internal_error),
            ));
        }
    }
}
