//! Device-session constants and identifiers shared by the provider and the
//! host application's cookie handling.

/// How long a device authentication stays fresh before the account must sign
/// in again, in seconds (7 days).
pub const AUTHENTICATION_MAX_AGE: u64 = 7 * 24 * 3600;
/// Lifetime of an ephemeral (not remembered) sign-in proof, in seconds.
pub const EPHEMERAL_SESSION_MAX_AGE: u64 = 15 * 60;
/// Minimum interval between two metadata touches of the same device row, in
/// seconds.
pub const DEVICE_TOUCH_INTERVAL: u64 = 60;

pub const DEVICE_ID_PREFIX: &str = "dev-";
pub const SESSION_ID_PREFIX: &str = "ses-";
const ID_BYTES_LENGTH: usize = 16;

fn random_hex_id(prefix: &str) -> String {
    format!(
        "{prefix}{}",
        hex::encode(rsky_crypto::utils::random_bytes(ID_BYTES_LENGTH))
    )
}

pub fn generate_device_id() -> String {
    random_hex_id(DEVICE_ID_PREFIX)
}

pub fn generate_session_id() -> String {
    random_hex_id(SESSION_ID_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_are_prefixed_and_unique() {
        let device = generate_device_id();
        let session = generate_session_id();
        assert!(device.starts_with(DEVICE_ID_PREFIX));
        assert!(session.starts_with(SESSION_ID_PREFIX));
        assert_eq!(device.len(), DEVICE_ID_PREFIX.len() + ID_BYTES_LENGTH * 2);
        assert_eq!(session.len(), SESSION_ID_PREFIX.len() + ID_BYTES_LENGTH * 2);
        assert_ne!(generate_session_id(), session);
        assert_eq!(AUTHENTICATION_MAX_AGE, 604_800);
        assert_eq!(EPHEMERAL_SESSION_MAX_AGE, 900);
        assert_eq!(DEVICE_TOUCH_INTERVAL, 60);
    }
}
