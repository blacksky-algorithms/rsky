use crate::error::OAuthError;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const P256_CRV: &str = "P-256";
pub const K256_CRV: &str = "secp256k1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EcCurve {
    P256,
    K256,
}

impl EcCurve {
    pub fn crv(self) -> &'static str {
        match self {
            Self::P256 => P256_CRV,
            Self::K256 => K256_CRV,
        }
    }

    pub fn alg(self) -> &'static str {
        match self {
            Self::P256 => "ES256",
            Self::K256 => "ES256K",
        }
    }

    pub fn from_crv(crv: &str) -> Result<Self, OAuthError> {
        match crv {
            P256_CRV => Ok(Self::P256),
            K256_CRV => Ok(Self::K256),
            other => Err(OAuthError::InvalidRequest(format!(
                "unsupported curve: {other}"
            ))),
        }
    }

    pub fn from_alg(alg: &str) -> Result<Self, OAuthError> {
        match alg {
            "ES256" => Ok(Self::P256),
            "ES256K" => Ok(Self::K256),
            other => Err(OAuthError::InvalidRequest(format!(
                "unsupported algorithm: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Jwk {
    pub kty: String,
    pub crv: String,
    pub x: String,
    pub y: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub d: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alg: Option<String>,
    #[serde(rename = "use", default, skip_serializing_if = "Option::is_none")]
    pub r#use: Option<String>,
}

impl Jwk {
    pub fn curve(&self) -> Result<EcCurve, OAuthError> {
        if self.kty != "EC" {
            return Err(OAuthError::InvalidRequest(format!(
                "unsupported key type: {}",
                self.kty
            )));
        }
        EcCurve::from_crv(&self.crv)
    }

    pub fn is_private(&self) -> bool {
        self.d.is_some()
    }

    pub fn to_public(&self) -> Jwk {
        Jwk {
            d: None,
            ..self.clone()
        }
    }

    pub fn from_sec1(curve: EcCurve, point: &[u8]) -> Result<Jwk, OAuthError> {
        let uncompressed = decompress_point(curve, point)?;
        Ok(Jwk {
            kty: "EC".to_string(),
            crv: curve.crv().to_string(),
            x: URL_SAFE_NO_PAD.encode(&uncompressed[1..33]),
            y: URL_SAFE_NO_PAD.encode(&uncompressed[33..65]),
            d: None,
            kid: None,
            alg: None,
            r#use: None,
        })
    }

    pub fn from_private_key_bytes(curve: EcCurve, d: &[u8]) -> Result<Jwk, OAuthError> {
        let uncompressed = match curve {
            EcCurve::P256 => {
                let signing_key = p256::ecdsa::SigningKey::from_slice(d)
                    .map_err(|e| OAuthError::InvalidRequest(format!("invalid private key: {e}")))?;
                signing_key
                    .verifying_key()
                    .to_encoded_point(false)
                    .as_bytes()
                    .to_vec()
            }
            EcCurve::K256 => {
                let secret = secp256k1::SecretKey::from_slice(d)
                    .map_err(|e| OAuthError::InvalidRequest(format!("invalid private key: {e}")))?;
                secret
                    .public_key(secp256k1::SECP256K1)
                    .serialize_uncompressed()
                    .to_vec()
            }
        };
        let mut jwk =
            Self::from_sec1(curve, &uncompressed).expect("derived public key is a valid point");
        jwk.d = Some(URL_SAFE_NO_PAD.encode(d));
        Ok(jwk)
    }

    /// Uncompressed SEC1 point (0x04 || x || y), validated to be on the curve.
    pub fn to_sec1_uncompressed(&self) -> Result<Vec<u8>, OAuthError> {
        let curve = self.curve()?;
        let x = decode_coordinate(&self.x, "x")?;
        let y = decode_coordinate(&self.y, "y")?;
        let mut point = Vec::with_capacity(65);
        point.push(0x04);
        point.extend_from_slice(&x);
        point.extend_from_slice(&y);
        decompress_point(curve, &point)
    }

    pub fn private_key_bytes(&self) -> Result<[u8; 32], OAuthError> {
        let d = self
            .d
            .as_deref()
            .ok_or_else(|| OAuthError::InvalidRequest("JWK is not a private key".to_string()))?;
        decode_coordinate(d, "d")
    }

    /// RFC 7638 JWK thumbprint (the `jkt` used for DPoP key binding).
    pub fn thumbprint(&self) -> String {
        let canonical = format!(
            r#"{{"crv":"{}","kty":"{}","x":"{}","y":"{}"}}"#,
            self.crv, self.kty, self.x, self.y
        );
        URL_SAFE_NO_PAD.encode(Sha256::digest(canonical.as_bytes()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JwkSet {
    pub keys: Vec<Jwk>,
}

impl JwkSet {
    pub fn find_by_kid(&self, kid: &str) -> Option<&Jwk> {
        self.keys.iter().find(|key| key.kid.as_deref() == Some(kid))
    }
}

fn decode_coordinate(value: &str, name: &str) -> Result<[u8; 32], OAuthError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|e| OAuthError::InvalidRequest(format!("invalid JWK \"{name}\": {e}")))?;
    bytes
        .try_into()
        .map_err(|_| OAuthError::InvalidRequest(format!("JWK \"{name}\" must encode 32 bytes")))
}

fn decompress_point(curve: EcCurve, point: &[u8]) -> Result<Vec<u8>, OAuthError> {
    match curve {
        EcCurve::P256 => {
            let key = p256::ecdsa::VerifyingKey::from_sec1_bytes(point)
                .map_err(|e| OAuthError::InvalidRequest(format!("invalid EC point: {e}")))?;
            Ok(key.to_encoded_point(false).as_bytes().to_vec())
        }
        EcCurve::K256 => {
            let key = secp256k1::PublicKey::from_slice(point)
                .map_err(|e| OAuthError::InvalidRequest(format!("invalid EC point: {e}")))?;
            Ok(key.serialize_uncompressed().to_vec())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // RFC 7515 A.3 ES256 key; thumbprint computed per RFC 7638 section 3.2.
    const RFC7515_X: &str = "f83OJ3D2xF1Bg8vub9tLe1gHMzV76e8Tus9uPHvRVEU";
    const RFC7515_Y: &str = "x_FEzRu9m36HLN_tue659LNpXW6pCyStikYjKIWI5a0";
    const RFC7515_THUMBPRINT: &str = "oKIywvGUpTVTyxMQ3bwIIeQUudfr_CkLMjCE19ECD-U";

    fn golden_jwk() -> Jwk {
        serde_json::from_value(serde_json::json!({
            "kty": "EC",
            "crv": "P-256",
            "x": RFC7515_X,
            "y": RFC7515_Y,
        }))
        .unwrap()
    }

    #[test]
    fn golden_thumbprint_vector() {
        assert_eq!(golden_jwk().thumbprint(), RFC7515_THUMBPRINT);
    }

    #[test]
    fn parse_and_serialize_roundtrip() {
        let json = serde_json::json!({
            "kty": "EC",
            "crv": "secp256k1",
            "x": "A",
            "y": "B",
            "d": "C",
            "kid": "key-1",
            "alg": "ES256K",
            "use": "sig",
        });
        let jwk: Jwk = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(jwk.kid.as_deref(), Some("key-1"));
        assert_eq!(jwk.r#use.as_deref(), Some("sig"));
        assert!(jwk.is_private());
        assert_eq!(serde_json::to_value(&jwk).unwrap(), json);
        let public = jwk.to_public();
        assert!(!public.is_private());
        assert_eq!(public.x, jwk.x);
    }

    #[test]
    fn sec1_roundtrip_p256() {
        let jwk = Jwk::from_private_key_bytes(EcCurve::P256, &[0x42u8; 32]).unwrap();
        let uncompressed = jwk.to_sec1_uncompressed().unwrap();
        assert_eq!(uncompressed.len(), 65);
        assert_eq!(uncompressed[0], 0x04);
        let roundtrip = Jwk::from_sec1(EcCurve::P256, &uncompressed).unwrap();
        assert_eq!(roundtrip, jwk.to_public());
        let compressed = p256::ecdsa::VerifyingKey::from_sec1_bytes(&uncompressed)
            .unwrap()
            .to_encoded_point(true);
        let from_compressed = Jwk::from_sec1(EcCurve::P256, compressed.as_bytes()).unwrap();
        assert_eq!(from_compressed, roundtrip);
    }

    #[test]
    fn sec1_roundtrip_k256() {
        let jwk = Jwk::from_private_key_bytes(EcCurve::K256, &[0x42u8; 32]).unwrap();
        let uncompressed = jwk.to_sec1_uncompressed().unwrap();
        assert_eq!(uncompressed.len(), 65);
        let roundtrip = Jwk::from_sec1(EcCurve::K256, &uncompressed).unwrap();
        assert_eq!(roundtrip, jwk.to_public());
        let compressed = secp256k1::PublicKey::from_slice(&uncompressed)
            .unwrap()
            .serialize();
        let from_compressed = Jwk::from_sec1(EcCurve::K256, &compressed).unwrap();
        assert_eq!(from_compressed, roundtrip);
    }

    #[test]
    fn private_key_bytes_roundtrip() {
        let d = [0x42u8; 32];
        for curve in [EcCurve::P256, EcCurve::K256] {
            let jwk = Jwk::from_private_key_bytes(curve, &d).unwrap();
            assert_eq!(jwk.private_key_bytes().unwrap(), d);
            assert_eq!(
                jwk.to_public().private_key_bytes().unwrap_err(),
                OAuthError::InvalidRequest("JWK is not a private key".to_string())
            );
        }
    }

    #[test]
    fn invalid_private_keys_rejected() {
        for curve in [EcCurve::P256, EcCurve::K256] {
            assert!(Jwk::from_private_key_bytes(curve, &[0u8; 32]).is_err());
        }
    }

    #[test]
    fn curve_and_alg_lookups() {
        assert_eq!(EcCurve::from_crv("P-256").unwrap(), EcCurve::P256);
        assert_eq!(EcCurve::from_crv("secp256k1").unwrap(), EcCurve::K256);
        assert!(EcCurve::from_crv("P-384").is_err());
        assert_eq!(EcCurve::from_alg("ES256").unwrap(), EcCurve::P256);
        assert_eq!(EcCurve::from_alg("ES256K").unwrap(), EcCurve::K256);
        assert!(EcCurve::from_alg("RS256").is_err());
        assert_eq!(EcCurve::P256.crv(), "P-256");
        assert_eq!(EcCurve::K256.crv(), "secp256k1");
        assert_eq!(EcCurve::P256.alg(), "ES256");
        assert_eq!(EcCurve::K256.alg(), "ES256K");
    }

    #[test]
    fn invalid_jwk_rejected() {
        let mut jwk = golden_jwk();
        jwk.kty = "RSA".to_string();
        assert!(jwk.curve().is_err());
        assert!(jwk.to_sec1_uncompressed().is_err());

        let mut jwk = golden_jwk();
        jwk.crv = "P-384".to_string();
        assert!(jwk.to_sec1_uncompressed().is_err());

        let mut jwk = golden_jwk();
        jwk.x = "!not-base64!".to_string();
        assert!(jwk.to_sec1_uncompressed().is_err());

        let mut jwk = golden_jwk();
        jwk.y = URL_SAFE_NO_PAD.encode([1u8; 16]);
        assert!(jwk.to_sec1_uncompressed().is_err());

        let mut jwk = golden_jwk();
        jwk.y = URL_SAFE_NO_PAD.encode([1u8; 32]);
        let err = jwk.to_sec1_uncompressed().unwrap_err();
        assert!(err.error_description().starts_with("invalid EC point"));

        let mut jwk = golden_jwk();
        jwk.crv = "secp256k1".to_string();
        assert!(jwk.to_sec1_uncompressed().is_err());
    }

    #[test]
    fn from_sec1_rejects_garbage() {
        assert!(Jwk::from_sec1(EcCurve::P256, &[0x04, 0x01]).is_err());
        assert!(Jwk::from_sec1(EcCurve::K256, &[0x04, 0x01]).is_err());
    }

    #[test]
    fn jwk_set_find_by_kid() {
        let mut a = golden_jwk();
        a.kid = Some("a".to_string());
        let b = golden_jwk();
        let set = JwkSet {
            keys: vec![a.clone(), b],
        };
        assert_eq!(set.find_by_kid("a"), Some(&a));
        assert!(set.find_by_kid("missing").is_none());
        let parsed: JwkSet = serde_json::from_str(&serde_json::to_string(&set).unwrap()).unwrap();
        assert_eq!(parsed, set);
    }
}
