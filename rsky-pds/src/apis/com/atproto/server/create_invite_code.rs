use rocket::serde::json::Json;
use rocket::response::status;
use rocket::http::Status;
use std::time::SystemTime;
use chrono::offset::Utc as UtcOffset;
use chrono::DateTime;
use diesel::prelude::*;
use crate::DbConn;
use rsky_lexicon::com::atproto::server::{CreateInviteCodeInput, CreateInviteCodeOutput};
use crate::models::{InternalErrorMessageResponse, InternalErrorCode};

#[rocket::post("/xrpc/com.atproto.server.createInviteCode", format = "json", data = "<body>")]
pub async fn create_invite_code(
    body: Json<CreateInviteCodeInput>,
    connection: DbConn
) -> Result<Json<CreateInviteCodeOutput>, status::Custom<Json<InternalErrorMessageResponse>>>  {
    use crate::schema::pds::invite_code::dsl::*;

    let result = connection
        .run(move |conn| {
            let body: CreateInviteCodeInput = body.into_inner();
            let system_time = SystemTime::now();
            let dt: DateTime<UtcOffset> = system_time.into();
            let new_invite_code = (
                code.eq("blacksky-app-1234".to_string()),
                availableUses.eq(body.use_count),
                disabled.eq(0),
                forUser.eq(body.for_account.unwrap_or("admin".to_string())),
                createdBy.eq("admin".to_string()),
                createdAt.eq(format!("{}", dt.format("%+"))),
            );

            match diesel::insert_into(invite_code)
                .values(&new_invite_code)
                .execute(conn) {
                Ok(_) => Ok(Json(CreateInviteCodeOutput {code: "blacksky-app-1234".into()})),
                Err(error) => {
                    eprintln!("Internal Error: {error}");
                    let internal_error = InternalErrorMessageResponse {
                        code: Some(InternalErrorCode::InternalError),
                        message: Some(error.to_string()),
                    };
                    Err(status::Custom(
                        Status::InternalServerError,
                        Json(internal_error),
                    ))
                }
            }
        })
        .await;

    result
}