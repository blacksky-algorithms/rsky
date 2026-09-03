use crate::db::sqlite::Db;
use anyhow::{anyhow, bail, Result};
use argon2::{
    password_hash::{PasswordHash, PasswordVerifier},
    Argon2,
};
use rand::rngs::OsRng;
use rand::RngCore;
use rsky_common::{get_random_str, now};
use rsky_lexicon::com::atproto::server::CreateAppPasswordOutput;
use rusqlite::{params, OptionalExtension};
use scrypt::{scrypt, Params as ScryptParams};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

pub struct UpdateUserPasswordOpts {
    pub did: String,
    pub password_encrypted: String,
}

/// Scrypt cost parameters, chosen to match the reference TypeScript PDS
/// (`packages/pds/src/account-manager/helpers/scrypt.ts`), which calls
/// Node's `crypto.scrypt(password, salt, 64, cb)` with no options object.
/// Node's documented defaults for the omitted options are `N = 16384`
/// (`log2(N) = 14`), `r = 8`, `p = 1`.
const SCRYPT_LOG_N: u8 = 14;
const SCRYPT_R: u32 = 8;
const SCRYPT_P: u32 = 1;
/// Derived key length in bytes: pds's `scrypt.ts` always requests 64 bytes.
const SCRYPT_KEY_LEN: usize = 64;
/// Random salt length in bytes (before hex-encoding), matching pds's
/// `crypto.randomBytes(16).toString('hex')` in `genSaltAndHash`.
const SCRYPT_SALT_LEN: usize = 16;

fn scrypt_params() -> ScryptParams {
    ScryptParams::new(SCRYPT_LOG_N, SCRYPT_R, SCRYPT_P)
        .expect("static scrypt parameters are always valid")
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

/// Hash a brand-new password with a freshly generated random salt.
///
/// This always produces a **scrypt** hash, in the exact `<hex salt>:<hex
/// derived key>` shape written by pds's `scrypt.ts genSaltAndHash`, so rows
/// created going forward (new accounts, password resets, `updateAccountPassword`)
/// are readable by either rsky-pds or the reference TS pds against the same
/// database.
pub fn gen_salt_and_hash(password: String) -> Result<String> {
    let mut salt_bytes = [0u8; SCRYPT_SALT_LEN];
    OsRng.fill_bytes(&mut salt_bytes);
    let salt = hex::encode(salt_bytes);
    hash_with_salt(&password, &salt)
}

/// Hash `password` with the scrypt KDF using `salt` verbatim as the salt
/// input bytes.
///
/// Note: this intentionally mirrors Node's `crypto.scrypt(password, salt, 64,
/// cb)`, which -- when `salt` is a JS string -- treats it as its raw
/// UTF-8/ASCII byte representation, **not** as hex-decoded bytes. So when
/// `salt` is itself a hex-encoded string (as produced by `gen_salt_and_hash`
/// and `hash_app_password`), the bytes actually fed into scrypt are the ASCII
/// bytes of that hex string, not the 16 raw bytes it represents. Matching
/// this exactly is required for interop with the TS pds's stored hashes.
pub fn hash_with_salt(password: &String, salt: &str) -> Result<String> {
    let params = scrypt_params();
    let mut derived_key = [0u8; SCRYPT_KEY_LEN];
    scrypt(
        password.as_bytes(),
        salt.as_bytes(),
        &params,
        &mut derived_key,
    )
    .map_err(|error| anyhow!(error.to_string()))?;
    Ok(format!("{}:{}", salt, hex::encode(derived_key)))
}

/// Verify `password` against `stored_hash`.
///
/// Dispatches on the stored hash's own encoding so that both algorithms can
/// coexist in the `account.password` column during (and after) the Argon2 ->
/// scrypt migration:
///   - Argon2 PHC strings always start with `$` (e.g. `$argon2id$v=19$...`).
///   - scrypt hashes are `<hex salt>:<hex derived key>` and never start with
///     `$`.
///
/// `gen_salt_and_hash`/`hash_with_salt` only ever produce scrypt hashes going
/// forward, but real rsky-pds deployments already have accounts hashed with
/// Argon2. Rather than force a mass password reset, old rows keep verifying
/// against Argon2 forever; only newly hashed/reset passwords move to scrypt.
pub fn verify(password: &String, stored_hash: &str) -> Result<bool> {
    if stored_hash.starts_with('$') {
        verify_argon2(password, stored_hash)
    } else {
        verify_scrypt(password, stored_hash)
    }
}

fn verify_argon2(password: &String, stored_hash: &str) -> Result<bool> {
    let parsed_hash = PasswordHash::new(stored_hash).map_err(|error| anyhow!(error.to_string()))?;
    Ok(Argon2::default()
        .verify_password(password.as_ref(), &parsed_hash)
        .is_ok())
}

fn verify_scrypt(password: &String, stored_hash: &str) -> Result<bool> {
    let (salt, expected_hex) = stored_hash
        .split_once(':')
        .ok_or_else(|| anyhow!("invalid scrypt hash format: missing ':' separator"))?;
    let derived = hash_with_salt(password, salt)?;
    let (_, derived_hex) = derived
        .split_once(':')
        .expect("hash_with_salt always returns a '<salt>:<hash>' string");
    // Constant-time comparison of the hex-encoded derived keys, as called
    // out by the scrypt crate's own doc comment on `scrypt::scrypt`.
    Ok(derived_hex.as_bytes().ct_eq(expected_hex.as_bytes()).into())
}

pub async fn hash_app_password(did: &String, password: &String) -> Result<String> {
    let digest = Sha256::digest(did);
    let salt = hex::encode(&digest[0..16]);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A scrypt hash produced by our own `gen_salt_and_hash`/`hash_with_salt`
    /// round-trips through `verify`.
    #[test]
    fn scrypt_hash_round_trips() {
        let hash = gen_salt_and_hash("secret".to_owned()).unwrap();
        // Sanity-check the shape: no PHC `$` prefix, exactly one `:` field
        // separator, 32 hex chars of salt, 128 hex chars (64 bytes) of key.
        assert!(!hash.starts_with('$'));
        let (salt, key) = hash.split_once(':').unwrap();
        assert_eq!(salt.len(), SCRYPT_SALT_LEN * 2);
        assert_eq!(key.len(), SCRYPT_KEY_LEN * 2);

        assert!(verify(&"secret".to_owned(), &hash).unwrap());
        assert!(!verify(&"other".to_owned(), &hash).unwrap());
    }

    /// A hand-constructed scrypt hash in pds's exact stored format (`<hex
    /// salt>:<hex derived key>`, produced independently via Python's
    /// `hashlib.scrypt` with N=16384, r=8, p=1, dklen=64, and the salt bytes
    /// being the ASCII bytes of the hex salt string itself -- exactly what
    /// Node's `crypto.scrypt` does when given a string salt) verifies
    /// correctly, proving interop with the reference TS pds.
    #[test]
    fn interops_with_reference_pds_scrypt_format() {
        let stored_hash = "aabbccddeeff00112233445566778899:\
3d2a91e248809343123c0186c87868141ad7be0efcdd1939b28a85c3a9a6f84\
501a21efec04cedc29e2d7dad96021bf0109d0ffb2c7be13faf3f9eac5adfed25";
        assert!(verify(
            &"correct horse battery staple".to_owned(),
            stored_hash
        )
        .unwrap());
        assert!(!verify(&"wrong password".to_owned(), stored_hash).unwrap());
    }

    /// A pre-existing Argon2-format hash (as produced by the old
    /// `gen_salt_and_hash`, before the scrypt migration) still verifies, so
    /// accounts created before this change are never locked out.
    #[test]
    fn legacy_argon2_hash_still_verifies() {
        // A real Argon2id PHC string for the password "secret", generated
        // with the old argon2-based `gen_salt_and_hash`.
        let stored_hash =
            "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHRzb21lc2FsdA$\
14ukWqiThj4Xz77NYv01V28GbBZHY9AaZwsFswQFO0U";
        assert!(verify(&"secret".to_owned(), stored_hash).unwrap());
        assert!(!verify(&"other".to_owned(), stored_hash).unwrap());
    }

    #[test]
    fn rejects_malformed_hash() {
        assert!(verify(&"secret".to_owned(), "not-a-recognized-hash-format").is_err());
    }

    #[test]
    fn hash_with_salt_accepts_arbitrary_salt_strings() {
        // pds's `hashWithSalt` performs no validation on the salt string --
        // any bytes are valid scrypt salt input -- so we match that
        // permissiveness rather than rejecting "unusual" salts.
        let hash = hash_with_salt(&"secret".to_owned(), "not/a valid+hex!salt").unwrap();
        assert!(verify(&"secret".to_owned(), &hash).unwrap());
    }
}
