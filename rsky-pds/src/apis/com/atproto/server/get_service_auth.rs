use crate::account_manager::helpers::auth::{create_service_jwt, ServiceJwtParams};
use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use crate::auth_verifier::AccessFull;
use crate::pipethrough::{PRIVILEGED_METHODS, PROTECTED_METHODS};
use anyhow::{bail, Result};
use chrono::offset::Utc as UtcOffset;
use chrono::DateTime;
use rocket::serde::json::Json;
use rocket::State;
use rsky_common::time::{from_micros_to_utc, HOUR, MINUTE};
use rsky_lexicon::com::atproto::server::GetServiceAuthOutput;
use rsky_syntax::did::ensure_valid_did;
use std::time::SystemTime;

/// Denies a request for a service-auth token when the *requested* method
/// (`lxm`) is privileged (the `chat.bsky.*` surface plus
/// `com.atproto.server.createAccount`) and the *caller's own* session is not
/// itself privileged.
///
/// Mirrors the upstream TS reference (`getServiceAuth.ts`):
/// `lxm != null && PRIVILEGED_METHODS.has(lxm) && !isAccessPrivileged(scope)`.
///
/// This is intentionally the inverse of a naive "gate privileged sessions"
/// check: a plain (non-privileged) app-password session must never be able
/// to mint a token for a privileged method such as
/// `chat.bsky.convo.getMessages`, while a fully-privileged session (full
/// `Access` or `AppPassPrivileged`) must be allowed to request a token for
/// any method, privileged or not.
fn ensure_lxm_access(lxm: &str, is_privileged: bool) -> Result<()> {
    if PRIVILEGED_METHODS.contains(lxm) && !is_privileged {
        bail!("insufficient access to request a service auth token for the following method: {lxm}");
    }
    Ok(())
}

/// Validates that `aud` is a syntactically valid atproto DID, optionally
/// followed by a `#serviceId` fragment (e.g. `did:web:example.com#atproto_labeler`),
/// matching the upstream check `isAtprotoDid(aud) || isAtprotoDidRefAbsolute(aud)`.
fn ensure_valid_aud(aud: &str) -> Result<()> {
    let did_part = aud.split('#').next().unwrap_or(aud);
    ensure_valid_did(did_part).map_err(|_| {
        anyhow::anyhow!("aud must be a valid atproto DID or did#serviceId reference")
    })
}

pub async fn inner_get_service_auth(
    aud: String,
    exp: Option<u64>,
    lxm: Option<String>,
    auth: AccessFull,
    actor_store: &State<ActorStore>,
) -> Result<String> {
    let credentials = auth.access.credentials.unwrap();
    let did = credentials.clone().did.unwrap();
    ensure_valid_aud(&aud)?;
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
        ensure_lxm_access(lxm.as_str(), credentials.is_privileged.unwrap_or(false))?;
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
        Ok(token) => Ok(Json(GetServiceAuthOutput { token })),
        Err(error) => {
            tracing::error!("Internal Error: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHAT_LXM: &str = "chat.bsky.convo.getMessages";
    const CREATE_ACCOUNT_LXM: &str = "com.atproto.server.createAccount";
    const NON_PRIVILEGED_LXM: &str = "app.bsky.feed.getTimeline";

    // --- ensure_lxm_access: the privilege gate that was previously inverted ---
    //
    // These four tests are written to fail against the *old* (backwards)
    // condition `is_privileged && PRIVILEGED_METHODS.contains(lxm)` and pass
    // against the fixed condition `PRIVILEGED_METHODS.contains(lxm) &&
    // !is_privileged`. This was confirmed by temporarily restoring the old
    // condition locally and observing `plain_app_password_session_is_denied_for_chat_method`
    // and `plain_app_password_session_is_denied_for_create_account` fail (they
    // passed under the old code, i.e. incorrectly allowed access), before
    // re-applying the fix.

    #[test]
    fn plain_app_password_session_is_denied_for_chat_method() {
        // A plain (non-privileged) app-password session must NOT be able to
        // mint a service-auth token for a chat.bsky.* method.
        assert!(ensure_lxm_access(CHAT_LXM, false).is_err());
    }

    #[test]
    fn plain_app_password_session_is_denied_for_create_account() {
        assert!(ensure_lxm_access(CREATE_ACCOUNT_LXM, false).is_err());
    }

    #[test]
    fn privileged_session_is_allowed_for_chat_method() {
        // A fully-privileged session (full `Access` or `AppPassPrivileged`)
        // must be allowed to request a token for a chat.bsky.* method.
        assert!(ensure_lxm_access(CHAT_LXM, true).is_ok());
    }

    #[test]
    fn privileged_session_is_allowed_for_create_account() {
        assert!(ensure_lxm_access(CREATE_ACCOUNT_LXM, true).is_ok());
    }

    #[test]
    fn non_privileged_method_is_allowed_regardless_of_privilege_level() {
        // Non-privileged methods must be allowed regardless of the caller's
        // privilege level.
        assert!(ensure_lxm_access(NON_PRIVILEGED_LXM, false).is_ok());
        assert!(ensure_lxm_access(NON_PRIVILEGED_LXM, true).is_ok());
    }

    // --- ensure_valid_aud ---

    #[test]
    fn aud_validation_rejects_non_did_values() {
        assert!(ensure_valid_aud("not-a-did").is_err());
        assert!(ensure_valid_aud("https://example.com").is_err());
        assert!(ensure_valid_aud("").is_err());
    }

    #[test]
    fn aud_validation_accepts_plain_did_and_service_ref() {
        assert!(ensure_valid_aud("did:web:example.com").is_ok());
        assert!(ensure_valid_aud("did:plc:7iza6de2dwap2sbkpav7c6c6").is_ok());
        assert!(ensure_valid_aud("did:web:example.com#atproto_labeler").is_ok());
    }
}
