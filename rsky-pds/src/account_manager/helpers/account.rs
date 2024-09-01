use crate::common;
use crate::common::RFC3339_VARIANT;
use crate::db::establish_connection;
use crate::schema::pds::account::dsl as AccountSchema;
use crate::schema::pds::account::table as AccountTable;
use crate::schema::pds::actor::dsl as ActorSchema;
use crate::schema::pds::actor::table as ActorTable;
use anyhow::Result;
use chrono::offset::Utc as UtcOffset;
use chrono::DateTime;
use diesel::dsl::{exists, not, LeftJoinOn};
use diesel::helper_types::{Eq, IntoBoxed};
use diesel::pg::Pg;
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use diesel::*;
use rsky_lexicon::com::atproto::admin::StatusAttr;
use std::ops::Add;
use std::time::SystemTime;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AccountHelperError {
    #[error("UserAlreadyExistsError")]
    UserAlreadyExistsError,
    #[error("DatabaseError: `{0}`")]
    DieselError(String),
}

pub struct AvailabilityFlags {
    pub include_taken_down: Option<bool>,
    pub include_deactivated: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub enum AccountStatus {
    Active,
    Takendown,
    Suspended,
    Deleted,
    Deactivated,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct FormattedAccountStatus {
    pub active: bool,
    pub status: Option<AccountStatus>,
}

#[derive(Debug)]
pub struct GetAccountAdminStatusOutput {
    pub takedown: StatusAttr,
    pub deactivated: StatusAttr,
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
    let created_at = format!("{}", dt.format(RFC3339_VARIANT));
    let deactivate_at = match deactivated {
        Some(true) => Some(created_at.clone()),
        _ => None,
    };
    let deactivate_after = match deactivated {
        Some(true) => {
            let exp = dt.add(chrono::Duration::days(3));
            Some(format!("{}", exp.format(RFC3339_VARIANT)))
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

    let created_at = common::now();

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

pub async fn delete_account(did: &String) -> Result<()> {
    use crate::schema::pds::email_token::dsl as EmailTokenSchema;
    use crate::schema::pds::refresh_token::dsl as RefreshTokenSchema;
    use crate::schema::pds::repo_root::dsl as RepoRootSchema;

    let conn = &mut establish_connection()?;
    delete(RepoRootSchema::repo_root)
        .filter(RepoRootSchema::did.eq(did))
        .execute(conn)?;
    delete(EmailTokenSchema::email_token)
        .filter(EmailTokenSchema::did.eq(did))
        .execute(conn)?;
    delete(RefreshTokenSchema::refresh_token)
        .filter(RefreshTokenSchema::did.eq(did))
        .execute(conn)?;
    delete(AccountSchema::account)
        .filter(AccountSchema::did.eq(did))
        .execute(conn)?;
    delete(ActorSchema::actor)
        .filter(ActorSchema::did.eq(did))
        .execute(conn)?;
    Ok(())
}

pub async fn update_account_takedown_status(did: &String, takedown: StatusAttr) -> Result<()> {
    let conn = &mut establish_connection()?;
    let takedown_ref: Option<String> = match takedown.applied {
        true => match takedown.r#ref {
            Some(takedown_ref) => Some(takedown_ref),
            None => Some(common::now()),
        },
        false => None,
    };
    update(ActorSchema::actor)
        .filter(ActorSchema::did.eq(did))
        .set((ActorSchema::takedownRef.eq(takedown_ref),))
        .execute(conn)?;
    Ok(())
}

pub async fn deactivate_account(did: &String, delete_after: Option<String>) -> Result<()> {
    let conn = &mut establish_connection()?;

    update(ActorSchema::actor)
        .filter(ActorSchema::did.eq(did))
        .set((
            ActorSchema::deactivatedAt.eq(common::now()),
            ActorSchema::deleteAfter.eq(delete_after),
        ))
        .execute(conn)?;
    Ok(())
}

pub async fn activate_account(did: &String) -> Result<()> {
    let conn = &mut establish_connection()?;

    update(ActorSchema::actor)
        .filter(ActorSchema::did.eq(did))
        .set((
            ActorSchema::deactivatedAt.eq::<Option<String>>(None),
            ActorSchema::deleteAfter.eq::<Option<String>>(None),
        ))
        .execute(conn)?;
    Ok(())
}

pub async fn update_email(did: &String, email: &String) -> Result<()> {
    let conn = &mut establish_connection()?;

    let res = update(AccountSchema::account)
        .filter(AccountSchema::did.eq(did))
        .set((
            AccountSchema::email.eq(email.to_lowercase()),
            AccountSchema::emailConfirmedAt.eq::<Option<String>>(None),
        ))
        .execute(conn);

    match res {
        Ok(_) => Ok(()),
        Err(DieselError::DatabaseError(kind, _)) => match kind {
            DatabaseErrorKind::UniqueViolation => Err(anyhow::Error::new(
                AccountHelperError::UserAlreadyExistsError,
            )),
            _ => Err(anyhow::Error::new(AccountHelperError::DieselError(
                format!("{:?}", kind),
            ))),
        },
        Err(e) => Err(anyhow::Error::new(e)),
    }
}

pub async fn update_handle(did: &String, handle: &String) -> Result<()> {
    let conn = &mut establish_connection()?;
    use crate::schema::pds::actor;

    let actor2 = diesel::alias!(actor as actor2);

    let res = update(ActorSchema::actor)
        .filter(ActorSchema::did.eq(did))
        .filter(not(exists(actor2.filter(ActorSchema::handle.eq(handle)))))
        .set((ActorSchema::handle.eq(handle),))
        .execute(conn)?;

    if res < 1 {
        return Err(anyhow::Error::new(
            AccountHelperError::UserAlreadyExistsError,
        ));
    }
    Ok(())
}

pub async fn set_email_confirmed_at(did: &String, email_confirmed_at: String) -> Result<()> {
    let conn = &mut establish_connection()?;

    update(AccountSchema::account)
        .filter(AccountSchema::did.eq(did))
        .set(AccountSchema::emailConfirmedAt.eq(email_confirmed_at))
        .execute(conn)?;
    Ok(())
}

pub async fn get_account_admin_status(did: &String) -> Result<Option<GetAccountAdminStatusOutput>> {
    let conn = &mut establish_connection()?;

    let res: Option<(Option<String>, Option<String>)> = ActorSchema::actor
        .filter(ActorSchema::did.eq(did))
        .select((ActorSchema::takedownRef, ActorSchema::deactivatedAt))
        .first(conn)
        .optional()?;
    match res {
        None => Ok(None),
        Some(res) => {
            let takedown = match res.0 {
                Some(takedown_ref) => StatusAttr {
                    applied: true,
                    r#ref: Some(takedown_ref),
                },
                None => StatusAttr {
                    applied: false,
                    r#ref: None,
                },
            };
            let deactivated = match res.1 {
                Some(_) => StatusAttr {
                    applied: true,
                    r#ref: None,
                },
                None => StatusAttr {
                    applied: false,
                    r#ref: None,
                },
            };
            Ok(Some(GetAccountAdminStatusOutput {
                takedown,
                deactivated,
            }))
        }
    }
}

pub fn format_account_status(account: Option<ActorAccount>) -> FormattedAccountStatus {
    match account {
        None => FormattedAccountStatus {
            active: false,
            status: Some(AccountStatus::Deleted),
        },
        Some(got) if got.takedown_ref.is_some() => FormattedAccountStatus {
            active: false,
            status: Some(AccountStatus::Takendown),
        },
        Some(got) if got.deactivated_at.is_some() => FormattedAccountStatus {
            active: false,
            status: Some(AccountStatus::Deactivated),
        },
        _ => FormattedAccountStatus {
            active: true,
            status: None,
        },
    }
}
