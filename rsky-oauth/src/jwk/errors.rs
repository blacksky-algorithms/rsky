const ERR_JWKS_NO_MATCHING_KEY: &str = "ERR_JWKS_NO_MATCHING_KEY";
const ERR_JWK_INVALID: &str = "ERR_JWK_INVALID";
const ERR_JWK_NOT_FOUND: &str = "ERR_JWK_NOT_FOUND";
const ERR_JWT_INVALID: &str = "ERR_JWT_INVALID";
const ERR_JWT_CREATE: &str = "ERR_JWT_CREATE";
const ERR_JWT_VERIFY: &str = "ERR_JWT_VERIFY";

#[derive(Debug)]
pub enum JwkError {
    JwtCreateError,
    JwtVerifyError,
}
