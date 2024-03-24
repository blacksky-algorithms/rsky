use crate::db::establish_connection;
use anyhow::Result;
use chrono::offset::Utc as UtcOffset;
use chrono::DateTime;
use diesel::*;
use std::ops::Add;
use std::time::SystemTime;

pub fn register_actor(did: String, handle: String, deactivated: Option<bool>) -> Result<()> {
    use crate::schema::pds::actor::dsl as ActorSchema;
    let conn = &mut establish_connection()?;

    let system_time = SystemTime::now();
    let dt: DateTime<UtcOffset> = system_time.into();
    let created_at = format!("{}", dt.format("%+"));
    let deactivate_at = match deactivated {
        Some(true) => Some(created_at.clone()),
        _ => None,
    };
    let deactivate_after = match deactivated {
        Some(true) => {
            let exp = dt.add(chrono::Duration::days(3));
            Some(format!("{}", exp.format("%+")))
        }
        _ => None,
    };

    let _: String = insert_into(ActorSchema::actor)
        .values((
            ActorSchema::did.eq(did),
            ActorSchema::handle.eq(handle),
            ActorSchema::createdAt.eq(created_at),
            ActorSchema::deactivatedAt.eq(deactivate_at),
            ActorSchema::deleteAfter.eq(deactivate_after),
        ))
        .on_conflict_do_nothing()
        .returning(ActorSchema::did)
        .get_result(conn)?;
    Ok(())
}

pub fn register_account(did: String, email: String, password: String) -> Result<()> {
    use crate::schema::pds::account::dsl as AccountSchema;
    let conn = &mut establish_connection()?;

    let _: String = insert_into(AccountSchema::account)
        .values((
            AccountSchema::did.eq(did),
            AccountSchema::email.eq(email),
            AccountSchema::password.eq(password),
        ))
        .on_conflict_do_nothing()
        .returning(AccountSchema::did)
        .get_result(conn)?;
    Ok(())
}
