use crate::apis::ApiError;
use crate::auth_verifier::AccessStandardSignupQueued;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::temp::CheckSignupQueueOutput;

/// Check accounts location in signup queue. Since rsky-pds is not an entryway,
/// accounts are never queued and are always activated.
#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.temp.checkSignupQueue")]
pub async fn check_signup_queue(
    _auth: AccessStandardSignupQueued,
) -> Result<Json<CheckSignupQueueOutput>, ApiError> {
    Ok(Json(CheckSignupQueueOutput {
        activated: true,
        place_in_queue: None,
        estimated_time_ms: None,
    }))
}
