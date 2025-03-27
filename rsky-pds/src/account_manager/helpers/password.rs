use crate::db::DbConn;
use crate::models;
use crate::models::AppPassword;
use anyhow::{anyhow, bail, Result};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use base64ct::{Base64, Encoding};
use diesel::*;
use rsky_common::{get_random_str, now};
use rsky_lexicon::com::atproto::server::CreateAppPasswordOutput;
use sha2::{Digest, Sha256};

pub struct UpdateUserPasswordOpts {
    pub did: String,
    pub password_encrypted: String,
}

pub async fn verify_account_password(did: &str, password: &String, db: &DbConn) -> Result<bool> {
    use crate::schema::pds::account::dsl as AccountSchema;

    let did = did.to_owned();
    let found = db
        .run(move |conn| {
            AccountSchema::account
                .filter(AccountSchema::did.eq(did))
                .select(models::Account::as_select())
                .first(conn)
                .optional()
        })
        .await?;
    if let Some(found) = found {
        verify(password, &found.password)
    } else {
        Ok(false)
    }
}

pub async fn verify_app_password(did: &str, password: &str, db: &DbConn) -> Result<Option<String>> {
    use crate::schema::pds::app_password::dsl as AppPasswordSchema;

    let did = did.to_owned();
    let password = password.to_owned();
    let password_encrypted = hash_app_password(&did, &password).await?;
    let found = db
        .run(move |conn| {
            AppPasswordSchema::app_password
                .filter(AppPasswordSchema::did.eq(did))
                .filter(AppPasswordSchema::password.eq(password_encrypted))
                .select(AppPassword::as_select())
                .first(conn)
                .optional()
        })
        .await?;
    if let Some(found) = found {
        Ok(Some(found.name))
    } else {
        Ok(None)
    }
}

// We use Argon because it's 3x faster than scrypt.
pub fn gen_salt_and_hash(password: String) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    // Hash password to PHC string
    let argon2 = Argon2::default();
    let password_hash = argon2
        .hash_password(password.as_ref(), &salt)
        .map_err(|error| anyhow!(error.to_string()))?
        .to_string();
    Ok(password_hash)
}

pub fn hash_with_salt(password: &String, salt: &str) -> Result<String> {
    let salt = SaltString::from_b64(salt).map_err(|error| anyhow!(error.to_string()))?;
    let argon2 = Argon2::default();
    let password_hash = argon2
        .hash_password(password.as_ref(), &salt)
        .map_err(|error| anyhow!(error.to_string()))?
        .to_string();
    Ok(password_hash)
}

pub fn verify(password: &String, stored_hash: &str) -> Result<bool> {
    let parsed_hash = PasswordHash::new(stored_hash).map_err(|error| anyhow!(error.to_string()))?;
    Ok(Argon2::default()
        .verify_password(password.as_ref(), &parsed_hash)
        .is_ok())
}

pub async fn hash_app_password(did: &String, password: &String) -> Result<String> {
    let hash = Sha256::digest(did);
    let salt = Base64::encode_string(&hash).replace("=", "");
    hash_with_salt(password, &salt)
}

/// create an app password with format:
/// 1234-abcd-5678-efgh
pub async fn create_app_password(
    did: String,
    name: String,
    db: &DbConn,
) -> Result<CreateAppPasswordOutput> {
    let str = &get_random_str()[0..16].to_lowercase();
    let chunks = [&str[0..4], &str[4..8], &str[8..12], &str[12..16]];
    let password = chunks.join("-");
    let password_encrypted = hash_app_password(&did, &password).await?;

    use crate::schema::pds::app_password::dsl as AppPasswordSchema;

    let created_at = now();

    db.run(move |conn| {
        let got: Option<AppPassword> = insert_into(AppPasswordSchema::app_password)
            .values((
                AppPasswordSchema::did.eq(did),
                AppPasswordSchema::name.eq(&name),
                AppPasswordSchema::password.eq(password_encrypted),
                AppPasswordSchema::createdAt.eq(&created_at),
            ))
            .returning(AppPassword::as_select())
            .get_result(conn)
            .optional()?;
        if got.is_some() {
            Ok(CreateAppPasswordOutput {
                name,
                password,
                created_at,
            })
        } else {
            bail!("could not create app-specific password")
        }
    })
    .await
}

pub async fn list_app_passwords(did: &str, db: &DbConn) -> Result<Vec<(String, String)>> {
    use crate::schema::pds::app_password::dsl as AppPasswordSchema;

    let did = did.to_owned();
    db.run(move |conn| {
        Ok(AppPasswordSchema::app_password
            .filter(AppPasswordSchema::did.eq(did))
            .select((AppPasswordSchema::name, AppPasswordSchema::createdAt))
            .get_results(conn)?)
    })
    .await
}

pub async fn update_user_password(opts: UpdateUserPasswordOpts, db: &DbConn) -> Result<()> {
    use crate::schema::pds::account::dsl as AccountSchema;

    db.run(move |conn| {
        update(AccountSchema::account)
            .filter(AccountSchema::did.eq(opts.did))
            .set(AccountSchema::password.eq(opts.password_encrypted))
            .execute(conn)?;
        Ok(())
    })
    .await
}

pub async fn delete_app_password(did: &str, name: &str, db: &DbConn) -> Result<()> {
    use crate::schema::pds::app_password::dsl as AppPasswordSchema;

    let did = did.to_owned();
    let name = name.to_owned();
    db.run(move |conn| {
        delete(AppPasswordSchema::app_password)
            .filter(AppPasswordSchema::did.eq(did))
            .filter(AppPasswordSchema::name.eq(name))
            .execute(conn)?;
        Ok(())
    })
    .await
}
