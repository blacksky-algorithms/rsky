use crate::auth_verifier::OptionalAccessOrAdminToken;

#[rocket::get("/xrpc/com.atproto.sync.getBlob?<did>&<cid>")]
pub async fn get_blob(did: String, cid: String, auth: OptionalAccessOrAdminToken) {
    todo!();
}
