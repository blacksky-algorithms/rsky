use crate::jwk::errors::JwkError;
use crate::jwk::jwt::{JwtHeader, JwtPayload};

pub fn unsafe_decode_jwt(jwt: String) -> Result<(JwtHeader, JwtPayload), JwkError> {
    unimplemented!()
}
