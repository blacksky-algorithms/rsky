use crate::models::{InternalErrorCode, InternalErrorMessageResponse};
use crate::DbConn;
use chrono::offset::Utc as UtcOffset;
use chrono::DateTime;
use diesel::prelude::*;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::{
    AccountCodes, CreateInviteCodesInput, CreateInviteCodesOutput,
};
use std::time::SystemTime;

#[rocket::post(
    "/xrpc/com.atproto.server.createInviteCodes",
    format = "json",
    data = "<body>"
)]
pub async fn create_invite_codes(
    body: Json<CreateInviteCodesInput>,
    connection: DbConn,
) -> Result<Json<CreateInviteCodesOutput>, status::Custom<Json<InternalErrorMessageResponse>>> {
    use crate::schema::pds::invite_code::dsl as InviteCodeSchema;

    let result = connection
        .run(move |conn| {
            let body: CreateInviteCodesInput = body.into_inner();
            let for_accounts;
            if let Some(for_accounts_input) = body.for_accounts {
                if for_accounts_input.len() == 0 {
                    for_accounts = vec!["admin".to_owned()];
                } else {
                    for_accounts = for_accounts_input;
                }
            } else {
                for_accounts = vec!["admin".to_owned()];
            }
            let mut new_invite_codes = Vec::new();
            let mut account_codes = Vec::new();

            for_accounts
                .into_iter()
                .map(|account| {
                    let system_time = SystemTime::now();
                    let dt: DateTime<UtcOffset> = system_time.into();
                    let codes = super::gen_invite_codes(body.code_count);
                    for code in &codes {
                        new_invite_codes.push((
                            InviteCodeSchema::code.eq(code.clone()),
                            InviteCodeSchema::availableUses.eq(&body.use_count),
                            InviteCodeSchema::disabled.eq(0),
                            InviteCodeSchema::forUser.eq(account.clone()),
                            InviteCodeSchema::createdBy.eq("admin".to_string()),
                            InviteCodeSchema::createdAt.eq(format!("{}", dt.format("%+"))),
                        ))
                    }
                    account_codes.push(AccountCodes {
                        account: account.clone(),
                        codes,
                    });
                })
                .for_each(drop);
            match diesel::insert_into(InviteCodeSchema::invite_code)
                .values(&new_invite_codes)
                .execute(conn)
            {
                Ok(_) => Ok(Json(CreateInviteCodesOutput {
                    codes: account_codes,
                })),
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
