use crate::account_manager::helpers::account::{
    format_account_status, AccountStatus, ActorAccount, FormattedAccountStatus,
};
use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::db::pagination::{SortDirection, TimeCidKeyset};
use crate::db::sqlite::Db;
use anyhow::Result;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::sync::{ListReposOutput, RefRepo as LexiconRepo, RepoStatus};
use rusqlite::params_from_iter;
use rusqlite::types::Value as SqlValue;

#[derive(Debug, Clone)]
pub struct RepoRow {
    pub did: String,
    pub cid: String,
    pub rev: String,
    pub created_at: String,
    pub deactivated_at: Option<String>,
    pub takedown_ref: Option<String>,
}

pub async fn paginate_repos(db: &Db, limit: i64, cursor: Option<String>) -> Result<Vec<RepoRow>> {
    let keyset = TimeCidKeyset::new("actor.\"createdAt\"", "actor.did");
    let unpacked = keyset.unpack(cursor.as_deref())?;

    let mut sql = "\
        SELECT actor.did, repo_root.cid, repo_root.rev, actor.\"createdAt\", \
        actor.\"deactivatedAt\", actor.\"takedownRef\" \
        FROM actor JOIN repo_root ON repo_root.did = actor.did"
        .to_string();
    let mut sql_params: Vec<SqlValue> = Vec::new();
    if let Some((created_at, did)) = unpacked {
        sql.push_str(&format!(
            " WHERE {}",
            keyset.where_clause(SortDirection::Asc)
        ));
        sql_params.push(SqlValue::Text(created_at));
        sql_params.push(SqlValue::Text(did));
    }
    sql.push_str(&format!(
        " ORDER BY {} LIMIT {limit}",
        keyset.order_by_clause(SortDirection::Asc)
    ));

    db.run(move |conn| {
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(sql_params.iter()), |row| {
                Ok(RepoRow {
                    did: row.get(0)?,
                    cid: row.get(1)?,
                    rev: row.get(2)?,
                    created_at: row.get(3)?,
                    deactivated_at: row.get(4)?,
                    takedown_ref: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<RepoRow>, rusqlite::Error>>()?;
        Ok(rows)
    })
    .await
}

async fn inner_list_repos(
    limit: Option<i64>,
    cursor: Option<String>,
    db: &Db,
) -> Result<ListReposOutput> {
    let keyset = TimeCidKeyset::new("actor.\"createdAt\"", "actor.did");
    let result = paginate_repos(db, limit.unwrap_or(500), cursor).await?;
    let cursor_rows = result
        .iter()
        .map(|row| (row.created_at.clone(), row.did.clone()))
        .collect::<Vec<(String, String)>>();
    let repos = result
        .into_iter()
        .map(|row| {
            let FormattedAccountStatus { active, status } =
                format_account_status(Some(ActorAccount {
                    did: row.did.clone(),
                    handle: None,
                    created_at: row.created_at,
                    takedown_ref: row.takedown_ref,
                    deactivated_at: row.deactivated_at,
                    delete_after: None,
                    email: None,
                    invites_disabled: None,
                    email_confirmed_at: None,
                }));
            LexiconRepo {
                did: row.did,
                head: row.cid,
                rev: row.rev,
                active: Some(active),
                status: match status {
                    None => None,
                    Some(status) => match status {
                        AccountStatus::Active => None,
                        AccountStatus::Takendown => Some(RepoStatus::Takedown),
                        AccountStatus::Suspended => Some(RepoStatus::Suspended),
                        AccountStatus::Deleted => None,
                        AccountStatus::Deactivated => Some(RepoStatus::Deactivated),
                        AccountStatus::Desynchronized => Some(RepoStatus::Desynchronized),
                        AccountStatus::Throttled => Some(RepoStatus::Throttled),
                    },
                },
            }
        })
        .collect::<Vec<LexiconRepo>>();
    Ok(ListReposOutput {
        cursor: keyset.pack_from_result(&cursor_rows)?,
        repos,
    })
}

#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.sync.listRepos?<limit>&<cursor>")]
pub async fn list_repos(
    limit: Option<i64>,
    cursor: Option<String>,
    account_manager: AccountManager,
) -> Result<Json<ListReposOutput>, ApiError> {
    match inner_list_repos(limit, cursor, &account_manager.db).await {
        Ok(res) => Ok(Json(res)),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::RuntimeError)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account_manager::tests::test_manager;
    use crate::account_manager::{CreateAccountOpts, UpdateEmailOpts};
    use lexicon_cid::Cid;
    use std::str::FromStr;

    const TEST_CID: &str = "bafkreibjfgx2gprinfvicegelk5kosd6y2frmqpqzwqkg7usac74l3t2v4";

    async fn create_repo(am: &crate::account_manager::AccountManager, did: &str, handle: &str) {
        am.create_account(CreateAccountOpts {
            did: did.to_owned(),
            handle: handle.to_owned(),
            email: Some(format!("{handle}@example.com")),
            password: Some("password".to_owned()),
            repo_cid: Cid::from_str(TEST_CID).unwrap(),
            repo_rev: "3jzfcijpj2z2a".to_owned(),
            invite_code: None,
            deactivated: None,
        })
        .await
        .unwrap();
        // suppress unused import lint in cfg(test)
        let _ = UpdateEmailOpts {
            did: did.to_owned(),
            email: format!("{handle}@example.com"),
        };
    }

    #[tokio::test]
    async fn lists_repos_with_pagination_and_status() {
        let (_dir, am) = test_manager().await;
        create_repo(&am, "did:plc:repo1", "repo1.test").await;
        create_repo(&am, "did:plc:repo2", "repo2.test").await;
        create_repo(&am, "did:plc:repo3", "repo3.test").await;
        am.deactivate_account("did:plc:repo2", None).await.unwrap();
        am.takedown_account(
            "did:plc:repo3",
            rsky_lexicon::com::atproto::admin::StatusAttr {
                applied: true,
                r#ref: None,
            },
        )
        .await
        .unwrap();

        let page1 = inner_list_repos(Some(2), None, &am.db).await.unwrap();
        assert_eq!(page1.repos.len(), 2);
        let cursor = page1.cursor.clone().unwrap();
        let page2 = inner_list_repos(Some(2), Some(cursor), &am.db)
            .await
            .unwrap();
        assert_eq!(page2.repos.len(), 1);

        let all = inner_list_repos(None, None, &am.db).await.unwrap();
        assert_eq!(all.repos.len(), 3);
        let by_did = |did: &str| all.repos.iter().find(|repo| repo.did == did).unwrap();
        assert_eq!(by_did("did:plc:repo1").active, Some(true));
        assert!(by_did("did:plc:repo1").status.is_none());
        assert_eq!(by_did("did:plc:repo2").active, Some(false));
        assert!(matches!(
            by_did("did:plc:repo2").status,
            Some(RepoStatus::Deactivated)
        ));
        assert_eq!(by_did("did:plc:repo3").active, Some(false));
        assert!(matches!(
            by_did("did:plc:repo3").status,
            Some(RepoStatus::Takedown)
        ));

        // an empty page yields no cursor
        let empty = inner_list_repos(Some(2), page2.cursor, &am.db)
            .await
            .unwrap();
        assert!(empty.repos.is_empty());
        assert!(empty.cursor.is_none());

        // malformed cursors are rejected
        assert!(inner_list_repos(Some(2), Some("bogus".to_owned()), &am.db)
            .await
            .is_err());
    }
}
