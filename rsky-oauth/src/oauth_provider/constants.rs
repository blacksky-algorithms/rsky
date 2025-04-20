// The purpose of the prefix is to provide type safety

pub const DEVICE_ID_PREFIX: &str = "dev-";
pub const DEVICE_ID_BYTES_LENGTH: usize = 64; // 128 bits

pub const SESSION_ID_PREFIX: &str = "ses-";
pub const SESSION_ID_BYTES_LENGTH: usize = 64; // 128 bits - only valid if device id is valid

pub const REFRESH_TOKEN_PREFIX: &str = "ref-";
pub const REFRESH_TOKEN_BYTES_LENGTH: usize = 64; // 256 bits

pub const TOKEN_ID_PREFIX: &str = "tok-";
pub const TOKEN_ID_BYTES_LENGTH: usize = 32; // 128 bits - used as `jti` in JWTs (cannot be forged)

pub const REQUEST_ID_PREFIX: &str = "req-";
pub const REQUEST_ID_BYTES_LENGTH: usize = 32; // 128 bits

pub const CODE_PREFIX: &str = "cod-";
pub const CODE_BYTES_LENGTH: usize = 64;

pub const SECOND: u64 = 1000;
pub const MINUTE: u64 = 60 * SECOND;
pub const HOUR: u64 = 60 * MINUTE;
pub const DAY: u64 = 24 * HOUR;
pub const WEEK: u64 = 7 * DAY;
pub const YEAR: u64 = 365 * DAY;
pub const MONTH: u64 = YEAR / 12;

/** 7 days */
pub const AUTHENTICATION_MAX_AGE: u64 = 7 * DAY;

/** 60 minutes */
pub const TOKEN_MAX_AGE: u64 = 60 * MINUTE;

/** 5 minutes */
pub const AUTHORIZATION_INACTIVITY_TIMEOUT: u64 = 5 * MINUTE;

/** 1 months */
pub const AUTHENTICATED_REFRESH_INACTIVITY_TIMEOUT: u64 = MONTH;

/** 2 days */
pub const UNAUTHENTICATED_REFRESH_INACTIVITY_TIMEOUT: u64 = 2 * DAY;

/** 1 week */
pub const UNAUTHENTICATED_REFRESH_LIFETIME: u64 = WEEK;

/** 1 year */
pub const AUTHENTICATED_REFRESH_LIFETIME: u64 = YEAR;

/** 5 minutes */
pub const PAR_EXPIRES_IN: u64 = 5 * MINUTE;

/**
 * 59 seconds (should be less than a minute)
 *
 * @see {@link https://datatracker.ietf.org/doc/html/rfc9101#section-10.2}
 */
pub const JAR_MAX_AGE: u64 = 59 * SECOND;

/** 1 minute */
pub const CLIENT_ASSERTION_MAX_AGE: u64 = MINUTE;

/** 3 minutes */
pub const DPOP_NONCE_MAX_AGE: u64 = 3 * MINUTE;

/** 5 seconds */
pub const SESSION_FIXATION_MAX_AGE: u64 = 5 * SECOND;

/** 1 day */
pub const CODE_CHALLENGE_REPLAY_TIMEFRAME: u64 = DAY;
