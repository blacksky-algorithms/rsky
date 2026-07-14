use crate::account_manager::helpers::invite::{get_invite_codes_uses_v2, CodeDetail};
use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::auth_verifier::Moderator;
use crate::db::pagination::{pack_cursor, unpack_cursor, Cursor, SortDirection, TimeCidKeyset};
use crate::db::sqlite::Db;
use crate::models::models;
use anyhow::{bail, Result};
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::admin::GetInviteCodesOutput;
use rusqlite::types::Value as SqlValue;
use rusqlite::{params_from_iter, Row};
use std::mem;

const SELECT_CODES_WITH_USES: &str = "\
    SELECT * FROM (\
    SELECT code, \"availableUses\", disabled, \"forAccount\", \"createdBy\", \"createdAt\", \
    (SELECT count(*) FROM invite_code_use WHERE invite_code_use.code = invite_code.code) AS uses \
    FROM invite_code) codes";

#[derive(Debug, Clone)]
struct InviteCodeRow {
    code: models::InviteCode,
    uses: i64,
}

fn invite_code_row(row: &Row) -> Result<InviteCodeRow, rusqlite::Error> {
    Ok(InviteCodeRow {
        code: models::InviteCode {
            code: row.get(0)?,
            available_uses: row.get(1)?,
            disabled: row.get(2)?,
            for_account: row.get(3)?,
            created_by: row.get(4)?,
            created_at: row.get(5)?,
        },
        uses: row.get(6)?,
    })
}

async fn query_codes(
    db: &Db,
    where_clause: Option<(String, Vec<SqlValue>)>,
    order_by: String,
    limit: i64,
) -> Result<Vec<InviteCodeRow>> {
    let mut sql = SELECT_CODES_WITH_USES.to_string();
    let mut sql_params: Vec<SqlValue> = Vec::new();
    if let Some((clause, params)) = where_clause {
        sql.push_str(&format!(" WHERE {clause}"));
        sql_params.extend(params);
    }
    sql.push_str(&format!(" ORDER BY {order_by} LIMIT {limit}"));
    db.run(move |conn| {
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(sql_params.iter()), invite_code_row)?
            .collect::<Result<Vec<InviteCodeRow>, rusqlite::Error>>()?;
        Ok(rows)
    })
    .await
}

async fn paginate_by_recent(
    db: &Db,
    limit: i64,
    cursor: Option<String>,
) -> Result<(Vec<InviteCodeRow>, Option<String>)> {
    let keyset = TimeCidKeyset::new("\"createdAt\"", "code");
    let where_clause = keyset.unpack(cursor.as_deref())?.map(|(created_at, code)| {
        (
            keyset.where_clause(SortDirection::Desc),
            vec![SqlValue::Text(created_at), SqlValue::Text(code)],
        )
    });
    let rows = query_codes(
        db,
        where_clause,
        keyset.order_by_clause(SortDirection::Desc),
        limit,
    )
    .await?;
    let cursor_rows = rows
        .iter()
        .map(|row| (row.code.created_at.clone(), row.code.code.clone()))
        .collect::<Vec<(String, String)>>();
    let result_cursor = keyset.pack_from_result(&cursor_rows)?;
    Ok((rows, result_cursor))
}

async fn paginate_by_usage(
    db: &Db,
    limit: i64,
    cursor: Option<String>,
) -> Result<(Vec<InviteCodeRow>, Option<String>)> {
    let where_clause = match unpack_cursor(cursor.as_deref())? {
        None => None,
        Some(cursor) => {
            let Ok(uses) = cursor.primary.parse::<i64>() else {
                bail!("Malformed cursor")
            };
            Some((
                "((uses, code) < (?, ?))".to_string(),
                vec![SqlValue::Integer(uses), SqlValue::Text(cursor.secondary)],
            ))
        }
    };
    let rows = query_codes(db, where_clause, "uses DESC, code DESC".to_string(), limit).await?;
    let result_cursor = pack_cursor(rows.last().map(|row| Cursor {
        primary: row.uses.to_string(),
        secondary: row.code.code.clone(),
    }));
    Ok((rows, result_cursor))
}

async fn inner_get_invite_codes(
    sort: Option<String>,
    limit: Option<i64>,
    cursor: Option<String>,
    db: &Db,
) -> Result<GetInviteCodesOutput> {
    let limit = limit.unwrap_or(100);
    // the lexicon defaults sort to "recent"
    let (rows, result_cursor) = match sort.as_deref() {
        Some("recent") | None => paginate_by_recent(db, limit, cursor).await?,
        Some("usage") => paginate_by_usage(db, limit, cursor).await?,
        _ => bail!("Unknown sort method: {:?}", sort),
    };

    let codes: Vec<String> = rows.iter().map(|row| row.code.code.clone()).collect();
    let mut uses = get_invite_codes_uses_v2(codes, db).await?;
    let codes = rows
        .into_iter()
        .map(|row| CodeDetail {
            code: row.code.code.clone(),
            available: row.code.available_uses,
            disabled: row.code.disabled == 1,
            for_account: row.code.for_account,
            created_by: row.code.created_by,
            created_at: row.code.created_at,
            uses: mem::take(uses.get_mut(&row.code.code).unwrap_or(&mut Vec::new())),
        })
        .collect::<Vec<CodeDetail>>();

    Ok(GetInviteCodesOutput {
        cursor: result_cursor,
        codes,
    })
}

#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.admin.getInviteCodes?<sort>&<limit>&<cursor>")]
pub async fn get_invite_codes(
    sort: Option<String>,
    limit: Option<i64>,
    cursor: Option<String>,
    _auth: Moderator,
    account_manager: AccountManager,
) -> Result<Json<GetInviteCodesOutput>, ApiError> {
    match inner_get_invite_codes(sort, limit, cursor, &account_manager.db).await {
        Ok(res) => Ok(Json(res)),
        Err(error) => {
            tracing::error!("Internal Error: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account_manager::tests::test_manager;
    use rusqlite::params;

    async fn seed_codes(db: &Db) {
        db.run(|conn| {
            let mut insert_code = conn.prepare(
                "INSERT INTO invite_code \
                 (code, \"availableUses\", disabled, \"forAccount\", \"createdBy\", \"createdAt\") \
                 VALUES (?1, 1, 0, 'did:plc:admin', 'admin', ?2)",
            )?;
            insert_code.execute(params!["code-a", "2023-01-01T00:00:00.000Z"])?;
            insert_code.execute(params!["code-b", "2023-01-02T00:00:00.000Z"])?;
            insert_code.execute(params!["code-c", "2023-01-03T00:00:00.000Z"])?;
            let mut insert_use = conn.prepare(
                "INSERT INTO invite_code_use (code, \"usedBy\", \"usedAt\") VALUES (?1, ?2, ?3)",
            )?;
            insert_use.execute(params!["code-a", "did:plc:u1", "2023-02-01T00:00:00.000Z"])?;
            insert_use.execute(params!["code-a", "did:plc:u2", "2023-02-02T00:00:00.000Z"])?;
            insert_use.execute(params!["code-b", "did:plc:u3", "2023-02-03T00:00:00.000Z"])?;
            Ok(())
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn paginates_by_recent() {
        let (_dir, am) = test_manager().await;
        seed_codes(&am.db).await;

        // sort defaults to recent
        let page1 = inner_get_invite_codes(None, Some(2), None, &am.db)
            .await
            .unwrap();
        assert_eq!(
            page1
                .codes
                .iter()
                .map(|code| code.code.as_str())
                .collect::<Vec<&str>>(),
            vec!["code-c", "code-b"]
        );
        assert_eq!(page1.codes[1].uses.len(), 1);
        let page2 = inner_get_invite_codes(
            Some("recent".to_owned()),
            Some(2),
            page1.cursor.clone(),
            &am.db,
        )
        .await
        .unwrap();
        assert_eq!(page2.codes.len(), 1);
        assert_eq!(page2.codes[0].code, "code-a");
        assert_eq!(page2.codes[0].uses.len(), 2);

        // empty page yields no cursor
        let page3 = inner_get_invite_codes(
            Some("recent".to_owned()),
            Some(2),
            page2.cursor.clone(),
            &am.db,
        )
        .await
        .unwrap();
        assert!(page3.codes.is_empty());
        assert!(page3.cursor.is_none());

        // malformed cursor
        assert!(inner_get_invite_codes(
            Some("recent".to_owned()),
            Some(2),
            Some("bogus".to_owned()),
            &am.db
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn paginates_by_usage() {
        let (_dir, am) = test_manager().await;
        seed_codes(&am.db).await;

        let page1 = inner_get_invite_codes(Some("usage".to_owned()), Some(2), None, &am.db)
            .await
            .unwrap();
        assert_eq!(
            page1
                .codes
                .iter()
                .map(|code| code.code.as_str())
                .collect::<Vec<&str>>(),
            vec!["code-a", "code-b"]
        );
        let page2 = inner_get_invite_codes(
            Some("usage".to_owned()),
            Some(2),
            page1.cursor.clone(),
            &am.db,
        )
        .await
        .unwrap();
        assert_eq!(page2.codes.len(), 1);
        assert_eq!(page2.codes[0].code, "code-c");
        assert!(page2.codes[0].uses.is_empty());

        // malformed cursors: unparseable primary and missing separator
        assert!(inner_get_invite_codes(
            Some("usage".to_owned()),
            Some(2),
            Some("abc::code".to_owned()),
            &am.db
        )
        .await
        .is_err());
        assert!(inner_get_invite_codes(
            Some("usage".to_owned()),
            Some(2),
            Some("bogus".to_owned()),
            &am.db
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn rejects_unknown_sorts() {
        let (_dir, am) = test_manager().await;
        let err = inner_get_invite_codes(Some("bogus".to_owned()), None, None, &am.db)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Unknown sort method"));
    }
}
