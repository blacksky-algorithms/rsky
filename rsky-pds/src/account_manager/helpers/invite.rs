use crate::account_manager::DisableInviteCodesOpts;
use crate::db::sqlite::Db;
use crate::models::models;
use anyhow::{bail, Result};
use rsky_lexicon::com::atproto::server::AccountCodes;
use rsky_lexicon::com::atproto::server::{
    InviteCode as LexiconInviteCode, InviteCodeUse as LexiconInviteCodeUse,
};
use rusqlite::{params, params_from_iter, OptionalExtension, Row};
use std::collections::BTreeMap;
use std::mem;

pub type CodeUse = LexiconInviteCodeUse;
pub type CodeDetail = LexiconInviteCode;

pub(crate) fn invite_code_from_row(row: &Row) -> Result<models::InviteCode, rusqlite::Error> {
    Ok(models::InviteCode {
        code: row.get(0)?,
        available_uses: row.get(1)?,
        disabled: row.get(2)?,
        for_account: row.get(3)?,
        created_by: row.get(4)?,
        created_at: row.get(5)?,
    })
}

pub(crate) const SELECT_INVITE_CODE: &str = "\
    SELECT invite_code.code, invite_code.\"availableUses\", invite_code.disabled, \
    invite_code.\"forAccount\", invite_code.\"createdBy\", invite_code.\"createdAt\" \
    FROM invite_code";

fn placeholders(len: usize) -> String {
    vec!["?"; len].join(", ")
}

pub async fn ensure_invite_is_available(invite_code: String, db: &Db) -> Result<()> {
    db.run(move |conn| {
        let invite: Option<models::InviteCode> = conn
            .query_row(
                &format!(
                    "{SELECT_INVITE_CODE} \
                     LEFT JOIN actor ON invite_code.\"forAccount\" = actor.did \
                         AND actor.\"takedownRef\" IS NULL \
                     WHERE code = ?1"
                ),
                params![invite_code],
                invite_code_from_row,
            )
            .optional()?;

        let Some(invite) = invite else {
            bail!("InvalidInviteCode: None or disabled. Provided invite code not available `{invite_code:?}`")
        };
        if invite.disabled > 0 {
            bail!("InvalidInviteCode: None or disabled. Provided invite code not available `{invite_code:?}`")
        }

        let uses: i64 = conn.query_row(
            "SELECT count(*) FROM invite_code_use WHERE code = ?1",
            params![invite_code],
            |row| row.get(0),
        )?;

        if invite.available_uses as i64 <= uses {
            bail!("InvalidInviteCode: Not enough uses. Provided invite code not available `{invite_code:?}`")
        }
        Ok(())
    })
    .await
}

pub async fn record_invite_use(
    did: String,
    invite_code: Option<String>,
    now: String,
    db: &Db,
) -> Result<()> {
    if let Some(invite_code) = invite_code {
        db.run(move |conn| {
            conn.execute(
                "INSERT INTO invite_code_use (code, \"usedBy\", \"usedAt\") VALUES (?1, ?2, ?3)",
                params![invite_code, did, now],
            )?;
            Ok(())
        })
        .await?;
    }
    Ok(())
}

fn insert_invite_codes(
    conn: &rusqlite::Connection,
    rows: &[models::InviteCode],
) -> Result<(), rusqlite::Error> {
    let mut stmt = conn.prepare(
        "INSERT INTO invite_code \
         (code, \"availableUses\", disabled, \"forAccount\", \"createdBy\", \"createdAt\") \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;
    for row in rows {
        stmt.execute(params![
            row.code,
            row.available_uses,
            row.disabled,
            row.for_account,
            row.created_by,
            row.created_at,
        ])?;
    }
    Ok(())
}

pub async fn create_invite_codes(
    to_create: Vec<AccountCodes>,
    use_count: i32,
    db: &Db,
) -> Result<()> {
    let created_at = rsky_common::now();

    db.tx(move |tx| {
        let rows: Vec<models::InviteCode> = to_create
            .iter()
            .flat_map(|account| {
                let for_account = &account.account;
                account
                    .codes
                    .iter()
                    .map(|code| models::InviteCode {
                        code: code.clone(),
                        available_uses: use_count,
                        disabled: 0,
                        for_account: for_account.clone(),
                        created_by: "admin".to_owned(),
                        created_at: created_at.clone(),
                    })
                    .collect::<Vec<models::InviteCode>>()
            })
            .collect();
        insert_invite_codes(tx, &rows)?;
        Ok(())
    })
    .await
}

pub async fn create_account_invite_codes(
    for_account: &str,
    codes: Vec<String>,
    expected_total: usize,
    disabled: bool,
    db: &Db,
) -> Result<Vec<CodeDetail>> {
    let for_account = for_account.to_owned();
    let rows = db
        .tx(move |tx| {
            let now = rsky_common::now();

            let rows: Vec<models::InviteCode> = codes
                .iter()
                .map(|code| models::InviteCode {
                    code: code.clone(),
                    available_uses: 1,
                    disabled: if disabled { 1 } else { 0 },
                    for_account: for_account.clone(),
                    created_by: for_account.clone(),
                    created_at: now.clone(),
                })
                .collect();

            insert_invite_codes(tx, &rows)?;

            // don't count admin-gifted codes against the user
            let final_routine_invite_codes: i64 = tx.query_row(
                "SELECT count(*) FROM invite_code \
                 WHERE \"forAccount\" = ?1 AND \"createdBy\" != 'admin'",
                params![for_account],
                |row| row.get(0),
            )?;

            if final_routine_invite_codes as usize > expected_total {
                bail!("DuplicateCreate: attempted to create additional codes in another request")
            }

            Ok(rows)
        })
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| CodeDetail {
            code: row.code,
            available: 1,
            disabled: row.disabled == 1,
            for_account: row.for_account,
            created_by: row.created_by,
            created_at: row.created_at,
            uses: Vec::new(),
        })
        .collect())
}

pub async fn get_account_invite_codes(did: &str, db: &Db) -> Result<Vec<CodeDetail>> {
    let did = did.to_owned();
    let res: Vec<models::InviteCode> = db
        .run(move |conn| {
            let mut stmt =
                conn.prepare(&format!("{SELECT_INVITE_CODE} WHERE \"forAccount\" = ?1"))?;
            let rows = stmt
                .query_map(params![did], invite_code_from_row)?
                .collect::<Result<Vec<models::InviteCode>, rusqlite::Error>>()?;
            Ok(rows)
        })
        .await?;

    let codes: Vec<String> = res.iter().map(|row| row.code.clone()).collect();
    let mut uses = get_invite_codes_uses_v2(codes, db).await?;
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

pub async fn get_invite_codes_uses_v2(
    codes: Vec<String>,
    db: &Db,
) -> Result<BTreeMap<String, Vec<CodeUse>>> {
    let mut uses: BTreeMap<String, Vec<CodeUse>> = BTreeMap::new();
    if !codes.is_empty() {
        let uses_res: Vec<models::InviteCodeUse> = db
            .run(move |conn| {
                let mut stmt = conn.prepare(&format!(
                    "SELECT code, \"usedBy\", \"usedAt\" FROM invite_code_use \
                     WHERE code IN ({}) ORDER BY \"usedAt\" DESC",
                    placeholders(codes.len())
                ))?;
                let rows = stmt
                    .query_map(params_from_iter(codes.iter()), |row| {
                        Ok(models::InviteCodeUse {
                            code: row.get(0)?,
                            used_by: row.get(1)?,
                            used_at: row.get(2)?,
                        })
                    })?
                    .collect::<Result<Vec<models::InviteCodeUse>, rusqlite::Error>>()?;
                Ok(rows)
            })
            .await?;
        for invite_code_use in uses_res {
            let models::InviteCodeUse {
                code,
                used_by,
                used_at,
            } = invite_code_use;
            match uses.get_mut(&code) {
                None => {
                    uses.insert(code, vec![CodeUse { used_by, used_at }]);
                }
                Some(matched_uses) => matched_uses.push(CodeUse { used_by, used_at }),
            };
        }
    }
    Ok(uses)
}

pub async fn get_invited_by_for_accounts(
    dids: Vec<String>,
    db: &Db,
) -> Result<BTreeMap<String, CodeDetail>> {
    if dids.is_empty() {
        return Ok(BTreeMap::new());
    }

    let query_dids = dids.clone();
    let res: Vec<models::InviteCode> = db
        .run(move |conn| {
            let mut stmt = conn.prepare(&format!(
                "{SELECT_INVITE_CODE} WHERE invite_code.code IN (\
                 SELECT DISTINCT code FROM invite_code_use WHERE \"usedBy\" IN ({}))",
                placeholders(query_dids.len())
            ))?;
            let rows = stmt
                .query_map(params_from_iter(query_dids.iter()), invite_code_from_row)?
                .collect::<Result<Vec<models::InviteCode>, rusqlite::Error>>()?;
            Ok(rows)
        })
        .await?;
    let codes: Vec<String> = res.iter().map(|row| row.code.clone()).collect();
    let mut uses = get_invite_codes_uses_v2(codes, db).await?;

    let code_details = res
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
        .collect::<Vec<CodeDetail>>();

    Ok(code_details.iter().fold(
        BTreeMap::new(),
        |mut acc: BTreeMap<String, CodeDetail>, cur| {
            for code_use in &cur.uses {
                acc.insert(code_use.used_by.clone(), cur.clone());
            }
            acc
        },
    ))
}

pub async fn set_account_invites_disabled(did: &str, disabled: bool, db: &Db) -> Result<()> {
    let disabled: i16 = if disabled { 1 } else { 0 };
    let did = did.to_owned();
    db.run(move |conn| {
        conn.execute(
            "UPDATE account SET \"invitesDisabled\" = ?1 WHERE did = ?2",
            params![disabled, did],
        )?;
        Ok(())
    })
    .await
}

pub async fn disable_invite_codes(opts: DisableInviteCodesOpts, db: &Db) -> Result<()> {
    let DisableInviteCodesOpts { codes, accounts } = opts;
    if !codes.is_empty() {
        db.run(move |conn| {
            conn.execute(
                &format!(
                    "UPDATE invite_code SET disabled = 1 WHERE code IN ({})",
                    placeholders(codes.len())
                ),
                params_from_iter(codes.iter()),
            )?;
            Ok(())
        })
        .await?;
    }
    if !accounts.is_empty() {
        db.run(move |conn| {
            conn.execute(
                &format!(
                    "UPDATE invite_code SET disabled = 1 WHERE \"forAccount\" IN ({})",
                    placeholders(accounts.len())
                ),
                params_from_iter(accounts.iter()),
            )?;
            Ok(())
        })
        .await?;
    }
    Ok(())
}
