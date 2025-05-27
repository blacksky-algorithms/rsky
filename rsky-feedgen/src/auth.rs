use crate::models::JwtParts;
use base64::{engine::general_purpose, Engine as _};
use std::time::{SystemTime, UNIX_EPOCH};

/// Verifies a JWT (JSON Web Token) for validity, audience, and expiration.
///
/// # Arguments
///
/// * `jwtstr` - A reference to a `String` containing the JWT to be verified. 
/// * `service_did` - A reference to a `String` containing the decentralized identifier (DID) of the service, 
///   which is used to validate the audience (`aud`) of the JWT payload.
///
/// # Returns
///
/// * `Ok(String)` - The payload of the JWT as a serialized `String` if successfully verified.
/// * `Err(String)` - A descriptive error message if the JWT verification fails.
///
/// # Errors
///
/// This function will return an error in the following situations:
///
/// 1. The JWT is not properly formatted (i.e., it does not contain three parts separated by dots ".").
/// 2. The payload cannot be base64 decoded or deserialized into the expected `JwtParts` structure.
/// 3. The JWT has expired (as determined by comparing the current system time to the `exp` field in the payload).
/// 4. The audience (`aud`) specified in the JWT does not match the provided `service_did`.
/// 5. The payload cannot be serialized back to a JSON `String` after validation.
///
/// # Example
///
/// ```rust
/// use rsky_feedgen::verify_jwt;
///
/// let jwt = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJhdWQiOiJzZXJ2aWNlX2RpZCIsImV4cCI6MTcxNzIwODAwOX0.somesignature".to_string();
/// let service_did = "service_did".to_string();
///
/// match verify_jwt(&jwt, &service_did) {
///     Ok(payload) => println!("Verified payload: {}", payload),
///     Err(err) => println!("JWT verification failed: {}", err),
/// }
/// ```
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
