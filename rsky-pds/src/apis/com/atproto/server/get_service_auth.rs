use crate::account_manager::helpers::auth::{create_service_jwt, ServiceJwtParams};
use crate::apis::ApiError;
use crate::auth_verifier::AccessFull;
use crate::pipethrough::{PRIVILEGED_METHODS, PROTECTED_METHODS};
use anyhow::{bail, Result};
use chrono::offset::Utc as UtcOffset;
use chrono::DateTime;
use rocket::serde::json::Json;
use rsky_common::time::{from_micros_to_utc, HOUR, MINUTE};
use rsky_lexicon::com::atproto::server::GetServiceAuthOutput;
use std::time::SystemTime;

/// Validate a requested service-auth expiry against atproto's bounds.
///
/// `exp` is a Unix timestamp in seconds (per the lexicon); it is converted to
/// microseconds for [`from_micros_to_utc`]. A method-less token may sit at most
/// a minute in the future, any token at most an hour.
fn validate_service_auth_exp(
    exp_seconds: u64,
    now: DateTime<UtcOffset>,
    has_lxm: bool,
) -> Result<()> {
    let exp_micros = (exp_seconds as i64) * 1_000_000;
    let diff = from_micros_to_utc(exp_micros) - now;
    if diff.num_milliseconds() < 0 {
        bail!("BadExpiration: expiration is in past");
    } else if diff.num_milliseconds() > HOUR as i64 {
        bail!("BadExpiration: cannot request a token with an expiration more than an hour in the future");
    } else if !has_lxm && diff.num_milliseconds() > MINUTE as i64 {
        bail!("BadExpiration: cannot request a method-less token with an expiration more than a minute in the future");
    }
    Ok(())
}

pub async fn inner_get_service_auth(
    aud: String,
    exp: Option<u64>,
    lxm: Option<String>,
    auth: AccessFull,
) -> Result<String> {
    let credentials = auth.access.credentials.unwrap();
    let did = credentials.clone().did.unwrap();
    if let Some(exp) = exp {
        let now: DateTime<UtcOffset> = SystemTime::now().into();
        validate_service_auth_exp(exp, now, lxm.is_some())?;
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
    })
    .await
}

/// Get a signed token on behalf of the requesting DID for the requested service.
#[tracing::instrument(skip_all)]
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
) -> Result<Json<GetServiceAuthOutput>, ApiError> {
    match inner_get_service_auth(aud, exp, lxm, auth).await {
        Ok(token) => Ok(Json(GetServiceAuthOutput { token })),
        Err(error) => {
            tracing::error!("Internal Error: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::validate_service_auth_exp;
    use chrono::offset::Utc as UtcOffset;
    use chrono::{DateTime, TimeZone};

    // A fixed "now": 2023-11-14T22:13:20Z (1_700_000_000 seconds).
    fn now() -> DateTime<UtcOffset> {
        UtcOffset.timestamp_opt(1_700_000_000, 0).unwrap()
    }

    #[test]
    fn accepts_expiry_thirty_minutes_out_with_lxm() {
        // Regression: the old code multiplied seconds by 1_000 (milliseconds)
        // before a microsecond-based conversion, so every real expiry landed
        // near 1970 and failed "expiration is in past".
        let exp = 1_700_000_000 + 30 * 60;
        assert!(validate_service_auth_exp(exp, now(), true).is_ok());
    }

    #[test]
    fn accepts_method_less_expiry_under_a_minute() {
        let exp = 1_700_000_000 + 30;
        assert!(validate_service_auth_exp(exp, now(), false).is_ok());
    }

    #[test]
    fn rejects_expiry_in_the_past() {
        let exp = 1_700_000_000 - 1;
        let err = validate_service_auth_exp(exp, now(), true).unwrap_err();
        assert!(err.to_string().contains("in past"));
    }

    #[test]
    fn rejects_expiry_more_than_an_hour_out() {
        let exp = 1_700_000_000 + 60 * 60 + 1;
        let err = validate_service_auth_exp(exp, now(), true).unwrap_err();
        assert!(err.to_string().contains("more than an hour"));
    }

    #[test]
    fn rejects_method_less_expiry_over_a_minute() {
        let exp = 1_700_000_000 + 61;
        let err = validate_service_auth_exp(exp, now(), false).unwrap_err();
        assert!(err.to_string().contains("method-less"));
    }
}
