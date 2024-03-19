use scrypt::{
    password_hash::{
        rand_core::OsRng,
        PasswordHash, PasswordHasher, PasswordVerifier, SaltString
    },
    Scrypt
};
use anyhow::Result;

pub fn gen_salt_and_hash(password: String) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    // Hash password to PHC string ($scrypt$...)
    let password_hash = Scrypt.hash_password(password.as_ref(), &salt)?.to_string();
    Ok(password_hash)
}

pub fn verify(password: String, stored_hash: String) -> Result<bool> {
    let parsed_hash = PasswordHash::new(&stored_hash)?;
    Ok(Scrypt.verify_password(password.as_ref(), &parsed_hash).is_ok())
}
