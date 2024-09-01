use crate::account_manager::helpers::invite::{get_invite_codes_uses, CodeDetail};
use crate::auth_verifier::Moderator;
use crate::common::time::{from_millis_to_utc, from_str_to_millis};
use crate::common::RFC3339_VARIANT;
use crate::db::establish_connection;
use crate::models::{models, ErrorCode, ErrorMessageResponse};
use anyhow::{anyhow, bail, Result};
use diesel::dsl::sql;
use diesel::prelude::*;
use diesel::sql_types::{Bool, Text};
use diesel::QueryDsl;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::admin::GetInviteCodesOutput;
use std::mem;

#[derive(Debug, Clone)]
pub struct TimeCodeResult {
    pub created_at: String,
    pub code: String,
}

pub struct Cursor {
    pub primary: String,
    pub secondary: String,
}

pub struct TimeCodeKeySet {}

pub struct KeySetPaginateOpts {
    pub limit: Option<i64>,
    pub cursor: Option<String>,
    pub direction: Option<String>,
}

/// The GenericKeyset is an abstract class that sets-up the interface and partial implementation
/// of a keyset-paginated cursor with two parts. There are three types involved:
///  - Result: a raw result (i.e. a row from the db) containing data that will make-up a cursor.
///    - E.g. { createdAt: '2022-01-01T12:00:00Z', cid: 'bafyx' }
///  - LabeledResult: a Result processed such that the "primary" and "secondary" parts of the cursor are labeled.
///    - E.g. { primary: '2022-01-01T12:00:00Z', secondary: 'bafyx' }
///  - Cursor: the two string parts that make-up the packed/string cursor.
///    - E.g. packed cursor '1641038400000::bafyx' in parts { primary: '1641038400000', secondary: 'bafyx' }
///
/// These types relate as such. Implementers define the relations marked with a *:
///   Result -*-> LabeledResult <-*-> Cursor <--> packed/string cursor
///                     ↳ SQL Condition
impl TimeCodeKeySet {
    pub fn new() -> Self {
        TimeCodeKeySet {}
    }

    pub fn label_result(&self, result: TimeCodeResult) -> Cursor {
        Cursor {
            primary: result.created_at,
            secondary: result.code,
        }
    }

    pub fn labeled_result_to_cursor(&self, labeled: Cursor) -> Result<Cursor> {
        Ok(Cursor {
            primary: from_str_to_millis(&labeled.primary)?.to_string(),
            secondary: labeled.secondary,
        })
    }

    pub fn cursor_to_labeled_result(&self, cursor: Cursor) -> Result<Cursor> {
        let primary_date = from_millis_to_utc(
            cursor
                .primary
                .parse::<i64>()
                .map_err(|_| anyhow!("Malformed cursor"))?,
        );
        Ok(Cursor {
            primary: format!("{}", primary_date.format(RFC3339_VARIANT)),
            secondary: cursor.secondary,
        })
    }

    pub fn pack_from_result(&self, results: Vec<TimeCodeResult>) -> Result<Option<String>> {
        match results.last() {
            None => Ok(None),
            Some(result) => self.pack(Some(self.label_result(result.clone()))),
        }
    }

    pub fn pack(&self, labeled: Option<Cursor>) -> Result<Option<String>> {
        match labeled {
            None => Ok(None),
            Some(labeled) => {
                let cursor = self.labeled_result_to_cursor(labeled)?;
                Ok(self.pack_cursor(Some(cursor)))
            }
        }
    }

    pub fn unpack(&self, cursor_str: Option<String>) -> Result<Option<Cursor>> {
        match self.unpack_cursor(cursor_str)? {
            None => Ok(None),
            Some(cursor) => Ok(Some(self.cursor_to_labeled_result(cursor)?)),
        }
    }

    pub fn pack_cursor(&self, cursor: Option<Cursor>) -> Option<String> {
        match cursor {
            None => None,
            Some(cursor) => Some(format!("{0}::{1}", cursor.primary, cursor.secondary)),
        }
    }

    pub fn unpack_cursor(&self, cursor_str: Option<String>) -> Result<Option<Cursor>> {
        match cursor_str {
            None => Ok(None),
            Some(cursor_str) => {
                let result = cursor_str.split("::").collect::<Vec<&str>>();
                match (result.get(0), result.get(1), result.get(2)) {
                    (Some(primary), Some(secondary), None) => Ok(Some(Cursor {
                        primary: primary.to_string(),
                        secondary: secondary.to_string(),
                    })),
                    _ => bail!("Malformed cursor"),
                }
            }
        }
    }

    pub async fn paginate(&self, opts: KeySetPaginateOpts) -> Result<Vec<CodeDetail>> {
        let KeySetPaginateOpts {
            limit,
            cursor,
            direction,
        } = opts;
        let direction = direction.unwrap_or("desc".to_string());

        use crate::schema::pds::invite_code::dsl as InviteCodeSchema;
        let conn = &mut establish_connection()?;

        let labeled = self.unpack(cursor)?;

        let mut builder = InviteCodeSchema::invite_code
            .select(models::InviteCode::as_select())
            .into_boxed();

        if let Some(limit) = limit {
            builder = builder.limit(limit);
        }

        if direction == "desc" {
            builder = builder.order((
                InviteCodeSchema::createdAt.desc(),
                InviteCodeSchema::code.desc(),
            ));
        } else {
            builder = builder.order((
                InviteCodeSchema::createdAt.asc(),
                InviteCodeSchema::code.asc(),
            ));
        }

        if let Some(labeled) = labeled {
            if direction == "asc" {
                builder = builder.filter(
                    sql::<Bool>("((")
                        .bind(InviteCodeSchema::createdAt)
                        .sql(", ")
                        .bind(InviteCodeSchema::code)
                        .sql(") > (")
                        .bind::<Text, _>(labeled.primary)
                        .sql(", ")
                        .bind::<Text, _>(labeled.secondary)
                        .sql("))"),
                );
            } else {
                builder = builder.filter(
                    sql::<Bool>("((")
                        .bind(InviteCodeSchema::createdAt)
                        .sql(", ")
                        .bind(InviteCodeSchema::code)
                        .sql(") < (")
                        .bind::<Text, _>(labeled.primary)
                        .sql(", ")
                        .bind::<Text, _>(labeled.secondary)
                        .sql("))"),
                );
            }
        }

        let res = builder.load(conn)?;
        let codes: Vec<String> = res.iter().map(|row| row.code.clone()).collect();
        let mut uses = get_invite_codes_uses(codes).await?;

        Ok(res
            .into_iter()
            .map(|row| CodeDetail {
                code: row.code.clone(),
                available: row.available_uses,
                disabled: row.disabled == 1,
                for_account: row.for_account,
                created_by: row.created_by,
                created_at: row.created_at,
                uses: mem::take(uses.get_mut(&row.code).unwrap_or(&mut Vec::new())),
            })
            .collect::<Vec<CodeDetail>>())
    }
}

pub struct UseCodeResult {
    pub uses: u16,
    pub code: String,
}

pub struct UseCodeKeyset {}

impl UseCodeKeyset {
    pub fn new() -> Self {
        UseCodeKeyset {}
    }

    pub fn label_result(&self, result: TimeCodeResult) -> Cursor {
        Cursor {
            primary: result.created_at,
            secondary: result.code,
        }
    }

    pub fn labeled_result_to_cursor(&self, labeled: Cursor) -> Result<Cursor> {
        Ok(Cursor {
            primary: from_str_to_millis(&labeled.primary)?.to_string(),
            secondary: labeled.secondary,
        })
    }

    pub fn cursor_to_labeled_result(&self, cursor: Cursor) -> Result<Cursor> {
        let primary_date = from_millis_to_utc(
            cursor
                .primary
                .parse::<i64>()
                .map_err(|_| anyhow!("Malformed cursor"))?,
        );
        Ok(Cursor {
            primary: format!("{}", primary_date.format(RFC3339_VARIANT)),
            secondary: cursor.secondary,
        })
    }

    pub fn pack_from_result(&self, results: Vec<TimeCodeResult>) -> Result<Option<String>> {
        match results.last() {
            None => Ok(None),
            Some(result) => self.pack(Some(self.label_result(result.clone()))),
        }
    }

    pub fn pack(&self, labeled: Option<Cursor>) -> Result<Option<String>> {
        match labeled {
            None => Ok(None),
            Some(labeled) => {
                let cursor = self.labeled_result_to_cursor(labeled)?;
                Ok(self.pack_cursor(Some(cursor)))
            }
        }
    }

    pub fn unpack(&self, cursor_str: Option<String>) -> Result<Option<Cursor>> {
        match self.unpack_cursor(cursor_str)? {
            None => Ok(None),
            Some(cursor) => Ok(Some(self.cursor_to_labeled_result(cursor)?)),
        }
    }

    pub fn pack_cursor(&self, cursor: Option<Cursor>) -> Option<String> {
        match cursor {
            None => None,
            Some(cursor) => Some(format!("{0}::{1}", cursor.primary, cursor.secondary)),
        }
    }

    pub fn unpack_cursor(&self, cursor_str: Option<String>) -> Result<Option<Cursor>> {
        match cursor_str {
            None => Ok(None),
            Some(cursor_str) => {
                let result = cursor_str.split("::").collect::<Vec<&str>>();
                match (result.get(0), result.get(1), result.get(2)) {
                    (Some(primary), Some(secondary), None) => Ok(Some(Cursor {
                        primary: primary.to_string(),
                        secondary: secondary.to_string(),
                    })),
                    _ => bail!("Malformed cursor"),
                }
            }
        }
    }

    // @TODO: Fix issues with `invitecodeuse.count() as uses` subquery
    pub async fn paginate(&self, _opts: KeySetPaginateOpts) -> Result<Vec<CodeDetail>> {
        unimplemented!()
    }
}

async fn inner_get_invite_codes(
    sort: Option<String>,
    limit: Option<i64>,
    cursor: Option<String>,
) -> Result<GetInviteCodesOutput> {
    let (res, result_cursor) = match sort {
        Some(sort) if sort == "recent" => {
            let keyset = TimeCodeKeySet::new();
            let result = keyset
                .paginate(KeySetPaginateOpts {
                    limit,
                    cursor,
                    direction: None,
                })
                .await?;
            let time_code_results = result
                .iter()
                .map(|row| TimeCodeResult {
                    created_at: row.created_at.clone(),
                    code: row.code.clone(),
                })
                .collect::<Vec<TimeCodeResult>>();
            (result, keyset.pack_from_result(time_code_results)?)
        }
        Some(sort) if sort == "usage" => {
            unimplemented!()
        }
        _ => bail!("Unknown sort method: {:?}", sort),
    };

    Ok(GetInviteCodesOutput {
        cursor: result_cursor,
        codes: res,
    })
}

#[rocket::get("/xrpc/com.atproto.admin.getInviteCodes?<sort>&<limit>&<cursor>")]
pub async fn get_invite_codes(
    sort: Option<String>,
    limit: Option<i64>,
    cursor: Option<String>,
    _auth: Moderator,
) -> Result<Json<GetInviteCodesOutput>, status::Custom<Json<ErrorMessageResponse>>> {
    match inner_get_invite_codes(sort, limit, cursor).await {
        Ok(res) => Ok(Json(res)),
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
