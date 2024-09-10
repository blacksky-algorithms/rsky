use crate::account_manager::helpers::account::{
    format_account_status, AccountStatus, ActorAccount, FormattedAccountStatus,
};
use crate::common::time::{from_millis_to_utc, from_str_to_millis};
use crate::common::RFC3339_VARIANT;
use crate::db::establish_connection;
use crate::models::{ErrorCode, ErrorMessageResponse};
use anyhow::{anyhow, bail, Result};
use diesel::dsl::sql;
use diesel::prelude::*;
use diesel::sql_types::{Bool, Text};
use diesel::QueryDsl;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::sync::{ListReposOutput, RefRepo as LexiconRepo, RepoStatus};

#[derive(Debug, Clone)]
pub struct TimeDidResult {
    pub created_at: String,
    pub did: String,
}

pub struct Cursor {
    pub primary: String,
    pub secondary: String,
}

pub struct TimeDidKeySet {}

pub struct KeySetPaginateOpts {
    pub limit: i64,
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
impl TimeDidKeySet {
    pub fn new() -> Self {
        TimeDidKeySet {}
    }

    pub fn label_result(&self, result: TimeDidResult) -> Cursor {
        Cursor {
            primary: result.created_at,
            secondary: result.did,
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

    pub fn pack_from_result(&self, results: Vec<TimeDidResult>) -> Result<Option<String>> {
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

    pub async fn paginate(
        &self,
        opts: KeySetPaginateOpts,
    ) -> Result<
        Vec<(
            String,
            String,
            String,
            String,
            Option<String>,
            Option<String>,
        )>,
    > {
        let KeySetPaginateOpts {
            limit,
            cursor,
            direction,
        } = opts;
        let direction = direction.unwrap_or("desc".to_string());

        use crate::schema::pds::actor::dsl as ActorSchema;
        use crate::schema::pds::repo_root::dsl as RepoRootSchema;
        let conn = &mut establish_connection()?;

        let labeled = self.unpack(cursor)?;

        let mut builder = ActorSchema::actor
            .inner_join(RepoRootSchema::repo_root.on(RepoRootSchema::did.eq(ActorSchema::did)))
            .select((
                ActorSchema::did,
                RepoRootSchema::cid,
                RepoRootSchema::rev,
                ActorSchema::createdAt,
                ActorSchema::deactivatedAt,
                ActorSchema::takedownRef,
            ))
            .limit(limit)
            .into_boxed();

        if direction == "desc" {
            builder = builder.order((ActorSchema::createdAt.desc(), ActorSchema::did.desc()));
        } else {
            builder = builder.order((ActorSchema::createdAt.asc(), ActorSchema::did.asc()));
        }

        if let Some(labeled) = labeled {
            if direction == "asc" {
                builder = builder.filter(
                    sql::<Bool>("((")
                        .bind(ActorSchema::createdAt)
                        .sql(", ")
                        .bind(ActorSchema::did)
                        .sql(") > (")
                        .bind::<Text, _>(labeled.primary)
                        .sql(", ")
                        .bind::<Text, _>(labeled.secondary)
                        .sql("))"),
                );
            } else {
                builder = builder.filter(
                    sql::<Bool>("((")
                        .bind(ActorSchema::createdAt)
                        .sql(", ")
                        .bind(ActorSchema::did)
                        .sql(") < (")
                        .bind::<Text, _>(labeled.primary)
                        .sql(", ")
                        .bind::<Text, _>(labeled.secondary)
                        .sql("))"),
                );
            }
        }

        let res = builder.load::<(
            String,
            String,
            String,
            String,
            Option<String>,
            Option<String>,
        )>(conn)?;
        Ok(res)
    }
}

async fn inner_list_repos(limit: Option<i64>, cursor: Option<String>) -> Result<ListReposOutput> {
    let keyset = TimeDidKeySet::new();
    let result = keyset
        .paginate(KeySetPaginateOpts {
            limit: limit.unwrap_or(500),
            cursor,
            direction: Some("asc".to_string()),
        })
        .await?;
    let time_did_results = result
        .iter()
        .map(|row| TimeDidResult {
            created_at: row.3.clone(),
            did: row.0.clone(),
        })
        .collect::<Vec<TimeDidResult>>();
    let repos = result
        .into_iter()
        .map(|row| {
            let FormattedAccountStatus { active, status } =
                format_account_status(Some(ActorAccount {
                    did: row.0.clone(),
                    handle: None,
                    created_at: row.3,
                    takedown_ref: row.5,
                    deactivated_at: row.4,
                    delete_after: None,
                    email: None,
                    invites_disabled: None,
                    email_confirmed_at: None,
                }));
            LexiconRepo {
                did: row.0,
                head: row.1,
                rev: row.2,
                active: Some(active),
                status: match status {
                    None => None,
                    Some(status) => match status {
                        AccountStatus::Active => None,
                        AccountStatus::Takendown => Some(RepoStatus::Takedown),
                        AccountStatus::Suspended => Some(RepoStatus::Suspended),
                        AccountStatus::Deleted => None,
                        AccountStatus::Deactivated => Some(RepoStatus::Deactivated),
                    },
                },
            }
        })
        .collect::<Vec<LexiconRepo>>();
    Ok(ListReposOutput {
        cursor: keyset.pack_from_result(time_did_results)?,
        repos,
    })
}

#[rocket::get("/xrpc/com.atproto.sync.listRepos?<limit>&<cursor>")]
pub async fn list_repos(
    limit: Option<i64>,
    cursor: Option<String>,
) -> Result<Json<ListReposOutput>, status::Custom<Json<ErrorMessageResponse>>> {
    match inner_list_repos(limit, cursor).await {
        Ok(res) => Ok(Json(res)),
        Err(error) => {
            eprintln!("@LOG: ERROR: {error}");
            let internal_error = ErrorMessageResponse {
                code: Some(ErrorCode::InternalServerError),
                message: Some(error.to_string()),
            };
            return Err(status::Custom(
                Status::InternalServerError,
                Json(internal_error),
            ));
        }
    }
}
