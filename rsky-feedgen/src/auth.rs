use crate::models::JwtParts;
use base64::{engine::general_purpose, Engine as _};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn verify_jwt(jwtstr: &String, service_did: &String) -> Result<String, String> {
    let parts = jwtstr.split(".").map(String::from).collect::<Vec<_>>();

    if parts.len() != 3 {
        return Err("poorly formatted jwt".into());
    }

    let bytes = general_purpose::STANDARD_NO_PAD.decode(&parts[1]).unwrap();

    if let Ok(payload) = std::str::from_utf8(&bytes) {
        if let Ok(payload) = serde_json::from_str::<JwtParts>(payload) {
            let start = SystemTime::now();
            let since_the_epoch = start
                .duration_since(UNIX_EPOCH)
                .expect("Time went backwards");

            if since_the_epoch.as_millis() / 1000 > payload.exp {
                return Err("jwt expired".into());
            }
            if service_did != &payload.aud {
                return Err("jwt audience does not match service did".into());
            }
            // TO DO: Verify cryptographic signature
            if let Ok(jwtstr) = serde_json::to_string(&payload) {
                Ok(jwtstr)
            } else {
                Err("error parsing payload".into())
            }
        } else {
            Err("error parsing payload".into())
        }
    } else {
        Err("error parsing payload".into())
    }
}
