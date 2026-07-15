use crate::db::sqlite::Db;
use anyhow::{anyhow, bail, Result};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use base64ct::{Base64, Encoding};
use rsky_common::{get_random_str, now};
use rsky_lexicon::com::atproto::server::CreateAppPasswordOutput;
use rusqlite::{params, OptionalExtension};
use sha2::{Digest, Sha256};

pub struct UpdateUserPasswordOpts {
    pub did: String,
    pub password_encrypted: String,
}

pub async fn verify_account_password(did: &str, password: &String, db: &Db) -> Result<bool> {
    let did = did.to_owned();
    let found: Option<String> = db
        .run(move |conn| {
            Ok(conn
                .query_row(
                    "SELECT password FROM account WHERE did = ?1",
                    params![did],
                    |row| row.get(0),
                )
                .optional()?)
        })
        .await?;
    if let Some(stored_hash) = found {
        verify(password, &stored_hash)
    } else {
        Ok(false)
    }
}

pub async fn verify_app_password(did: &str, password: &str, db: &Db) -> Result<Option<String>> {
    let did = did.to_owned();
    let password = password.to_owned();
    let password_encrypted = hash_app_password(&did, &password).await?;
    db.run(move |conn| {
        Ok(conn
            .query_row(
                "SELECT name FROM app_password WHERE did = ?1 AND password = ?2",
                params![did, password_encrypted],
                |row| row.get(0),
            )
            .optional()?)
    })
    .await
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
    db: &Db,
) -> Result<CreateAppPasswordOutput> {
    let str = &get_random_str()[0..16].to_lowercase();
    let chunks = [&str[0..4], &str[4..8], &str[8..12], &str[12..16]];
    let password = chunks.join("-");
    let password_encrypted = hash_app_password(&did, &password).await?;

    let created_at = now();

    db.run(move |conn| {
        let got: Option<String> = conn
            .query_row(
                "INSERT INTO app_password (did, name, password, \"createdAt\") \
                 VALUES (?1, ?2, ?3, ?4) \
                 ON CONFLICT (did, name) DO NOTHING \
                 RETURNING name",
                params![did, name, password_encrypted, created_at],
                |row| row.get(0),
            )
            .optional()?;
        if got.is_some() {
            Ok(CreateAppPasswordOutput {
                name: name.clone(),
                password: password.clone(),
                created_at: created_at.clone(),
            })
        } else {
            bail!("could not create app-specific password")
        }
    })
    .await
}

pub async fn list_app_passwords(did: &str, db: &Db) -> Result<Vec<(String, String)>> {
    let did = did.to_owned();
    db.run(move |conn| {
        let mut stmt =
            conn.prepare("SELECT name, \"createdAt\" FROM app_password WHERE did = ?1")?;
        let rows = stmt
            .query_map(params![did], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<(String, String)>, rusqlite::Error>>()?;
        Ok(rows)
    })
    .await
}

pub async fn update_user_password(opts: UpdateUserPasswordOpts, db: &Db) -> Result<()> {
    db.run(move |conn| {
        conn.execute(
            "UPDATE account SET password = ?1 WHERE did = ?2",
            params![opts.password_encrypted, opts.did],
        )?;
        Ok(())
    })
    .await
}

pub async fn delete_app_password(did: &str, name: &str, db: &Db) -> Result<()> {
    let did = did.to_owned();
    let name = name.to_owned();
    db.run(move |conn| {
        conn.execute(
            "DELETE FROM app_password WHERE did = ?1 AND name = ?2",
            params![did, name],
        )?;
        Ok(())
    })
    .await
}
