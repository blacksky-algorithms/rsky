use crate::db::establish_connection;
use crate::models::models;
use anyhow::{bail, Result};
use chrono::offset::Utc as UtcOffset;
use chrono::DateTime;
use diesel::*;
use rsky_lexicon::com::atproto::server::AccountCodes;
use std::time::SystemTime;

pub fn ensure_invite_is_available(invite_code: String) -> Result<()> {
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

    let system_time = SystemTime::now();
    let dt: DateTime<UtcOffset> = system_time.into();
    let created_at = format!("{}", dt.format("%Y-%m-%dT%H:%M:%S%.3fZ"));

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
        .values(rows)
        .execute(conn)?;
    Ok(())
}
