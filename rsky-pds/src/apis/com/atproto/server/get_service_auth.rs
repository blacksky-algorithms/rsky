use crate::account_manager::helpers::auth::{create_service_jwt, ServiceJwtParams};
use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use crate::auth_verifier::AccessFull;
use crate::metrics::{record_service_token_denied, record_service_token_issued};
use crate::pipethrough::{PRIVILEGED_METHODS, PROTECTED_METHODS};
use anyhow::{bail, Result};
use chrono::offset::Utc as UtcOffset;
use chrono::DateTime;
use rocket::serde::json::Json;
use rocket::State;
use rsky_common::time::{from_micros_to_utc, HOUR, MINUTE};
use rsky_lexicon::com::atproto::server::GetServiceAuthOutput;
use std::time::SystemTime;

pub async fn inner_get_service_auth(
    aud: String,
    exp: Option<u64>,
    lxm: Option<String>,
    auth: AccessFull,
    actor_store: &State<ActorStore>,
) -> Result<String> {
    let credentials = auth.access.credentials.unwrap();
    let did = credentials.clone().did.unwrap();
    // `exp` is seconds since the epoch (RFC 7519 §4.1.4).
    if let Some(exp) = exp {
        let system_time = SystemTime::now();
        let now: DateTime<UtcOffset> = system_time.into();
        let diff = from_micros_to_utc((exp * 1_000_000) as i64) - now;
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
    let keypair = actor_store.keypair(&did).await?;
    create_service_jwt(
        ServiceJwtParams {
            iss: did,
            aud,
            exp,
            lxm,
            jti: None,
        },
        &keypair,
    )
    .await
}

/// Classifies a denial from [`inner_get_service_auth`] into a small,
/// low-cardinality reason label for [`record_service_token_denied`].
///
/// Matches on the `bail!` message text rather than a typed error, since
/// `inner_get_service_auth` returns a plain `anyhow::Result`; the messages
/// matched here are exactly the ones raised above, so this stays in sync by
/// construction as long as both live in this file. Falls back to
/// `"internal_error"` for anything unrecognized (e.g. a `keypair`/JWT
/// signing failure) rather than growing an ever-expanding label set.
fn deny_reason(err: &anyhow::Error) -> &'static str {
    let msg = err.to_string();
    if msg.starts_with("BadExpiration") {
        "bad_expiration"
    } else if msg.contains("protected method") {
        "protected_method"
    } else if msg.contains("insufficient access") {
        "insufficient_privilege"
    } else {
        "internal_error"
    }
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
    actor_store: &State<ActorStore>,
) -> Result<Json<GetServiceAuthOutput>, ApiError> {
    match inner_get_service_auth(aud, exp, lxm, auth, actor_store).await {
        Ok(token) => {
            record_service_token_issued();
            Ok(Json(GetServiceAuthOutput { token }))
        }
        Err(error) => {
            record_service_token_denied(deny_reason(&error));
            tracing::error!("Internal Error: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::deny_reason;

    #[test]
    fn deny_reason_classifies_known_bail_messages() {
        assert_eq!(
            deny_reason(&anyhow::anyhow!("BadExpiration: expiration is in past")),
            "bad_expiration"
        );
        assert_eq!(
            deny_reason(&anyhow::anyhow!(
                "cannot request a service auth token for the following protected method: com.atproto.server.createAccount"
            )),
            "protected_method"
        );
        assert_eq!(
            deny_reason(&anyhow::anyhow!(
                "insufficient access to request a service auth token for the following method: chat.bsky.convo.getMessages"
            )),
            "insufficient_privilege"
        );
        assert_eq!(
            deny_reason(&anyhow::anyhow!("some unrelated keypair failure")),
            "internal_error"
        );
    }
}
