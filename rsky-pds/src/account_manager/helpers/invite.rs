use crate::account_manager::DisableInviteCodesOpts;
use crate::common;
use crate::db::establish_connection;
use crate::models::models;
use anyhow::{bail, Result};
use diesel::*;
use rsky_lexicon::com::atproto::server::AccountCodes;
use rsky_lexicon::com::atproto::server::{
    InviteCode as LexiconInviteCode, InviteCodeUse as LexiconInviteCodeUse,
};
use std::collections::BTreeMap;
use std::mem;

pub type CodeUse = LexiconInviteCodeUse;
pub type CodeDetail = LexiconInviteCode;

pub async fn ensure_invite_is_available(invite_code: String) -> Result<()> {
    use crate::schema::pds::actor::dsl as ActorSchema;
    use crate::schema::pds::invite_code::dsl as InviteCodeSchema;
    use crate::schema::pds::invite_code_use::dsl as InviteCodeUseSchema;

    let conn = &mut establish_connection()?;

    let invite: Option<models::InviteCode> = InviteCodeSchema::invite_code
        .left_join(
            ActorSchema::actor.on(InviteCodeSchema::forAccount
                .eq(ActorSchema::did)
                .and(ActorSchema::takedownRef.is_null())),
        )
        .filter(InviteCodeSchema::code.eq(&invite_code))
        .select(models::InviteCode::as_select())
        .first(conn)
        .optional()?;

    if invite.is_none() || invite.clone().unwrap().disabled > 0 {
        bail!("InvalidInviteCode: None or disabled. Provided invite code not available `{invite_code:?}`")
    }

    let uses: i64 = InviteCodeUseSchema::invite_code_use
        .count()
        .filter(InviteCodeUseSchema::code.eq(&invite_code))
        .first(conn)?;

    if invite.unwrap().available_uses as i64 <= uses {
        bail!("InvalidInviteCode: Not enough uses. Provided invite code not available `{invite_code:?}`")
    }
    Ok(())
}

pub fn record_invite_use(did: String, invite_code: Option<String>, now: String) -> Result<()> {
    if let Some(invite_code) = invite_code {
        use crate::schema::pds::invite_code_use::dsl as InviteCodeUseSchema;
        let conn = &mut establish_connection()?;

        insert_into(InviteCodeUseSchema::invite_code_use)
            .values((
                InviteCodeUseSchema::code.eq(invite_code),
                InviteCodeUseSchema::usedBy.eq(did),
                InviteCodeUseSchema::usedAt.eq(now),
            ))
            .execute(conn)?;
    }
    Ok(())
}

pub async fn create_invite_codes(to_create: Vec<AccountCodes>, use_count: i32) -> Result<()> {
    use crate::schema::pds::invite_code::dsl as InviteCodeSchema;
    let conn = &mut establish_connection()?;

    let created_at = common::now();

    let rows: Vec<models::InviteCode> = to_create
        .into_iter()
        .flat_map(|account| {
            let for_account = account.account;
            account
                .codes
                .iter()
                .map(|code| models::InviteCode {
                    code: code.clone(),
                    available_uses: use_count.clone(),
                    disabled: 0,
                    for_account: for_account.clone(),
                    created_by: "admin".to_owned(),
                    created_at: created_at.clone(),
                })
                .collect::<Vec<models::InviteCode>>()
        })
        .collect();
    insert_into(InviteCodeSchema::invite_code)
        .values(&rows)
        .execute(conn)?;
    Ok(())
}

pub async fn create_account_invite_codes(
    for_account: &String,
    codes: Vec<String>,
    expected_total: usize,
    disabled: bool,
) -> Result<Vec<CodeDetail>> {
    use crate::schema::pds::invite_code::dsl as InviteCodeSchema;
    let conn = &mut establish_connection()?;

    let now = common::now();

    let rows: Vec<models::InviteCode> = codes
        .into_iter()
        .map(|code| models::InviteCode {
            code,
            available_uses: 1,
            disabled: if disabled { 1 } else { 0 },
            for_account: for_account.clone(),
            created_by: for_account.clone(),
            created_at: now.clone(),
        })
        .collect();

    insert_into(InviteCodeSchema::invite_code)
        .values(&rows)
        .execute(conn)?;

    let final_routine_invite_codes: Vec<models::InviteCode> = InviteCodeSchema::invite_code
        .filter(InviteCodeSchema::forAccount.eq(for_account))
        .filter(InviteCodeSchema::createdBy.ne("admin")) // don't count admin-gifted codes against the user
        .select(models::InviteCode::as_select())
        .get_results(conn)?;
    if final_routine_invite_codes.len() > expected_total {
        bail!("DuplicateCreate: attempted to create additional codes in another request")
    }

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

pub async fn get_account_invite_codes(did: &String) -> Result<Vec<CodeDetail>> {
    use crate::schema::pds::invite_code::dsl as InviteCodeSchema;
    let conn = &mut establish_connection()?;

    let res: Vec<models::InviteCode> = InviteCodeSchema::invite_code
        .filter(InviteCodeSchema::forAccount.eq(did))
        .select(models::InviteCode::as_select())
        .get_results(conn)?;
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

pub async fn get_invite_codes_uses(codes: Vec<String>) -> Result<BTreeMap<String, Vec<CodeUse>>> {
    use crate::schema::pds::invite_code_use::dsl as InviteCodeUseSchema;
    let conn = &mut establish_connection()?;

    let mut uses: BTreeMap<String, Vec<CodeUse>> = BTreeMap::new();
    if codes.len() > 0 {
        let uses_res: Vec<models::InviteCodeUse> = InviteCodeUseSchema::invite_code_use
            .filter(InviteCodeUseSchema::code.eq_any(codes))
            .order_by(InviteCodeUseSchema::usedAt.desc())
            .select(models::InviteCodeUse::as_select())
            .get_results(conn)?;
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
    dids: Vec<&String>,
) -> Result<BTreeMap<String, CodeDetail>> {
    if dids.len() < 1 {
        return Ok(BTreeMap::new());
    }
    use crate::schema::pds::invite_code::dsl as InviteCodeSchema;
    use crate::schema::pds::invite_code_use::dsl as InviteCodeUseSchema;
    let conn = &mut establish_connection()?;

    let res: Vec<models::InviteCode> = InviteCodeSchema::invite_code
        .filter(
            InviteCodeSchema::forAccount.eq_any(
                InviteCodeUseSchema::invite_code_use
                    .filter(InviteCodeUseSchema::usedBy.eq_any(dids))
                    .select(InviteCodeUseSchema::code)
                    .distinct(),
            ),
        )
        .select(models::InviteCode::as_select())
        .get_results(conn)?;
    let codes: Vec<String> = res.iter().map(|row| row.code.clone()).collect();
    let mut uses = get_invite_codes_uses(codes).await?;

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

pub async fn set_account_invites_disabled(did: &String, disabled: bool) -> Result<()> {
    use crate::schema::pds::account::dsl as AccountSchema;
    let conn = &mut establish_connection()?;

    let disabled: i16 = if disabled { 1 } else { 0 };
    update(AccountSchema::account)
        .filter(AccountSchema::did.eq(did))
        .set((AccountSchema::invitesDisabled.eq(disabled),))
        .execute(conn)?;
    Ok(())
}

pub async fn disable_invite_codes(opts: DisableInviteCodesOpts) -> Result<()> {
    use crate::schema::pds::invite_code::dsl as InviteCodeSchema;
    let conn = &mut establish_connection()?;

    let DisableInviteCodesOpts { codes, accounts } = opts;
    if codes.len() > 0 {
        update(InviteCodeSchema::invite_code)
            .filter(InviteCodeSchema::code.eq_any(&codes))
            .set((InviteCodeSchema::disabled.eq(1),))
            .execute(conn)?;
    }
    if accounts.len() > 0 {
        update(InviteCodeSchema::invite_code)
            .filter(InviteCodeSchema::forAccount.eq_any(&accounts))
            .set((InviteCodeSchema::disabled.eq(1),))
            .execute(conn)?;
    }
    Ok(())
}
