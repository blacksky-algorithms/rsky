//! HTTP Response MAC header parsing and ECDH shared secret derivation.
//!
//! This module handles:
//! - Parsing `Atproto-Space-Want-Mac` request headers
//! - Formatting `Atproto-Space-Mac` response headers
//! - P-256 did:key encoding/decoding
//! - ECDH shared secret derivation per RFC 9180 (HPKE KEM)
//!
//! ## Usage in API Endpoints
//!
//! To add MAC support to an endpoint:
//!
//! 1. Add `want_mac: WantMacHeader` parameter to the handler
//! 2. Compute MAC using `compute_response_mac` with a `KemKeyProvider`
//! 3. Return response with MAC header using the helper functions
//!
//! Example:
//! ```ignore
//! #[rocket::get("/xrpc/com.atproto.space.getRecord?<space>&<repo>&...")]
//! pub async fn space_get_record(
//!     space: String,
//!     repo: String,
//!     want_mac: WantMacHeader,  // Optional MAC request
//!     // ... other params
//! ) -> Result<MacResponse<Json<GetRecordOutput>>, ApiError> {
//!     // ... compute record ...
//!
//!     // Compute MAC if requested
//!     let mac = if let Some(ref want) = want_mac.0 {
//!         compute_response_mac(&kem_provider, want, &repo, &space, ...)
//!             .await
//!             .ok()
//!     } else {
//!         None
//!     };
//!
//!     Ok(MacResponse::new(Json(output), mac, want_mac))
//! }
//! ```

use base64::Engine;
use hkdf::Hkdf;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::{ecdh::diffie_hellman, PublicKey, SecretKey};
use rocket::request::{FromRequest, Outcome, Request};
use sha2::Sha256;

/// Parsed `Atproto-Space-Want-Mac` header.
#[derive(Debug, Clone)]
pub struct WantMac {
    /// MAC type: "atproto-space-repo" or "atproto-space-record"
    pub mac_type: MacType,
    /// Protocol version (currently 1)
    pub version: u32,
    /// Syncer's ephemeral P-256 public key as did:key
    pub key_share: String,
}

/// The type of MAC requested/provided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacType {
    /// Commit MAC for getLatestCommit, getRepo, listRepoOps
    Commit,
    /// Record MAC for getRecord
    Record,
}

impl MacType {
    /// Returns the header type string (without version suffix).
    pub fn as_header_str(&self) -> &'static str {
        match self {
            MacType::Commit => "atproto-space-repo",
            MacType::Record => "atproto-space-record",
        }
    }

    /// Returns the expected MAC type for a given method.
    pub fn expected_for_method(method: &str) -> Self {
        match method {
            "getRecord" => MacType::Record,
            "getLatestCommit" | "getRepo" | "listRepoOps" => MacType::Commit,
            _ => MacType::Commit, // Default, should not happen
        }
    }
}

impl WantMac {
    /// Parse from header value.
    ///
    /// Format: `type="atproto-space-record", ver=1, key_share="did:key:zDna..."`
    ///
    /// The `ver` parameter is optional and defaults to 1 if not present.
    pub fn parse(header: &str) -> Result<Self, MacError> {
        // Parse as comma-separated key=value pairs
        // Handle both quoted and unquoted values
        let mut mac_type_str: Option<&str> = None;
        let mut version_i64: i64 = 1; // Default to 1 if not specified
        let mut key_share: Option<String> = None;

        for part in header.split(',') {
            let part = part.trim();
            if let Some((key, value)) = part.split_once('=') {
                let key = key.trim();
                let value = value.trim().trim_matches('"');

                match key {
                    "type" => mac_type_str = Some(value),
                    "ver" => {
                        version_i64 = value.parse().map_err(|_| MacError::InvalidHeader)?;
                    }
                    "key_share" => key_share = Some(value.to_string()),
                    _ => {} // Ignore unknown fields
                }
            }
        }

        // Validate mac_type
        let mac_type = match mac_type_str {
            Some("atproto-space-repo") => MacType::Commit,
            Some("atproto-space-record") => MacType::Record,
            Some(other) => return Err(MacError::UnsupportedType(other.to_string())),
            None => return Err(MacError::InvalidHeader),
        };

        // Validate version (must be exactly 1)
        if version_i64 != 1 {
            return Err(MacError::UnsupportedVersion(version_i64));
        }
        let version = version_i64 as u32;

        // Validate key_share
        let key_share = key_share.ok_or(MacError::InvalidHeader)?;
        if !key_share.starts_with("did:key:z") {
            return Err(MacError::InvalidKeyShare(key_share));
        }

        Ok(WantMac {
            mac_type,
            version,
            key_share,
        })
    }

    /// Validate that the MAC type matches the expected type for the method.
    pub fn validate_type_for_method(&self, method: &str) -> Result<(), MacError> {
        let expected = MacType::expected_for_method(method);
        if self.mac_type != expected {
            return Err(MacError::TypeMismatch {
                expected,
                actual: self.mac_type,
            });
        }
        Ok(())
    }
}

/// Format `Atproto-Space-Mac` response header value.
///
/// Format: `type="atproto-space-record", ver=1, mac=:base64encoded:`
pub fn format_mac_header(mac_type: MacType, version: u32, mac: &[u8; 32]) -> String {
    format!(
        "type=\"{}\", ver={}, mac=:{}:",
        mac_type.as_header_str(),
        version,
        base64::engine::general_purpose::STANDARD.encode(mac)
    )
}

/// Parse MAC value from response header (for verification/testing).
///
/// Extracts the base64-encoded MAC bytes from the header.
pub fn parse_mac_from_header(header: &str) -> Result<[u8; 32], MacError> {
    // Find mac=:...:
    let mac_start = header.find("mac=:").ok_or(MacError::InvalidHeader)? + 5;
    let mac_end = header[mac_start..]
        .find(':')
        .ok_or(MacError::InvalidHeader)?
        + mac_start;

    let mac_b64 = &header[mac_start..mac_end];
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(mac_b64)
        .map_err(|_| MacError::InvalidHeader)?;

    decoded.try_into().map_err(|_| MacError::InvalidHeader)
}

/// Parse P-256 public key from did:key format.
///
/// P-256 did:key uses the `zDna` multibase prefix (base58btc).
/// Multicodec prefix for P-256 public key (compressed) is 0x1200.
/// In varint encoding: 0x80 (0 with continuation bit), 0x24 (36 = 0x1200 >> 7).
pub fn parse_p256_did_key(did_key: &str) -> Result<PublicKey, MacError> {
    // Verify prefix
    let key_part = did_key
        .strip_prefix("did:key:")
        .ok_or_else(|| MacError::InvalidKeyShare(did_key.to_string()))?;

    // Verify P-256 multibase prefix 'z' (base58btc) and 'Dna' start
    if !key_part.starts_with("zDna") {
        return Err(MacError::InvalidKeyShare(format!(
            "Expected P-256 did:key (zDna prefix), got: {}",
            key_part
        )));
    }

    // Decode base58btc (strip 'z' prefix)
    let decoded = bs58::decode(&key_part[1..])
        .into_vec()
        .map_err(|_| MacError::InvalidKeyShare("Invalid base58".to_string()))?;

    // Verify multicodec prefix for P-256 public key (compressed)
    // 0x1200 in varint: [0x80, 0x24] (0 with continuation, then 36)
    if decoded.len() < 2 || decoded[0] != 0x80 || decoded[1] != 0x24 {
        return Err(MacError::InvalidKeyShare(
            "Invalid multicodec prefix (expected P-256: 0x80 0x24)".to_string(),
        ));
    }

    // Remaining bytes are compressed P-256 point (33 bytes)
    let point_bytes = &decoded[2..];
    if point_bytes.len() != 33 {
        return Err(MacError::InvalidKeyShare(format!(
            "Expected 33-byte compressed point, got {} bytes",
            point_bytes.len()
        )));
    }

    // Parse as P-256 public key
    PublicKey::from_sec1_bytes(point_bytes)
        .map_err(|_| MacError::InvalidKeyShare("Invalid P-256 point".to_string()))
}

/// Encode P-256 public key as did:key format.
///
/// Used for creating ephemeral key shares.
pub fn encode_p256_did_key(public_key: &PublicKey) -> String {
    // Compress the public key (33 bytes)
    let compressed = public_key.to_encoded_point(true);

    // Build multicodec-prefixed bytes
    let mut bytes = vec![0x80, 0x24]; // P-256 multicodec varint
    bytes.extend_from_slice(compressed.as_bytes());

    // Encode as base58btc with 'z' prefix
    format!("did:key:z{}", bs58::encode(&bytes).into_string())
}

/// Derive shared secret for MAC computation (host side).
///
/// The PDS (host) performs: `ECDH(author_kem_private, syncer_public)`
pub fn derive_shared_secret(
    author_kem_private: &[u8], // Author's KEM private key (raw 32-byte scalar)
    syncer_public_did_key: &str, // did:key from Want-Mac header
) -> Result<[u8; 32], MacError> {
    // 1. Parse syncer's public key from did:key
    let syncer_public = parse_p256_did_key(syncer_public_did_key)?;

    // 2. Parse author's private key (assuming raw 32-byte scalar)
    let author_private =
        SecretKey::from_bytes(author_kem_private.into()).map_err(|_| MacError::InvalidKey)?;

    // 3. Perform ECDH: dh = ECDH(author_private, syncer_public)
    let dh = diffie_hellman(
        author_private.to_nonzero_scalar(),
        syncer_public.as_affine(),
    );

    // 4. Derive shared secret via HKDF (RFC 9180 §4.1)
    let shared_secret = hpke_kem_derive(
        dh.raw_secret_bytes(),
        &syncer_public,
        &author_private.public_key(),
    )?;

    Ok(shared_secret)
}

/// Derive shared secret for MAC verification (syncer side).
///
/// The syncer performs: `ECDH(syncer_ephemeral_private, author_kem_public)`
pub fn derive_shared_secret_syncer(
    syncer_private: &SecretKey,
    author_public: &PublicKey,
) -> Result<[u8; 32], MacError> {
    // Perform ECDH: dh = ECDH(syncer_private, author_public)
    let dh = diffie_hellman(
        syncer_private.to_nonzero_scalar(),
        author_public.as_affine(),
    );

    // Derive shared secret via HKDF (RFC 9180 §4.1)
    // Note: syncer_public comes first in kem_context (enc || pkR)
    let shared_secret = hpke_kem_derive(
        dh.raw_secret_bytes(),
        &syncer_private.public_key(),
        author_public,
    )?;

    Ok(shared_secret)
}

/// HPKE KEM shared secret derivation per RFC 9180 §4.1.
///
/// ```text
/// kem_context = syncer_public || author_public  (uncompressed, 65 bytes each)
/// prefix = "HPKE-v1" || "KEM" || uint16be(0x0010)
/// ikm = prefix || "eae_prk" || dh
/// info = uint16be(32) || prefix || "shared_secret" || kem_context
/// shared_secret = HKDF-SHA256(salt="", ikm=ikm, info=info, length=32)
/// ```
fn hpke_kem_derive(
    dh: &[u8],
    syncer_public: &PublicKey,
    author_public: &PublicKey,
) -> Result<[u8; 32], MacError> {
    // Uncompressed point encoding: 0x04 || x || y (65 bytes)
    let syncer_key_bytes = syncer_public.to_encoded_point(false);
    let author_key_bytes = author_public.to_encoded_point(false);

    // kem_context = syncer_key_bytes || author_key_bytes (RFC 9180: enc || pkR)
    let mut kem_context = Vec::with_capacity(130);
    kem_context.extend_from_slice(syncer_key_bytes.as_bytes());
    kem_context.extend_from_slice(author_key_bytes.as_bytes());

    // prefix = "HPKE-v1" || "KEM" || uint16be(0x0010)
    let mut prefix = Vec::with_capacity(13);
    prefix.extend_from_slice(b"HPKE-v1");
    prefix.extend_from_slice(b"KEM");
    prefix.extend_from_slice(&0x0010u16.to_be_bytes());

    // ikm = prefix || "eae_prk" || dh
    let mut ikm = prefix.clone();
    ikm.extend_from_slice(b"eae_prk");
    ikm.extend_from_slice(dh);

    // info = uint16be(32) || prefix || "shared_secret" || kem_context
    let mut info = Vec::new();
    info.extend_from_slice(&32u16.to_be_bytes());
    info.extend_from_slice(&prefix);
    info.extend_from_slice(b"shared_secret");
    info.extend_from_slice(&kem_context);

    // HKDF-SHA256: Extract with empty salt, then Expand
    let hk = Hkdf::<Sha256>::new(None, &ikm);
    let mut okm = [0u8; 32];
    hk.expand(&info, &mut okm)
        .map_err(|_| MacError::HkdfError)?;

    Ok(okm)
}

/// Errors that can occur during MAC header processing.
#[derive(Debug, thiserror::Error)]
pub enum MacError {
    #[error("invalid Want-Mac header format")]
    InvalidHeader,

    #[error("unsupported MAC type: {0}")]
    UnsupportedType(String),

    #[error("unsupported MAC version: {0} (only version 1 is supported)")]
    UnsupportedVersion(i64),

    #[error("invalid key_share: {0}")]
    InvalidKeyShare(String),

    #[error("invalid key format")]
    InvalidKey,

    #[error("HKDF derivation failed")]
    HkdfError,

    #[error("KEM key not available for author")]
    KemKeyNotAvailable,

    #[error("MAC type mismatch: expected {expected:?}, got {actual:?}")]
    TypeMismatch { expected: MacType, actual: MacType },

    #[error("invalid CID: {0}")]
    InvalidCid(String),
}

// =============================================================================
// Phase 3: KEM Key Access Interface
// =============================================================================

/// Trait for retrieving KEM private keys.
///
/// The author's KEM private key is a P-256 key used for ECDH with the syncer's
/// ephemeral public key. This trait abstracts the storage mechanism.
///
/// **Implementation Note**: This trait needs to be implemented by the actor
/// store or a dedicated key service. The KEM key is distinct from the signing
/// key (secp256k1) and must be stored separately.
#[async_trait::async_trait]
pub trait KemKeyProvider: Send + Sync {
    /// Retrieve the KEM private key for the given author DID.
    ///
    /// Returns the raw 32-byte P-256 scalar, or an error if not available.
    async fn get_kem_key(&self, author_did: &str) -> Result<[u8; 32], MacError>;
}

/// Stub KEM key provider that always returns an error.
///
/// This is used when KEM key infrastructure is not yet implemented.
/// Replace with actual implementation when KEM keys are stored.
pub struct StubKemKeyProvider;

#[async_trait::async_trait]
impl KemKeyProvider for StubKemKeyProvider {
    async fn get_kem_key(&self, _author_did: &str) -> Result<[u8; 32], MacError> {
        // TODO: Implement actual KEM key retrieval from actor store or key service
        // The KEM key should be stored alongside or separately from the signing key
        // and retrieved based on the author's DID.
        Err(MacError::KemKeyNotAvailable)
    }
}

/// Compute MAC for an HTTP response using the KEM key provider.
///
/// This is the main entry point for MAC computation in API handlers.
///
/// For `getRecord` (MacType::Record):
///   - `space`: Space URI
///   - `repo`: Author DID
///   - `record_uri`: Full record URI (`{space}/{repo}/{collection}/{rkey}`)
///   - `cid_bytes`: Raw binary CID of the record
///
/// For `getLatestCommit`, `getRepo`, `listRepoOps` (MacType::Commit):
///   - `space`: Space URI
///   - `repo`: Author DID
///   - `rev`: Commit revision (TID)
///   - `data`: 32-byte SHA-256 of LtHash state (commit.hash)
#[allow(clippy::too_many_arguments)]
pub async fn compute_response_mac<K: KemKeyProvider>(
    kem_provider: &K,
    want_mac: &WantMac,
    author_did: &str,
    space: &str,
    repo: &str,
    rev: Option<&str>,
    data: Option<&[u8; 32]>,
    record_uri: Option<&str>,
    cid_bytes: Option<&[u8]>,
) -> Result<[u8; 32], MacError> {
    // Get author's KEM private key
    let kem_key = kem_provider.get_kem_key(author_did).await?;

    // Derive shared secret
    let shared_secret = derive_shared_secret(&kem_key, &want_mac.key_share)?;

    // Compute MAC based on type
    match want_mac.mac_type {
        MacType::Commit => {
            let rev = rev.ok_or(MacError::InvalidHeader)?;
            let data = data.ok_or(MacError::InvalidHeader)?;
            Ok(rsky_space::compute_response_commit_mac(
                &shared_secret,
                space,
                repo,
                rev,
                data,
            ))
        }
        MacType::Record => {
            let uri = record_uri.ok_or(MacError::InvalidHeader)?;
            let cid = cid_bytes.ok_or(MacError::InvalidHeader)?;
            Ok(rsky_space::compute_response_record_mac(
                &shared_secret,
                uri,
                cid,
            ))
        }
    }
}

/// Helper to append to the Vary header without overwriting existing values.
pub fn append_vary_header(existing: Option<&str>) -> String {
    const WANT_MAC_HEADER: &str = "Atproto-Space-Want-Mac";
    match existing {
        Some(v) if !v.is_empty() => format!("{}, {}", v, WANT_MAC_HEADER),
        _ => WANT_MAC_HEADER.to_string(),
    }
}

// =============================================================================
// Rocket Request Guard and Response Wrapper
// =============================================================================

/// Request header name for MAC requests.
pub const WANT_MAC_HEADER_NAME: &str = "Atproto-Space-Want-Mac";

/// Response header name for MAC values.
pub const MAC_HEADER_NAME: &str = "Atproto-Space-Mac";

/// Request guard that optionally parses the `Atproto-Space-Want-Mac` header.
///
/// This guard always succeeds - if the header is missing or invalid, `self.0`
/// is `None`. This allows endpoints to work with or without MAC support.
///
/// Usage in handlers:
/// ```ignore
/// pub async fn my_handler(want_mac: WantMacHeader) -> ... {
///     if let Some(ref want) = want_mac.0 {
///         // MAC was requested - compute and include it
///     }
/// }
/// ```
#[derive(Debug, Clone)]
pub struct WantMacHeader(pub Option<WantMac>);

impl WantMacHeader {
    /// Check if MAC was requested.
    pub fn is_requested(&self) -> bool {
        self.0.is_some()
    }

    /// Get the parsed header if available.
    pub fn get(&self) -> Option<&WantMac> {
        self.0.as_ref()
    }
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for WantMacHeader {
    type Error = std::convert::Infallible;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        // Get header value if present
        let header_value = req.headers().get_one(WANT_MAC_HEADER_NAME);

        let want_mac = match header_value {
            Some(value) => match WantMac::parse(value) {
                Ok(parsed) => Some(parsed),
                Err(e) => {
                    tracing::debug!(header = %value, error = %e, "Failed to parse Want-Mac header");
                    None
                }
            },
            None => None,
        };

        Outcome::Success(WantMacHeader(want_mac))
    }
}

/// Response wrapper that adds MAC header if computed.
///
/// This wraps any response type and adds:
/// - `Atproto-Space-Mac` header with the MAC value (if provided)
/// - `Vary: Atproto-Space-Want-Mac` header (always)
///
/// Usage:
/// ```ignore
/// Ok(MacResponse::new(Json(output), Some(mac_bytes), &want_mac))
/// ```
#[derive(Debug)]
pub struct MacResponse<R> {
    inner: R,
    mac: Option<([u8; 32], MacType)>,
    version: u32,
}

impl<R> MacResponse<R> {
    /// Create a new MAC response.
    ///
    /// - `inner`: The actual response content
    /// - `mac`: Optional MAC bytes if computed successfully
    /// - `want_mac`: The original request header (for mac type and version)
    pub fn new(inner: R, mac: Option<[u8; 32]>, want_mac: &WantMacHeader) -> Self {
        let (mac_with_type, version) = match (mac, want_mac.get()) {
            (Some(m), Some(w)) => (Some((m, w.mac_type)), w.version),
            _ => (None, 1),
        };
        Self {
            inner,
            mac: mac_with_type,
            version,
        }
    }

    /// Create a response without MAC (when MAC wasn't requested or computation failed).
    pub fn without_mac(inner: R) -> Self {
        Self {
            inner,
            mac: None,
            version: 1,
        }
    }
}

impl<'r, 'o: 'r, R: rocket::response::Responder<'r, 'o>> rocket::response::Responder<'r, 'o>
    for MacResponse<R>
{
    fn respond_to(self, req: &'r Request<'_>) -> rocket::response::Result<'o> {
        let mut response = self.inner.respond_to(req)?;

        // Always add Vary header for caching
        response.set_header(rocket::http::Header::new(
            "Vary",
            append_vary_header(response.headers().get_one("Vary")),
        ));

        // Add MAC header if computed
        if let Some((mac, mac_type)) = self.mac {
            response.set_header(rocket::http::Header::new(
                MAC_HEADER_NAME,
                format_mac_header(mac_type, self.version, &mac),
            ));
        }

        Ok(response)
    }
}

/// Convenience function to build record URI for MAC computation.
///
/// Format: `{space}/{repo}/{collection}/{rkey}`
pub fn build_record_uri(space: &str, repo: &str, collection: &str, rkey: &str) -> String {
    format!("{}/{}/{}/{}", space, repo, collection, rkey)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::thread_rng;

    #[test]
    fn parse_want_mac_valid() {
        let header = r#"type="atproto-space-record", ver=1, key_share="did:key:zDnaerDaTF5BXEavCrfRZEk316dpbLsfPDZ3WJ5hRTPFU2169""#;
        let want_mac = WantMac::parse(header).unwrap();
        assert_eq!(want_mac.mac_type, MacType::Record);
        assert_eq!(want_mac.version, 1);
        assert!(want_mac.key_share.starts_with("did:key:zDna"));
    }

    #[test]
    fn parse_want_mac_missing_version_defaults_to_1() {
        // Generate a valid P-256 did:key for testing
        let secret = SecretKey::random(&mut thread_rng());
        let did_key = encode_p256_did_key(&secret.public_key());

        let header = format!(r#"type="atproto-space-record", key_share="{}""#, did_key);
        let want_mac = WantMac::parse(&header).unwrap();
        assert_eq!(want_mac.version, 1);
    }

    #[test]
    fn parse_want_mac_version_zero() {
        let header = r#"type="atproto-space-repo", ver=0, key_share="did:key:zDnatest""#;
        let result = WantMac::parse(header);
        assert!(matches!(result, Err(MacError::UnsupportedVersion(0))));
    }

    #[test]
    fn parse_want_mac_version_negative() {
        let header = r#"type="atproto-space-repo", ver=-1, key_share="did:key:zDnatest""#;
        let result = WantMac::parse(header);
        assert!(matches!(result, Err(MacError::UnsupportedVersion(-1))));
    }

    #[test]
    fn parse_want_mac_unknown_type() {
        let header = r#"type="unknown-type", ver=1, key_share="did:key:zDnatest""#;
        let result = WantMac::parse(header);
        assert!(matches!(result, Err(MacError::UnsupportedType(_))));
    }

    #[test]
    fn validate_type_for_method() {
        let want_mac = WantMac {
            mac_type: MacType::Record,
            version: 1,
            key_share: "did:key:zDnatest".into(),
        };

        assert!(want_mac.validate_type_for_method("getRecord").is_ok());
        assert!(want_mac
            .validate_type_for_method("getLatestCommit")
            .is_err());
    }

    #[test]
    fn format_mac_header_output() {
        let mac = [0xAA; 32];
        let header = format_mac_header(MacType::Record, 1, &mac);
        assert!(header.starts_with("type=\"atproto-space-record\""));
        assert!(header.contains("ver=1"));
        assert!(header.contains("mac=:"));
        assert!(header.ends_with(':'));
    }

    #[test]
    fn parse_mac_from_header_roundtrip() {
        let original_mac = [0xBB; 32];
        let header = format_mac_header(MacType::Commit, 1, &original_mac);
        let parsed_mac = parse_mac_from_header(&header).unwrap();
        assert_eq!(original_mac, parsed_mac);
    }

    #[test]
    fn parse_p256_did_key_valid() {
        // Generate a random test key (guaranteed valid scalar)
        let secret = SecretKey::random(&mut thread_rng());
        let public = secret.public_key();
        let did_key = encode_p256_did_key(&public);

        // Parse it back
        let parsed = parse_p256_did_key(&did_key).unwrap();
        assert_eq!(public, parsed);
    }

    #[test]
    fn parse_p256_did_key_rejects_ed25519() {
        // Ed25519 key has z6Mk prefix - rejected at prefix check
        let ed_key = "did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK";
        let result = parse_p256_did_key(ed_key);
        assert!(matches!(result, Err(MacError::InvalidKeyShare(_))));
    }

    #[test]
    fn parse_p256_did_key_rejects_secp256k1() {
        // secp256k1 key has zQ3s prefix (multicodec 0xe7) - rejected at prefix check
        let secp_key = "did:key:zQ3shokFTS3brHcDQrn82RUDfCZESWL1a7y5hRTPFU2169";
        let result = parse_p256_did_key(secp_key);
        assert!(matches!(result, Err(MacError::InvalidKeyShare(_))));
    }

    #[test]
    fn encode_decode_p256_did_key_roundtrip() {
        let secret = SecretKey::random(&mut thread_rng());
        let public = secret.public_key();

        let did_key = encode_p256_did_key(&public);
        assert!(did_key.starts_with("did:key:zDna"));

        let decoded = parse_p256_did_key(&did_key).unwrap();
        assert_eq!(public, decoded);
    }

    #[test]
    fn ecdh_shared_secret_is_symmetric() {
        // Generate two random keypairs (guaranteed valid scalars)
        let alice_private = SecretKey::random(&mut thread_rng());
        let bob_private = SecretKey::random(&mut thread_rng());

        let alice_public = alice_private.public_key();
        let bob_public = bob_private.public_key();

        // ECDH both directions
        let dh_ab = diffie_hellman(alice_private.to_nonzero_scalar(), bob_public.as_affine());
        let dh_ba = diffie_hellman(bob_private.to_nonzero_scalar(), alice_public.as_affine());

        assert_eq!(dh_ab.raw_secret_bytes(), dh_ba.raw_secret_bytes());
    }

    #[test]
    fn shared_secret_host_and_syncer_match() {
        // Simulate the full flow with random keys (guaranteed valid scalars)
        let author_private = SecretKey::random(&mut thread_rng());
        let syncer_private = SecretKey::random(&mut thread_rng());

        let author_public = author_private.public_key();
        let syncer_did_key = encode_p256_did_key(&syncer_private.public_key());

        // Host derives shared secret
        let host_secret =
            derive_shared_secret(author_private.to_bytes().as_slice(), &syncer_did_key).unwrap();

        // Syncer derives shared secret
        let syncer_secret = derive_shared_secret_syncer(&syncer_private, &author_public).unwrap();

        assert_eq!(host_secret, syncer_secret);
    }

    #[test]
    fn hkdf_derivation_deterministic() {
        // Use random keys for valid scalars
        let syncer_private = SecretKey::random(&mut thread_rng());
        let author_private = SecretKey::random(&mut thread_rng());

        let syncer_did_key = encode_p256_did_key(&syncer_private.public_key());

        // Derive twice - must be identical
        let host_secret_1 =
            derive_shared_secret(author_private.to_bytes().as_slice(), &syncer_did_key).unwrap();

        let host_secret_2 =
            derive_shared_secret(author_private.to_bytes().as_slice(), &syncer_did_key).unwrap();

        assert_eq!(host_secret_1, host_secret_2);
    }

    #[test]
    fn mac_computation_deterministic() {
        use rsky_space::compute_response_commit_mac;

        let secret = [0x42u8; 32];
        let space = "at://did:plc:test/space/com.example/main";
        let repo = "did:plc:author";
        let rev = "3k12345";
        let data = [0xAB; 32];

        let mac1 = compute_response_commit_mac(&secret, space, repo, rev, &data);
        let mac2 = compute_response_commit_mac(&secret, space, repo, rev, &data);

        assert_eq!(mac1, mac2);
    }

    // ==========================================================================
    // Integration Tests
    // ==========================================================================

    /// Mock KEM provider for testing that stores a fixed key.
    struct MockKemProvider {
        key: [u8; 32],
    }

    #[async_trait::async_trait]
    impl KemKeyProvider for MockKemProvider {
        async fn get_kem_key(&self, _author_did: &str) -> Result<[u8; 32], MacError> {
            Ok(self.key)
        }
    }

    #[tokio::test]
    async fn end_to_end_commit_mac_computation() {
        // Setup: Generate keypairs for author and syncer
        let author_private = SecretKey::random(&mut thread_rng());
        let syncer_private = SecretKey::random(&mut thread_rng());

        let author_public = author_private.public_key();
        let syncer_did_key = encode_p256_did_key(&syncer_private.public_key());

        // Create mock KEM provider with author's private key
        let mut key = [0u8; 32];
        key.copy_from_slice(author_private.to_bytes().as_slice());
        let kem_provider = MockKemProvider { key };

        // Create Want-Mac header
        let want_mac = WantMac {
            mac_type: MacType::Commit,
            version: 1,
            key_share: syncer_did_key.clone(),
        };

        // Test data
        let space = "at://did:plc:authority/space/com.example/main";
        let repo = "did:plc:author";
        let rev = "3k12345";
        let data = [0xABu8; 32];

        // Host computes MAC
        let host_mac = compute_response_mac(
            &kem_provider,
            &want_mac,
            repo,
            space,
            repo,
            Some(rev),
            Some(&data),
            None,
            None,
        )
        .await
        .expect("MAC computation should succeed");

        // Syncer verifies by computing the same MAC
        let syncer_secret = derive_shared_secret_syncer(&syncer_private, &author_public).unwrap();
        let syncer_mac =
            rsky_space::compute_response_commit_mac(&syncer_secret, space, repo, rev, &data);

        // Both MACs should match
        assert_eq!(host_mac, syncer_mac);
    }

    #[tokio::test]
    async fn end_to_end_record_mac_computation() {
        // Setup: Generate keypairs for author and syncer
        let author_private = SecretKey::random(&mut thread_rng());
        let syncer_private = SecretKey::random(&mut thread_rng());

        let author_public = author_private.public_key();
        let syncer_did_key = encode_p256_did_key(&syncer_private.public_key());

        // Create mock KEM provider
        let mut key = [0u8; 32];
        key.copy_from_slice(author_private.to_bytes().as_slice());
        let kem_provider = MockKemProvider { key };

        // Create Want-Mac header for record
        let want_mac = WantMac {
            mac_type: MacType::Record,
            version: 1,
            key_share: syncer_did_key.clone(),
        };

        // Test data
        let space = "at://did:plc:authority/space/com.example/main";
        let repo = "did:plc:author";
        let collection = "app.bsky.feed.post";
        let rkey = "abc123";
        let record_uri = build_record_uri(space, repo, collection, rkey);
        let cid_bytes = [0x01, 0x71, 0x12, 0x20, 0xAA, 0xBB, 0xCC, 0xDD];

        // Host computes MAC
        let host_mac = compute_response_mac(
            &kem_provider,
            &want_mac,
            repo,
            space,
            repo,
            None,
            None,
            Some(&record_uri),
            Some(&cid_bytes),
        )
        .await
        .expect("MAC computation should succeed");

        // Syncer verifies
        let syncer_secret = derive_shared_secret_syncer(&syncer_private, &author_public).unwrap();
        let syncer_mac =
            rsky_space::compute_response_record_mac(&syncer_secret, &record_uri, &cid_bytes);

        assert_eq!(host_mac, syncer_mac);
    }

    #[tokio::test]
    async fn stub_kem_provider_returns_error() {
        let provider = StubKemKeyProvider;
        let result = provider.get_kem_key("did:plc:test").await;
        assert!(matches!(result, Err(MacError::KemKeyNotAvailable)));
    }

    #[test]
    fn vary_header_appends_correctly() {
        // Empty existing header
        assert_eq!(append_vary_header(None), "Atproto-Space-Want-Mac");
        assert_eq!(append_vary_header(Some("")), "Atproto-Space-Want-Mac");

        // With existing header
        assert_eq!(
            append_vary_header(Some("Accept")),
            "Accept, Atproto-Space-Want-Mac"
        );
        assert_eq!(
            append_vary_header(Some("Accept, Content-Type")),
            "Accept, Content-Type, Atproto-Space-Want-Mac"
        );
    }

    #[test]
    fn build_record_uri_format() {
        let uri = build_record_uri(
            "at://did:plc:auth/space/com.example/main",
            "did:plc:author",
            "app.bsky.feed.post",
            "abc123",
        );
        assert_eq!(
            uri,
            "at://did:plc:auth/space/com.example/main/did:plc:author/app.bsky.feed.post/abc123"
        );
    }

    #[test]
    fn mac_type_for_methods() {
        assert_eq!(MacType::expected_for_method("getRecord"), MacType::Record);
        assert_eq!(
            MacType::expected_for_method("getLatestCommit"),
            MacType::Commit
        );
        assert_eq!(MacType::expected_for_method("getRepo"), MacType::Commit);
        assert_eq!(MacType::expected_for_method("listRepoOps"), MacType::Commit);
    }

    #[test]
    fn want_mac_header_is_requested() {
        let header = WantMacHeader(None);
        assert!(!header.is_requested());

        let header = WantMacHeader(Some(WantMac {
            mac_type: MacType::Commit,
            version: 1,
            key_share: "did:key:zDnatest".into(),
        }));
        assert!(header.is_requested());
    }

    #[test]
    fn cross_party_verification_complete_flow() {
        // This test simulates the complete flow:
        // 1. Syncer generates ephemeral keypair
        // 2. Syncer sends Want-Mac header with public key
        // 3. Host retrieves author's KEM key
        // 4. Host performs ECDH and computes MAC
        // 5. Host returns response with MAC header
        // 6. Syncer performs ECDH and verifies MAC

        // Syncer side: generate ephemeral keypair
        let syncer_private = SecretKey::random(&mut thread_rng());
        let syncer_did_key = encode_p256_did_key(&syncer_private.public_key());

        // Author's pre-existing KEM keypair (stored on PDS)
        let author_private = SecretKey::random(&mut thread_rng());
        let author_public = author_private.public_key();

        // Host side: derive shared secret and compute MAC
        let host_secret =
            derive_shared_secret(author_private.to_bytes().as_slice(), &syncer_did_key).unwrap();

        let space = "at://did:plc:auth/space/t/k";
        let repo = "did:plc:author";
        let rev = "3k999";
        let data = [0x42u8; 32];

        let host_mac =
            rsky_space::compute_response_commit_mac(&host_secret, space, repo, rev, &data);

        // Format header for response
        let header_value = format_mac_header(MacType::Commit, 1, &host_mac);

        // Syncer side: parse MAC from header and verify
        let parsed_mac = parse_mac_from_header(&header_value).unwrap();

        // Syncer derives same shared secret (different order!)
        let syncer_secret = derive_shared_secret_syncer(&syncer_private, &author_public).unwrap();

        // Syncer computes expected MAC
        let expected_mac =
            rsky_space::compute_response_commit_mac(&syncer_secret, space, repo, rev, &data);

        // Verify using constant-time comparison
        assert!(rsky_space::verify_response_mac(&expected_mac, &parsed_mac));
    }
}
