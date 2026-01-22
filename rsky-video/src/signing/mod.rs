//! Service authentication token signing
//!
//! The video service needs to create service auth tokens to upload blobs
//! to users' PDS instances. This module handles loading the signing key
//! and creating properly signed JWTs.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use k256::ecdsa::{SigningKey, Signature, signature::Signer};
use k256::pkcs8::DecodePrivateKey;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use tracing::{debug, info};

use crate::error::{Error, Result};

/// JWT header for ES256K (secp256k1)
#[derive(Debug, Serialize)]
struct JwtHeader {
    alg: &'static str,
    typ: &'static str,
}

/// Service auth token claims
#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceAuthClaims {
    /// Issued at timestamp (seconds since epoch)
    pub iat: i64,
    /// Expiration timestamp (seconds since epoch)
    pub exp: i64,
    /// Issuer - the video service DID
    pub iss: String,
    /// Audience - the target PDS DID
    pub aud: String,
    /// Subject - the user's DID (on whose behalf we're acting)
    pub sub: String,
    /// Lexicon method being called
    pub lxm: String,
    /// Unique token ID
    pub jti: String,
}

/// Signer for creating service auth tokens
pub struct ServiceAuthSigner {
    signing_key: SigningKey,
    service_did: String,
}

impl ServiceAuthSigner {
    /// Load the signing key from a PEM file
    pub fn from_pem_file<P: AsRef<Path>>(path: P, service_did: String) -> Result<Self> {
        let pem_content = fs::read_to_string(&path)
            .map_err(|e| Error::Internal(format!("Failed to read signing key: {}", e)))?;

        // Parse EC private key in PEM format
        // First try PKCS#8 format, then fall back to SEC1 format
        let signing_key = SigningKey::from_pkcs8_pem(&pem_content)
            .or_else(|_| {
                // Try SEC1 format (EC PRIVATE KEY)
                use k256::SecretKey;
                SecretKey::from_sec1_pem(&pem_content)
                    .map(|sk| SigningKey::from(sk))
            })
            .map_err(|e| Error::Internal(format!("Failed to parse signing key: {}", e)))?;

        info!("Loaded signing key for {}", service_did);

        Ok(Self {
            signing_key,
            service_did,
        })
    }

    /// Create a service auth token for uploading a blob to a PDS
    ///
    /// # Arguments
    /// * `pds_did` - The DID of the target PDS
    /// * `user_did` - The DID of the user on whose behalf we're acting
    /// * `ttl_seconds` - How long the token should be valid (default: 300s / 5min)
    pub fn create_pds_upload_token(
        &self,
        pds_did: &str,
        user_did: &str,
        ttl_seconds: Option<i64>,
    ) -> Result<String> {
        let now = chrono::Utc::now().timestamp();
        let ttl = ttl_seconds.unwrap_or(300); // 5 minutes default

        let claims = ServiceAuthClaims {
            iat: now,
            exp: now + ttl,
            iss: self.service_did.clone(),
            aud: pds_did.to_string(),
            sub: user_did.to_string(),
            lxm: "com.atproto.repo.uploadBlob".to_string(),
            jti: uuid::Uuid::new_v4().to_string(),
        };

        debug!(
            "Creating service auth token: iss={}, aud={}, sub={}",
            claims.iss, claims.aud, claims.sub
        );

        self.sign_jwt(&claims)
    }

    /// Sign a JWT with the service's private key
    fn sign_jwt(&self, claims: &ServiceAuthClaims) -> Result<String> {
        // Create header
        let header = JwtHeader {
            alg: "ES256K",
            typ: "JWT",
        };

        // Encode header and payload
        let header_json = serde_json::to_string(&header)
            .map_err(|e| Error::Internal(format!("Failed to serialize header: {}", e)))?;
        let claims_json = serde_json::to_string(claims)
            .map_err(|e| Error::Internal(format!("Failed to serialize claims: {}", e)))?;

        let header_b64 = URL_SAFE_NO_PAD.encode(header_json.as_bytes());
        let claims_b64 = URL_SAFE_NO_PAD.encode(claims_json.as_bytes());

        // Create signing input
        let signing_input = format!("{}.{}", header_b64, claims_b64);

        // Sign with secp256k1
        let signature: Signature = self.signing_key.sign(signing_input.as_bytes());
        let sig_bytes = signature.to_bytes();
        let sig_b64 = URL_SAFE_NO_PAD.encode(&sig_bytes);

        // Combine into JWT
        Ok(format!("{}.{}", signing_input, sig_b64))
    }
}
