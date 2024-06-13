use crate::auth_verifier::OptionalAccessOrAdminToken;

/// Get a blob associated with a given account. Returns the full blob as originally uploaded.
/// Does not require auth; implemented by PDS.
#[rocket::get("/xrpc/com.atproto.sync.getBlob?<did>&<cid>")]
pub async fn get_blob(did: String, cid: String, auth: OptionalAccessOrAdminToken) {
    todo!();
}
