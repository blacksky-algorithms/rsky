use crate::error::OAuthError;
use crate::jwk::{EcCurve, Jwk};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

pub const DEFAULT_CLOCK_SKEW: u64 = 60;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JwtHeader {
    pub alg: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub typ: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jwk: Option<Jwk>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl JwtHeader {
    pub fn new(alg: impl Into<String>) -> Self {
        Self {
            alg: alg.into(),
            typ: None,
            kid: None,
            jwk: None,
            extra: Map::new(),
        }
    }

    pub fn validate_typ(&self, expected: &str) -> Result<(), OAuthError> {
        match self.typ.as_deref() {
            Some(typ) if typ == expected => Ok(()),
            _ => Err(OAuthError::InvalidToken(format!(
                "unexpected JWT \"typ\" header, expected \"{expected}\""
            ))),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct JwtClaims {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iss: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aud: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exp: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nbf: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iat: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jti: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl JwtClaims {
    /// Validates `exp`, `iat` and `nbf` (when present) against `now` with
    /// the given clock-skew tolerance in seconds.
    pub fn validate_time(&self, now: u64, skew: u64) -> Result<(), OAuthError> {
        if let Some(exp) = self.exp {
            if exp.saturating_add(skew) <= now {
                return Err(OAuthError::InvalidToken("token has expired".to_string()));
            }
        }
        if let Some(iat) = self.iat {
            if iat > now.saturating_add(skew) {
                return Err(OAuthError::InvalidToken(
                    "token issued in the future".to_string(),
                ));
            }
        }
        if let Some(nbf) = self.nbf {
            if nbf > now.saturating_add(skew) {
                return Err(OAuthError::InvalidToken("token not yet valid".to_string()));
            }
        }
        Ok(())
    }

    pub fn validate_iss(&self, expected: &str) -> Result<(), OAuthError> {
        match self.iss.as_deref() {
            Some(iss) if iss == expected => Ok(()),
            _ => Err(OAuthError::InvalidToken(
                "unexpected \"iss\" claim".to_string(),
            )),
        }
    }

    pub fn validate_aud(&self, expected: &str) -> Result<(), OAuthError> {
        let matches = match &self.aud {
            Some(Value::String(aud)) => aud == expected,
            Some(Value::Array(auds)) => auds.iter().any(|aud| aud.as_str() == Some(expected)),
            _ => false,
        };
        if matches {
            Ok(())
        } else {
            Err(OAuthError::InvalidToken(
                "unexpected \"aud\" claim".to_string(),
            ))
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecodedJwt {
    pub header: JwtHeader,
    pub claims: JwtClaims,
    pub signing_input: String,
    pub signature: Vec<u8>,
}

pub fn decode(token: &str) -> Result<DecodedJwt, OAuthError> {
    let mut parts = token.split('.');
    let (Some(header_b64), Some(claims_b64), Some(signature_b64), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(OAuthError::InvalidToken("malformed JWT".to_string()));
    };
    let header: JwtHeader = decode_json_part(header_b64, "header")?;
    let claims: JwtClaims = decode_json_part(claims_b64, "claims")?;
    let signature = URL_SAFE_NO_PAD
        .decode(signature_b64)
        .map_err(|e| OAuthError::InvalidToken(format!("invalid JWT signature encoding: {e}")))?;
    Ok(DecodedJwt {
        header,
        claims,
        signing_input: format!("{header_b64}.{claims_b64}"),
        signature,
    })
}

pub fn verify(token: &str, key: &Jwk) -> Result<DecodedJwt, OAuthError> {
    let decoded = decode(token)?;
    verify_signature(&decoded, key)?;
    Ok(decoded)
}

pub fn verify_signature(decoded: &DecodedJwt, key: &Jwk) -> Result<(), OAuthError> {
    let curve = key.curve()?;
    if decoded.header.alg != curve.alg() {
        return Err(OAuthError::InvalidToken(format!(
            "JWT \"alg\" {} does not match key algorithm {}",
            decoded.header.alg,
            curve.alg()
        )));
    }
    let public_key = key.to_sec1_uncompressed()?;
    let digest = Sha256::digest(decoded.signing_input.as_bytes());
    let verified = match curve {
        EcCurve::P256 => {
            let signature = p256::ecdsa::Signature::from_slice(&decoded.signature)
                .map_err(|e| OAuthError::InvalidToken(format!("invalid ES256 signature: {e}")))?;
            let signature = signature.normalize_s().unwrap_or(signature);
            rsky_crypto::p256::operations::verify_prehash_sig(
                &public_key,
                &digest,
                &signature.to_vec(),
                None,
            )
            .unwrap_or(false)
        }
        EcCurve::K256 => {
            let mut signature = secp256k1::ecdsa::Signature::from_compact(&decoded.signature)
                .map_err(|e| OAuthError::InvalidToken(format!("invalid ES256K signature: {e}")))?;
            signature.normalize_s();
            rsky_crypto::secp256k1::operations::verify_sig(
                &public_key,
                &digest,
                &signature.serialize_compact(),
                None,
            )
            .unwrap_or(false)
        }
    };
    if verified {
        Ok(())
    } else {
        Err(OAuthError::InvalidToken(
            "JWT signature verification failed".to_string(),
        ))
    }
}

pub fn sign(header: &JwtHeader, claims: &JwtClaims, key: &Jwk) -> Result<String, OAuthError> {
    let curve = key.curve()?;
    if header.alg != curve.alg() {
        return Err(OAuthError::InvalidRequest(format!(
            "JWT \"alg\" {} does not match key algorithm {}",
            header.alg,
            curve.alg()
        )));
    }
    let d = key.private_key_bytes()?;
    let header_json = serde_json::to_string(header).expect("JWT header serialization cannot fail");
    let claims_json = serde_json::to_string(claims).expect("JWT claims serialization cannot fail");
    let signing_input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(header_json),
        URL_SAFE_NO_PAD.encode(claims_json)
    );
    let digest = Sha256::digest(signing_input.as_bytes());
    let signature = match curve {
        EcCurve::P256 => {
            use p256::ecdsa::signature::hazmat::PrehashSigner;
            let signing_key = p256::ecdsa::SigningKey::from_slice(&d)
                .map_err(|e| OAuthError::InvalidRequest(format!("invalid private key: {e}")))?;
            let signature: p256::ecdsa::Signature = signing_key
                .sign_prehash(&digest)
                .expect("P-256 prehash signing cannot fail on a 32-byte digest");
            let signature = signature.normalize_s().unwrap_or(signature);
            signature.to_bytes().to_vec()
        }
        EcCurve::K256 => {
            let secret = secp256k1::SecretKey::from_slice(&d)
                .map_err(|e| OAuthError::InvalidRequest(format!("invalid private key: {e}")))?;
            let message =
                secp256k1::Message::from_digest_slice(&digest).expect("sha256 digest is 32 bytes");
            let mut signature = secret.sign_ecdsa(message);
            signature.normalize_s();
            signature.serialize_compact().to_vec()
        }
    };
    Ok(format!(
        "{signing_input}.{}",
        URL_SAFE_NO_PAD.encode(signature)
    ))
}

fn decode_json_part<T: serde::de::DeserializeOwned>(
    part: &str,
    name: &str,
) -> Result<T, OAuthError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(part)
        .map_err(|e| OAuthError::InvalidToken(format!("invalid JWT {name} encoding: {e}")))?;
    serde_json::from_slice(&bytes)
        .map_err(|e| OAuthError::InvalidToken(format!("invalid JWT {name}: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const NOW: u64 = 1_700_000_000;

    fn private_key(curve: EcCurve) -> Jwk {
        Jwk::from_private_key_bytes(curve, &[0x42u8; 32]).unwrap()
    }

    fn signed_token(curve: EcCurve) -> (String, Jwk) {
        let key = private_key(curve);
        let mut header = JwtHeader::new(curve.alg());
        header.typ = Some("JWT".to_string());
        let claims = JwtClaims {
            iss: Some("https://issuer.example.com".to_string()),
            aud: Some(json!("https://audience.example.com")),
            exp: Some(NOW + 300),
            iat: Some(NOW),
            jti: Some("token-1".to_string()),
            ..Default::default()
        };
        (sign(&header, &claims, &key).unwrap(), key)
    }

    #[test]
    fn sign_and_verify_both_curves() {
        for curve in [EcCurve::P256, EcCurve::K256] {
            let (token, key) = signed_token(curve);
            let decoded = verify(&token, &key.to_public()).unwrap();
            assert_eq!(decoded.header.alg, curve.alg());
            assert_eq!(decoded.claims.jti.as_deref(), Some("token-1"));
            decoded.header.validate_typ("JWT").unwrap();
            decoded
                .claims
                .validate_time(NOW, DEFAULT_CLOCK_SKEW)
                .unwrap();
            decoded
                .claims
                .validate_iss("https://issuer.example.com")
                .unwrap();
            decoded
                .claims
                .validate_aud("https://audience.example.com")
                .unwrap();
        }
    }

    #[test]
    fn tampered_payload_rejected() {
        for curve in [EcCurve::P256, EcCurve::K256] {
            let (token, key) = signed_token(curve);
            let mut parts: Vec<&str> = token.split('.').collect();
            let tampered_claims = URL_SAFE_NO_PAD.encode(r#"{"jti":"evil"}"#);
            parts[1] = &tampered_claims;
            let tampered = parts.join(".");
            assert_eq!(
                verify(&tampered, &key).unwrap_err(),
                OAuthError::InvalidToken("JWT signature verification failed".to_string())
            );
        }
    }

    #[test]
    fn high_s_signature_accepted_p256() {
        let (token, key) = signed_token(EcCurve::P256);
        let decoded = decode(&token).unwrap();
        let signature = p256::ecdsa::Signature::from_slice(&decoded.signature).unwrap();
        let high =
            p256::ecdsa::Signature::from_scalars(*signature.r().as_ref(), -*signature.s().as_ref())
                .unwrap();
        assert!(high.normalize_s().is_some());
        let parts: Vec<&str> = token.split('.').collect();
        let high_token = format!(
            "{}.{}.{}",
            parts[0],
            parts[1],
            URL_SAFE_NO_PAD.encode(high.to_bytes())
        );
        verify(&high_token, &key).unwrap();
    }

    #[test]
    fn high_s_signature_accepted_k256() {
        const K256_N: [u8; 32] = [
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFE, 0xBA, 0xAE, 0xDC, 0xE6, 0xAF, 0x48, 0xA0, 0x3B, 0xBF, 0xD2, 0x5E, 0x8C,
            0xD0, 0x36, 0x41, 0x41,
        ];
        let (token, key) = signed_token(EcCurve::K256);
        let decoded = decode(&token).unwrap();
        let mut compact: [u8; 64] = decoded.signature.clone().try_into().unwrap();
        // Replace s with n - s to produce the high-S counterpart.
        let mut borrow = 0i16;
        for i in (0..32).rev() {
            let mut diff = K256_N[i] as i16 - compact[32 + i] as i16 - borrow;
            borrow = if diff < 0 {
                diff += 256;
                1
            } else {
                0
            };
            compact[32 + i] = diff as u8;
        }
        assert_eq!(borrow, 0);
        let parts: Vec<&str> = token.split('.').collect();
        let high_token = format!(
            "{}.{}.{}",
            parts[0],
            parts[1],
            URL_SAFE_NO_PAD.encode(compact)
        );
        verify(&high_token, &key).unwrap();
    }

    #[test]
    fn malformed_signatures_rejected() {
        for curve in [EcCurve::P256, EcCurve::K256] {
            let (token, key) = signed_token(curve);
            let parts: Vec<&str> = token.split('.').collect();
            let zero_sig = format!(
                "{}.{}.{}",
                parts[0],
                parts[1],
                URL_SAFE_NO_PAD.encode([0u8; 64])
            );
            assert!(verify(&zero_sig, &key).is_err());
            let short_sig = format!(
                "{}.{}.{}",
                parts[0],
                parts[1],
                URL_SAFE_NO_PAD.encode([1u8])
            );
            assert!(verify(&short_sig, &key).is_err());
        }
    }

    #[test]
    fn alg_key_mismatch_rejected() {
        let (token, _) = signed_token(EcCurve::P256);
        let k256_key = private_key(EcCurve::K256);
        let err = verify(&token, &k256_key).unwrap_err();
        assert_eq!(
            err,
            OAuthError::InvalidToken(
                "JWT \"alg\" ES256 does not match key algorithm ES256K".to_string()
            )
        );

        let header = JwtHeader::new("ES256");
        let err = sign(&header, &JwtClaims::default(), &k256_key).unwrap_err();
        assert_eq!(
            err,
            OAuthError::InvalidRequest(
                "JWT \"alg\" ES256 does not match key algorithm ES256K".to_string()
            )
        );
    }

    #[test]
    fn sign_requires_private_key() {
        let key = private_key(EcCurve::P256).to_public();
        let header = JwtHeader::new("ES256");
        assert!(sign(&header, &JwtClaims::default(), &key).is_err());
    }

    #[test]
    fn sign_rejects_invalid_private_scalar() {
        for curve in [EcCurve::P256, EcCurve::K256] {
            let mut key = private_key(curve);
            key.d = Some(URL_SAFE_NO_PAD.encode([0u8; 32]));
            let header = JwtHeader::new(curve.alg());
            assert!(sign(&header, &JwtClaims::default(), &key).is_err());
        }
    }

    #[test]
    fn decode_rejects_malformed_tokens() {
        assert!(decode("only.two").is_err());
        assert!(decode("a.b.c.d").is_err());
        assert!(decode("!.!.!").is_err());
        let header = URL_SAFE_NO_PAD.encode(r#"{"alg":"ES256"}"#);
        let claims = URL_SAFE_NO_PAD.encode(r#"{}"#);
        assert!(decode(&format!("{header}.notjson.sig")).is_err());
        let bad_json = URL_SAFE_NO_PAD.encode("[1,2");
        assert!(decode(&format!("{header}.{bad_json}.sig")).is_err());
        assert!(decode(&format!("{header}.{claims}.!bad!")).is_err());
        let decoded = decode(&format!(
            "{header}.{claims}.{}",
            URL_SAFE_NO_PAD.encode("x")
        ))
        .unwrap();
        assert_eq!(decoded.signature, b"x");
        assert!(!format!("{decoded:?}").is_empty());
    }

    #[test]
    fn verify_rejects_non_ec_key() {
        let (token, key) = signed_token(EcCurve::P256);
        let mut rsa = key.clone();
        rsa.kty = "RSA".to_string();
        assert!(verify(&token, &rsa).is_err());
        assert!(sign(&JwtHeader::new("ES256"), &JwtClaims::default(), &rsa).is_err());
        let mut bad_x = key.clone();
        bad_x.x = "!not-base64!".to_string();
        assert!(verify(&token, &bad_x).is_err());
        assert!(verify("not-even-a-jwt", &key).is_err());
    }

    #[test]
    fn time_validation() {
        let claims = JwtClaims {
            exp: Some(NOW + 10),
            iat: Some(NOW),
            nbf: Some(NOW),
            ..Default::default()
        };
        claims.validate_time(NOW, DEFAULT_CLOCK_SKEW).unwrap();
        // Within skew tolerance.
        claims.validate_time(NOW + 60, DEFAULT_CLOCK_SKEW).unwrap();
        assert_eq!(
            claims
                .validate_time(NOW + 70, DEFAULT_CLOCK_SKEW)
                .unwrap_err(),
            OAuthError::InvalidToken("token has expired".to_string())
        );
        let future = JwtClaims {
            iat: Some(NOW + 120),
            ..Default::default()
        };
        assert_eq!(
            future.validate_time(NOW, DEFAULT_CLOCK_SKEW).unwrap_err(),
            OAuthError::InvalidToken("token issued in the future".to_string())
        );
        let not_yet = JwtClaims {
            nbf: Some(NOW + 120),
            ..Default::default()
        };
        assert_eq!(
            not_yet.validate_time(NOW, DEFAULT_CLOCK_SKEW).unwrap_err(),
            OAuthError::InvalidToken("token not yet valid".to_string())
        );
        JwtClaims::default()
            .validate_time(NOW, DEFAULT_CLOCK_SKEW)
            .unwrap();
    }

    #[test]
    fn iss_aud_typ_validation() {
        let claims = JwtClaims {
            iss: Some("iss".to_string()),
            aud: Some(json!(["aud-1", "aud-2"])),
            ..Default::default()
        };
        claims.validate_iss("iss").unwrap();
        assert!(claims.validate_iss("other").is_err());
        assert!(JwtClaims::default().validate_iss("iss").is_err());
        claims.validate_aud("aud-2").unwrap();
        assert!(claims.validate_aud("aud-3").is_err());
        assert!(JwtClaims::default().validate_aud("aud-1").is_err());
        let numeric_aud = JwtClaims {
            aud: Some(json!(42)),
            ..Default::default()
        };
        assert!(numeric_aud.validate_aud("42").is_err());

        let mut header = JwtHeader::new("ES256");
        assert!(header.validate_typ("JWT").is_err());
        header.typ = Some("JWT".to_string());
        header.validate_typ("JWT").unwrap();
        assert!(header.validate_typ("dpop+jwt").is_err());
    }
}
