use crate::models::{InternalErrorCode, InternalErrorMessageResponse};
use crate::DbConn;
use diesel::prelude::*;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::GetAccountInviteCodesOutput;

// Requires user session authorization
#[allow(non_snake_case)]
#[allow(unused_variables)]
#[rocket::get("/xrpc/com.atproto.server.getAccountInviteCodes?<includeUsed>&<createAvailable>")]
pub async fn get_account_invite_codes(
    includeUsed: bool,
    createAvailable: bool,
) -> Result<Json<GetAccountInviteCodesOutput>, status::Custom<Json<InternalErrorMessageResponse>>> {
    todo!();
}
