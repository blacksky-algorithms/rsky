use crate::db::establish_connection;
use crate::schema::pds::account::dsl as AccountSchema;
use crate::schema::pds::account::table as AccountTable;
use crate::schema::pds::actor::dsl as ActorSchema;
use crate::schema::pds::actor::table as ActorTable;
use anyhow::Result;
use chrono::offset::Utc as UtcOffset;
use chrono::DateTime;
use diesel::dsl::LeftJoinOn;
use diesel::helper_types::{Eq, IntoBoxed};
use diesel::pg::Pg;
use diesel::*;
use std::ops::Add;
use std::time::SystemTime;

pub struct AvailabilityFlags {
    pub include_taken_down: Option<bool>,
    pub include_deactivated: Option<bool>,
}

pub type ActorJoinAccount =
    LeftJoinOn<ActorTable, AccountTable, Eq<ActorSchema::did, AccountSchema::did>>;
pub type BoxedQuery<'a> = IntoBoxed<'a, ActorJoinAccount, Pg>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActorAccount {
    pub did: String,
    pub handle: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "takedownRef")]
    pub takedown_ref: Option<String>,
    #[serde(rename = "deactivatedAt")]
    pub deactivated_at: Option<String>,
    #[serde(rename = "deleteAfter")]
    pub delete_after: Option<String>,
    pub email: Option<String>,
    #[serde(rename = "invitesDisabled")]
    pub invites_disabled: Option<i16>,
    #[serde(rename = "emailConfirmedAt")]
    pub email_confirmed_at: Option<String>,
}

pub fn select_account_qb(flags: Option<AvailabilityFlags>) -> BoxedQuery<'static> {
    let AvailabilityFlags {
        include_taken_down,
        include_deactivated,
    } = flags.unwrap_or_else(|| AvailabilityFlags {
        include_taken_down: Some(false),
        include_deactivated: Some(false),
    });
    let include_taken_down = include_taken_down.unwrap_or_else(|| false);
    let include_deactivated = include_deactivated.unwrap_or_else(|| false);

    let mut builder = ActorSchema::actor
        .left_join(AccountSchema::account.on(ActorSchema::did.eq(AccountSchema::did)))
        .into_boxed();
    if !include_taken_down {
        builder = builder.filter(ActorSchema::takedownRef.is_null());
    }
    if !include_deactivated {
        builder = builder.filter(ActorSchema::deactivatedAt.is_null());
    }
    builder
}

pub async fn get_account(
    handle_or_did: &String,
    flags: Option<AvailabilityFlags>,
) -> Result<Option<ActorAccount>> {
    let conn = &mut establish_connection()?;
    let mut builder = select_account_qb(flags);
    if handle_or_did.starts_with("did:") {
        builder = builder.filter(ActorSchema::did.eq(handle_or_did));
    } else {
        builder = builder.filter(ActorSchema::handle.eq(handle_or_did));
    }
    let found = builder
        .select((
            ActorSchema::did,
            ActorSchema::handle,
            ActorSchema::createdAt,
            ActorSchema::takedownRef,
            ActorSchema::deactivatedAt,
            ActorSchema::deleteAfter,
            AccountSchema::email.nullable(),
            AccountSchema::emailConfirmedAt.nullable(),
            AccountSchema::invitesDisabled.nullable(),
        ))
        .first::<(
            String,
            Option<String>,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<i16>,
        )>(conn)
        .map(|res| ActorAccount {
            did: res.0,
            handle: res.1,
            created_at: res.2,
            takedown_ref: res.3,
            deactivated_at: res.4,
            delete_after: res.5,
            email: res.6,
            email_confirmed_at: res.7,
            invites_disabled: res.8,
        })
        .optional()?;
    Ok(found)
}

pub async fn get_account_by_email(
    email: &String,
    flags: Option<AvailabilityFlags>,
) -> Result<Option<ActorAccount>> {
    let conn = &mut establish_connection()?;

    let found = select_account_qb(flags)
        .select((
            ActorSchema::did,
            ActorSchema::handle,
            ActorSchema::createdAt,
            ActorSchema::takedownRef,
            ActorSchema::deactivatedAt,
            ActorSchema::deleteAfter,
            AccountSchema::email.nullable(),
            AccountSchema::emailConfirmedAt.nullable(),
            AccountSchema::invitesDisabled.nullable(),
        ))
        .filter(AccountSchema::email.eq(email.to_lowercase()))
        .first::<(
            String,
            Option<String>,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<i16>,
        )>(conn)
        .map(|res| ActorAccount {
            did: res.0,
            handle: res.1,
            created_at: res.2,
            takedown_ref: res.3,
            deactivated_at: res.4,
            delete_after: res.5,
            email: res.6,
            email_confirmed_at: res.7,
            invites_disabled: res.8,
        })
        .optional()?;
    Ok(found)
}

pub fn register_actor(did: String, handle: String, deactivated: Option<bool>) -> Result<()> {
    let conn = &mut establish_connection()?;

    let system_time = SystemTime::now();
    let dt: DateTime<UtcOffset> = system_time.into();
    let created_at = format!("{}", dt.format("%Y-%m-%dT%H:%M:%S%.3fZ"));
    let deactivate_at = match deactivated {
        Some(true) => Some(created_at.clone()),
        _ => None,
    };
    let deactivate_after = match deactivated {
        Some(true) => {
            let exp = dt.add(chrono::Duration::days(3));
            Some(format!("{}", exp.format("%Y-%m-%dT%H:%M:%S%.3fZ")))
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
    let conn = &mut establish_connection()?;

    let system_time = SystemTime::now();
    let dt: DateTime<UtcOffset> = system_time.into();
    let created_at = format!("{}", dt.format("%Y-%m-%dT%H:%M:%S%.3fZ"));

    // @TODO record recovery key for bring your own recovery key
    let _: String = insert_into(AccountSchema::account)
        .values((
            AccountSchema::did.eq(did),
            AccountSchema::email.eq(email),
            AccountSchema::password.eq(password),
            AccountSchema::createdAt.eq(created_at),
        ))
        .on_conflict_do_nothing()
        .returning(AccountSchema::did)
        .get_result(conn)?;
    Ok(())
}
