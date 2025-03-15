pub mod jws;
pub mod jwt;
pub mod signing;

pub use self::signing::create_signed_jwt;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Header {
    Jws(jws::Header),
    // TODO: JWE?
}

#[cfg(test)]
mod tests {
    use jose_jwa::{Algorithm, Signing};
    use jws::RegisteredHeader;

    use super::*;

    #[test]
    fn test_serialize_claims() {
        let header = Header::from(RegisteredHeader::from(Algorithm::Signing(Signing::Es256)));
        let json = serde_json::to_string(&header).expect("failed to serialize header");
        assert_eq!(json, r#"{"alg":"ES256"}"#);
    }
}
