use anyhow::{Result, anyhow};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};

// We use Argon because it's 3x faster than scrypt.
pub fn gen_salt_and_hash(password: String) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    // Hash password to PHC string
    let argon2 = Argon2::default();
    let password_hash = argon2.hash_password(password.as_ref(), &salt).map_err(|error| {
        anyhow!(error.to_string())
    })?.to_string();
    Ok(password_hash)
}

pub fn verify(password: String, stored_hash: String) -> Result<bool> {
    let parsed_hash = PasswordHash::new(&stored_hash).map_err(|error| {
        anyhow!(error.to_string())
    })?;
    Ok(Argon2::default()
        .verify_password(password.as_ref(), &parsed_hash)
        .is_ok())
}
