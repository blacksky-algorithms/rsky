use serde::{Deserialize, Serialize};

/// Allowed key usage types for JWKs
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyUsage {
    Sign,
    Verify,
    Encrypt,
    Decrypt,
    WrapKey,
    UnwrapKey,
    DeriveKey,
    DeriveBits,
}

/// Base JWK parameters common to all key types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JwkBase {
    /// Key type
    pub kty: String,

    /// Algorithm (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alg: Option<String>,

    /// Key ID (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kid: Option<String>,

    /// Extractable flag (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ext: Option<bool>,

    /// Key usage (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_: Option<KeyUse>,

    /// Key operations (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_ops: Option<Vec<KeyUsage>>,

    /// X.509 certificate chain (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x5c: Option<Vec<String>>,

    /// X.509 thumbprint SHA-1 (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x5t: Option<String>,

    /// X.509 thumbprint SHA-256 (optional)
    #[serde(rename = "x5t#S256", skip_serializing_if = "Option::is_none")]
    pub x5t_s256: Option<String>,

    /// X.509 URL (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x5u: Option<String>,
}

/// Key usage type (sig or enc)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyUse {
    Sig,
    Enc,
}

/// RSA key parameters
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RsaKeyParameters {
    /// Modulus
    pub n: String,
    /// Public exponent
    pub e: String,
    /// Private exponent (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub d: Option<String>,
    /// First prime factor (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p: Option<String>,
    /// Second prime factor (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,
    /// First CRT coefficient (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dp: Option<String>,
    /// Second CRT coefficient (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dq: Option<String>,
    /// First CRT exponent (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qi: Option<String>,
    /// Other prime info (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oth: Option<Vec<OtherPrimeInfo>>,
}

/// Other prime info for RSA keys
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OtherPrimeInfo {
    /// Prime factor
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r: Option<String>,
    /// Factor CRT exponent
    #[serde(skip_serializing_if = "Option::is_none")]
    pub d: Option<String>,
    /// Factor CRT coefficient
    #[serde(skip_serializing_if = "Option::is_none")]
    pub t: Option<String>,
}

/// EC key parameters
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EcKeyParameters {
    /// Curve
    pub crv: EcCurve,
    /// X coordinate
    pub x: String,
    /// Y coordinate
    pub y: String,
    /// Private value (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub d: Option<String>,
}

/// Supported EC curves
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EcCurve {
    #[serde(rename = "P-256")]
    P256,
    #[serde(rename = "P-384")]
    P384,
    #[serde(rename = "P-521")]
    P521,
    #[serde(rename = "secp256k1")]
    Secp256k1,
}

/// OKP key parameters
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OkpKeyParameters {
    /// Curve
    pub crv: OkpCurve,
    /// Public key
    pub x: String,
    /// Private key (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub d: Option<String>,
}

/// Supported OKP curves
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OkpCurve {
    Ed25519,
    Ed448,
}

/// Symmetric key parameters
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymmetricKeyParameters {
    /// Key value (base64url encoded)
    pub k: String,
}

/// JWK types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kty")]
pub enum Jwk {
    #[serde(rename = "RSA")]
    Rsa {
        #[serde(flatten)]
        base: JwkBase,
        #[serde(flatten)]
        params: RsaKeyParameters,
    },

    #[serde(rename = "EC")]
    Ec {
        #[serde(flatten)]
        base: JwkBase,
        #[serde(flatten)]
        params: EcKeyParameters,
    },

    #[serde(rename = "OKP")]
    Okp {
        #[serde(flatten)]
        base: JwkBase,
        #[serde(flatten)]
        params: OkpKeyParameters,
    },

    #[serde(rename = "oct")]
    Symmetric {
        #[serde(flatten)]
        base: JwkBase,
        #[serde(flatten)]
        params: SymmetricKeyParameters,
    },
}

impl Jwk {
    /// Check if the key is a private key
    pub fn is_private(&self) -> bool {
        match self {
            Jwk::Rsa { params, .. } => params.d.is_some(),
            Jwk::Ec { params, .. } => params.d.is_some(),
            Jwk::Okp { params, .. } => params.d.is_some(),
            Jwk::Symmetric { .. } => true,
        }
    }

    /// Get the key usage
    pub fn key_use(&self) -> Option<&KeyUse> {
        match self {
            Jwk::Rsa { base, .. }
            | Jwk::Ec { base, .. }
            | Jwk::Okp { base, .. }
            | Jwk::Symmetric { base, .. } => base.use_.as_ref(),
        }
    }

    /// Get the key operations
    pub fn key_ops(&self) -> Option<&Vec<KeyUsage>> {
        match self {
            Jwk::Rsa { base, .. }
            | Jwk::Ec { base, .. }
            | Jwk::Okp { base, .. }
            | Jwk::Symmetric { base, .. } => base.key_ops.as_ref(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ec_key_serialization() {
        let jwk = Jwk::Ec {
            base: JwkBase {
                kty: "EC".to_string(),
                alg: Some("ES256".to_string()),
                kid: Some("key-1".to_string()),
                ext: None,
                use_: Some(KeyUse::Sig),
                key_ops: None,
                x5c: None,
                x5t: None,
                x5t_s256: None,
                x5u: None,
            },
            params: EcKeyParameters {
                crv: EcCurve::P256,
                x: "x-coord".to_string(),
                y: "y-coord".to_string(),
                d: Some("private-key".to_string()),
            },
        };

        let serialized = serde_json::to_value(&jwk).unwrap();

        assert_eq!(serialized["kty"], "EC");
        assert_eq!(serialized["crv"], "P-256");
        assert_eq!(serialized["x"], "x-coord");
        assert_eq!(serialized["y"], "y-coord");
        assert_eq!(serialized["d"], "private-key");
    }

    #[test]
    fn test_is_private() {
        let private_key = Jwk::Rsa {
            base: JwkBase {
                kty: "RSA".to_string(),
                alg: None,
                kid: None,
                ext: None,
                use_: None,
                key_ops: None,
                x5c: None,
                x5t: None,
                x5t_s256: None,
                x5u: None,
            },
            params: RsaKeyParameters {
                n: "modulus".to_string(),
                e: "exponent".to_string(),
                d: Some("private_exponent".to_string()),
                p: None,
                q: None,
                dp: None,
                dq: None,
                qi: None,
                oth: None,
            },
        };

        let public_key = Jwk::Rsa {
            base: JwkBase {
                kty: "RSA".to_string(),
                alg: None,
                kid: None,
                ext: None,
                use_: None,
                key_ops: None,
                x5c: None,
                x5t: None,
                x5t_s256: None,
                x5u: None,
            },
            params: RsaKeyParameters {
                n: "modulus".to_string(),
                e: "exponent".to_string(),
                d: None,
                p: None,
                q: None,
                dp: None,
                dq: None,
                qi: None,
                oth: None,
            },
        };

        assert!(private_key.is_private());
        assert!(!public_key.is_private());
    }
}
