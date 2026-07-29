use crate::error::OAuthError;
use crate::jwt;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::Mutex;
use url::Url;

pub const DPOP_TYP: &str = "dpop+jwt";
/// Total nonce validity window in seconds (three rotation intervals).
pub const DPOP_NONCE_MAX_AGE: u64 = 180;
pub const DEFAULT_ROTATION_INTERVAL: u64 = DPOP_NONCE_MAX_AGE / 3;
/// Maximum age of a DPoP proof's `iat` before clock tolerance.
pub const DPOP_IAT_MAX_AGE: u64 = 10;
pub const DPOP_CLOCK_TOLERANCE: u64 = DPOP_NONCE_MAX_AGE;

/// Single-use `jti` tracking for DPoP proofs.
pub trait ReplayStore: Send + Sync {
    /// Records `jti` until `exp`. Returns true when `jti` was not seen
    /// before (proof accepted); false on replay.
    fn consume(&self, jti: &str, exp: u64, now: u64) -> bool;
}

#[derive(Debug, Default)]
pub struct InMemoryReplayStore {
    seen: Mutex<HashMap<String, u64>>,
}

impl ReplayStore for InMemoryReplayStore {
    fn consume(&self, jti: &str, exp: u64, now: u64) -> bool {
        let mut seen = self.seen.lock().expect("replay store lock poisoned");
        seen.retain(|_, expiry| *expiry > now);
        match seen.entry(jti.to_string()) {
            Entry::Occupied(_) => false,
            Entry::Vacant(entry) => {
                entry.insert(exp);
                true
            }
        }
    }
}

/// Rolling server nonce: HMAC-SHA256 of a time-bucket counter under a
/// fixed secret. Accepts the previous, current and next bucket, matching
/// the upstream `DpopNonce` behavior.
pub struct DpopNonce {
    secret: [u8; 32],
    rotation_interval: u64,
}

impl DpopNonce {
    pub fn new(secret: [u8; 32], rotation_interval: u64) -> Result<Self, OAuthError> {
        if rotation_interval == 0 || rotation_interval > DEFAULT_ROTATION_INTERVAL {
            return Err(OAuthError::ServerError(format!(
                "DPoP nonce rotation interval must be between 1 and {DEFAULT_ROTATION_INTERVAL} seconds"
            )));
        }
        Ok(Self {
            secret,
            rotation_interval,
        })
    }

    pub fn new_random(rotation_interval: u64) -> Result<Self, OAuthError> {
        let secret = rsky_crypto::utils::random_bytes(32)
            .try_into()
            .expect("random_bytes returns the requested length");
        Self::new(secret, rotation_interval)
    }

    fn counter(&self, now: u64) -> u64 {
        now / self.rotation_interval
    }

    fn compute(&self, counter: u64) -> String {
        let mut mac =
            Hmac::<Sha256>::new_from_slice(&self.secret).expect("HMAC accepts any key length");
        mac.update(&counter.to_be_bytes());
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    }

    /// The nonce clients should use on their next request.
    pub fn next(&self, now: u64) -> String {
        self.compute(self.counter(now) + 1)
    }

    pub fn check(&self, nonce: &str, now: u64) -> bool {
        let counter = self.counter(now);
        self.compute(counter + 1) == nonce
            || self.compute(counter) == nonce
            || (counter > 0 && self.compute(counter - 1) == nonce)
    }
}

/// Result of a successfully validated DPoP proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DpopProof {
    pub jti: String,
    /// RFC 7638 thumbprint of the proof's public key, for `cnf.jkt` binding.
    pub jkt: String,
    pub htm: String,
    pub htu: String,
}

/// The parts of an HTTP request needed to validate a DPoP proof.
#[derive(Debug, Clone)]
pub struct DpopRequest<'a> {
    pub method: &'a str,
    pub uri: &'a str,
    /// All values of the `DPoP` header, in order.
    pub dpop_headers: &'a [&'a str],
    /// The DPoP-bound access token accompanying the request, when present.
    pub access_token: Option<&'a str>,
}

pub struct DpopManager {
    nonce: Option<DpopNonce>,
    replay_store: Box<dyn ReplayStore>,
}

impl DpopManager {
    pub fn new(nonce: Option<DpopNonce>, replay_store: Box<dyn ReplayStore>) -> Self {
        Self {
            nonce,
            replay_store,
        }
    }

    /// The value for the `DPoP-Nonce` response header, when nonces are enabled.
    pub fn next_nonce(&self, now: u64) -> Option<String> {
        self.nonce.as_ref().map(|nonce| nonce.next(now))
    }

    /// Validates the request's DPoP proof per RFC 9449 section 4.3.
    /// Returns `Ok(None)` when the request carries no `DPoP` header.
    pub fn check_proof(
        &self,
        request: &DpopRequest,
        now: u64,
    ) -> Result<Option<DpopProof>, OAuthError> {
        if request.method.is_empty() {
            return Err(OAuthError::InvalidRequest(
                "HTTP method is required".to_string(),
            ));
        }
        let Some(proof) = extract_proof(request.dpop_headers)? else {
            return Ok(None);
        };
        let decoded = jwt::decode(proof).map_err(|e| {
            OAuthError::InvalidDpopProof(format!("Failed to parse DPoP proof: {e}"))
        })?;
        if decoded.header.typ.as_deref() != Some(DPOP_TYP) {
            return Err(OAuthError::InvalidDpopProof(format!(
                "DPoP proof \"typ\" must be \"{DPOP_TYP}\""
            )));
        }
        let Some(jwk) = &decoded.header.jwk else {
            return Err(OAuthError::InvalidDpopProof(
                "DPoP proof missing \"jwk\" header".to_string(),
            ));
        };
        if jwk.is_private() {
            return Err(OAuthError::InvalidDpopProof(
                "DPoP \"jwk\" must be a public key".to_string(),
            ));
        }
        jwt::verify_signature(&decoded, jwk).map_err(|e| {
            OAuthError::InvalidDpopProof(format!("Failed to verify DPoP proof: {e}"))
        })?;

        let claims = &decoded.claims;
        let Some(iat) = claims.iat else {
            return Err(OAuthError::InvalidDpopProof(
                "DPoP \"iat\" missing".to_string(),
            ));
        };
        if iat > now.saturating_add(DPOP_CLOCK_TOLERANCE) {
            return Err(OAuthError::InvalidDpopProof(
                "DPoP proof \"iat\" is in the future".to_string(),
            ));
        }
        if now.saturating_sub(iat) > DPOP_IAT_MAX_AGE + DPOP_CLOCK_TOLERANCE {
            return Err(OAuthError::InvalidDpopProof(
                "DPoP proof is too old".to_string(),
            ));
        }
        if let Some(exp) = claims.exp {
            if exp.saturating_add(DPOP_CLOCK_TOLERANCE) <= now {
                return Err(OAuthError::InvalidDpopProof(
                    "DPoP proof has expired".to_string(),
                ));
            }
        }
        let nonce_claim = match claims.extra.get("nonce") {
            None => None,
            Some(Value::String(nonce)) => Some(nonce.as_str()),
            Some(_) => {
                return Err(OAuthError::InvalidDpopProof(
                    "Invalid DPoP \"nonce\" type".to_string(),
                ))
            }
        };
        let jti = match claims.jti.as_deref() {
            Some(jti) if !jti.is_empty() => jti,
            _ => {
                return Err(OAuthError::InvalidDpopProof(
                    "DPoP \"jti\" missing".to_string(),
                ))
            }
        };
        match claims.extra.get("htm") {
            Some(Value::String(htm)) if htm == request.method => {}
            _ => {
                return Err(OAuthError::InvalidDpopProof(
                    "DPoP \"htm\" mismatch".to_string(),
                ))
            }
        }
        let Some(Value::String(htu)) = claims.extra.get("htu") else {
            return Err(OAuthError::InvalidDpopProof(
                "Invalid DPoP \"htu\" type".to_string(),
            ));
        };
        let request_url = Url::parse(request.uri)
            .map_err(|_| OAuthError::InvalidRequest("invalid request URI".to_string()))?;
        if parse_htu(htu)? != normalize_url(&request_url) {
            return Err(OAuthError::InvalidDpopProof(
                "DPoP \"htu\" mismatch".to_string(),
            ));
        }
        match (&self.nonce, nonce_claim) {
            (Some(_), None) => return Err(OAuthError::use_dpop_nonce()),
            (Some(nonce), Some(claim)) if !nonce.check(claim, now) => {
                return Err(OAuthError::UseDpopNonce(
                    "DPoP \"nonce\" mismatch".to_string(),
                ));
            }
            (None, Some(_)) => {
                return Err(OAuthError::UseDpopNonce(
                    "DPoP \"nonce\" mismatch".to_string(),
                ));
            }
            _ => {}
        }
        let ath_claim = claims.extra.get("ath");
        match request.access_token {
            Some(token) => {
                let expected = URL_SAFE_NO_PAD.encode(Sha256::digest(token.as_bytes()));
                if ath_claim.and_then(Value::as_str) != Some(expected.as_str()) {
                    return Err(OAuthError::InvalidDpopProof(
                        "DPoP \"ath\" mismatch".to_string(),
                    ));
                }
            }
            None => {
                if ath_claim.is_some() {
                    return Err(OAuthError::InvalidDpopProof(
                        "DPoP \"ath\" claim not allowed".to_string(),
                    ));
                }
            }
        }
        let replay_expiry = iat + DPOP_IAT_MAX_AGE + DPOP_CLOCK_TOLERANCE;
        if !self.replay_store.consume(jti, replay_expiry, now) {
            return Err(OAuthError::InvalidDpopProof(
                "DPoP proof \"jti\" replayed".to_string(),
            ));
        }
        Ok(Some(DpopProof {
            jti: jti.to_string(),
            jkt: jwk.thumbprint(),
            htm: request.method.to_string(),
            htu: htu.clone(),
        }))
    }
}

fn extract_proof<'a>(headers: &'a [&'a str]) -> Result<Option<&'a str>, OAuthError> {
    match headers {
        [] => Ok(None),
        [proof] if !proof.is_empty() => Ok(Some(proof)),
        [_] => Err(OAuthError::InvalidDpopProof(
            "DPoP header cannot be empty".to_string(),
        )),
        _ => Err(OAuthError::InvalidDpopProof(
            "DPoP header must contain a single proof".to_string(),
        )),
    }
}

/// RFC 9449 section 4.3 syntax normalization: scheme + host (+ explicit
/// non-default port) + path, no query or fragment.
fn normalize_url(url: &Url) -> String {
    format!("{}{}", url.origin().ascii_serialization(), url.path())
}

fn parse_htu(htu: &str) -> Result<String, OAuthError> {
    let url = Url::parse(htu)
        .map_err(|_| OAuthError::InvalidDpopProof("DPoP \"htu\" is not a valid URL".to_string()))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(OAuthError::InvalidDpopProof(
            "DPoP \"htu\" must not contain credentials".to_string(),
        ));
    }
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(OAuthError::InvalidDpopProof(
            "DPoP \"htu\" must be http or https".to_string(),
        ));
    }
    Ok(normalize_url(&url))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jwk::{EcCurve, Jwk};
    use crate::jwt::{JwtClaims, JwtHeader};
    use serde_json::json;

    const NOW: u64 = 1_700_000_000;
    const URI: &str = "https://pds.example.com/oauth/token?foo=bar#frag";
    const HTU: &str = "https://pds.example.com/oauth/token";

    fn private_key(curve: EcCurve) -> Jwk {
        Jwk::from_private_key_bytes(curve, &[0x42u8; 32]).unwrap()
    }

    fn base_claims() -> JwtClaims {
        let mut claims = JwtClaims {
            iat: Some(NOW),
            jti: Some("jti-1".to_string()),
            ..Default::default()
        };
        claims.extra.insert("htm".to_string(), json!("POST"));
        claims.extra.insert("htu".to_string(), json!(HTU));
        claims
    }

    fn proof(key: &Jwk, typ: Option<&str>, include_jwk: bool, claims: &JwtClaims) -> String {
        let mut header = JwtHeader::new(key.curve().unwrap().alg());
        header.typ = typ.map(String::from);
        if include_jwk {
            header.jwk = Some(key.to_public());
        }
        crate::jwt::sign(&header, claims, key).unwrap()
    }

    fn standard_proof(claims: &JwtClaims) -> String {
        proof(&private_key(EcCurve::P256), Some(DPOP_TYP), true, claims)
    }

    fn manager() -> DpopManager {
        DpopManager::new(None, Box::new(InMemoryReplayStore::default()))
    }

    fn manager_with_nonce() -> DpopManager {
        DpopManager::new(
            Some(DpopNonce::new([7u8; 32], DEFAULT_ROTATION_INTERVAL).unwrap()),
            Box::new(InMemoryReplayStore::default()),
        )
    }

    fn request<'a>(headers: &'a [&'a str], access_token: Option<&'a str>) -> DpopRequest<'a> {
        DpopRequest {
            method: "POST",
            uri: URI,
            dpop_headers: headers,
            access_token,
        }
    }

    fn check_err(claims: &JwtClaims) -> OAuthError {
        let token = standard_proof(claims);
        manager()
            .check_proof(&request(&[token.as_str()], None), NOW)
            .unwrap_err()
    }

    #[test]
    fn happy_path_both_curves() {
        for curve in [EcCurve::P256, EcCurve::K256] {
            let key = private_key(curve);
            let token = proof(&key, Some(DPOP_TYP), true, &base_claims());
            let result = manager()
                .check_proof(&request(&[token.as_str()], None), NOW)
                .unwrap()
                .unwrap();
            assert_eq!(result.jti, "jti-1");
            assert_eq!(result.htm, "POST");
            assert_eq!(result.htu, HTU);
            assert_eq!(result.jkt, key.thumbprint());
            assert!(!format!("{result:?}").is_empty());
        }
    }

    #[test]
    fn missing_and_malformed_dpop_headers() {
        let dpop_manager = manager();
        assert_eq!(
            dpop_manager.check_proof(&request(&[], None), NOW).unwrap(),
            None
        );
        assert_eq!(
            dpop_manager
                .check_proof(&request(&[""], None), NOW)
                .unwrap_err(),
            OAuthError::InvalidDpopProof("DPoP header cannot be empty".to_string())
        );
        assert_eq!(
            dpop_manager
                .check_proof(&request(&["a", "b"], None), NOW)
                .unwrap_err(),
            OAuthError::InvalidDpopProof("DPoP header must contain a single proof".to_string())
        );
        let err = dpop_manager
            .check_proof(&request(&["not-a-jwt"], None), NOW)
            .unwrap_err();
        assert!(err
            .error_description()
            .starts_with("Failed to parse DPoP proof"));
    }

    #[test]
    fn empty_method_and_bad_request_uri() {
        let token = standard_proof(&base_claims());
        let mut req = request(&[], None);
        req.method = "";
        assert_eq!(
            manager().check_proof(&req, NOW).unwrap_err(),
            OAuthError::InvalidRequest("HTTP method is required".to_string())
        );
        let headers = [token.as_str()];
        let mut req = request(&headers, None);
        req.uri = "not a url";
        assert_eq!(
            manager().check_proof(&req, NOW).unwrap_err(),
            OAuthError::InvalidRequest("invalid request URI".to_string())
        );
    }

    #[test]
    fn wrong_typ_rejected() {
        let key = private_key(EcCurve::P256);
        for typ in [Some("JWT"), None] {
            let token = proof(&key, typ, true, &base_claims());
            assert_eq!(
                manager()
                    .check_proof(&request(&[token.as_str()], None), NOW)
                    .unwrap_err(),
                OAuthError::InvalidDpopProof("DPoP proof \"typ\" must be \"dpop+jwt\"".to_string())
            );
        }
    }

    #[test]
    fn missing_jwk_rejected() {
        let key = private_key(EcCurve::P256);
        let token = proof(&key, Some(DPOP_TYP), false, &base_claims());
        assert_eq!(
            manager()
                .check_proof(&request(&[token.as_str()], None), NOW)
                .unwrap_err(),
            OAuthError::InvalidDpopProof("DPoP proof missing \"jwk\" header".to_string())
        );
    }

    #[test]
    fn private_jwk_smuggled_rejected() {
        let key = private_key(EcCurve::P256);
        let mut header = JwtHeader::new("ES256");
        header.typ = Some(DPOP_TYP.to_string());
        header.jwk = Some(key.clone());
        let token = crate::jwt::sign(&header, &base_claims(), &key).unwrap();
        assert_eq!(
            manager()
                .check_proof(&request(&[token.as_str()], None), NOW)
                .unwrap_err(),
            OAuthError::InvalidDpopProof("DPoP \"jwk\" must be a public key".to_string())
        );
    }

    #[test]
    fn tampered_signature_rejected() {
        let token = standard_proof(&base_claims());
        let parts: Vec<&str> = token.split('.').collect();
        let tampered = format!(
            "{}.{}.{}",
            parts[0],
            parts[1],
            URL_SAFE_NO_PAD.encode([0x11u8; 64])
        );
        let err = manager()
            .check_proof(&request(&[tampered.as_str()], None), NOW)
            .unwrap_err();
        assert!(err
            .error_description()
            .starts_with("Failed to verify DPoP proof"));
    }

    #[test]
    fn iat_and_exp_windows() {
        let mut claims = base_claims();
        claims.iat = None;
        assert_eq!(
            check_err(&claims),
            OAuthError::InvalidDpopProof("DPoP \"iat\" missing".to_string())
        );

        let mut claims = base_claims();
        claims.iat = Some(NOW + DPOP_CLOCK_TOLERANCE + 1);
        assert_eq!(
            check_err(&claims),
            OAuthError::InvalidDpopProof("DPoP proof \"iat\" is in the future".to_string())
        );

        let mut claims = base_claims();
        claims.iat = Some(NOW - DPOP_IAT_MAX_AGE - DPOP_CLOCK_TOLERANCE - 1);
        assert_eq!(
            check_err(&claims),
            OAuthError::InvalidDpopProof("DPoP proof is too old".to_string())
        );

        // Accepted at the edge of the window.
        let mut claims = base_claims();
        claims.iat = Some(NOW - DPOP_IAT_MAX_AGE - DPOP_CLOCK_TOLERANCE);
        let token = standard_proof(&claims);
        manager()
            .check_proof(&request(&[token.as_str()], None), NOW)
            .unwrap()
            .unwrap();

        let mut claims = base_claims();
        claims.exp = Some(NOW - DPOP_CLOCK_TOLERANCE);
        assert_eq!(
            check_err(&claims),
            OAuthError::InvalidDpopProof("DPoP proof has expired".to_string())
        );

        let mut claims = base_claims();
        claims.exp = Some(NOW + 30);
        let token = standard_proof(&claims);
        manager()
            .check_proof(&request(&[token.as_str()], None), NOW)
            .unwrap()
            .unwrap();
    }

    #[test]
    fn jti_required_and_single_use() {
        let mut claims = base_claims();
        claims.jti = None;
        assert_eq!(
            check_err(&claims),
            OAuthError::InvalidDpopProof("DPoP \"jti\" missing".to_string())
        );
        let mut claims = base_claims();
        claims.jti = Some(String::new());
        assert_eq!(
            check_err(&claims),
            OAuthError::InvalidDpopProof("DPoP \"jti\" missing".to_string())
        );

        let dpop_manager = manager();
        let token = standard_proof(&base_claims());
        let headers = [token.as_str()];
        dpop_manager
            .check_proof(&request(&headers, None), NOW)
            .unwrap()
            .unwrap();
        assert_eq!(
            dpop_manager
                .check_proof(&request(&headers, None), NOW)
                .unwrap_err(),
            OAuthError::InvalidDpopProof("DPoP proof \"jti\" replayed".to_string())
        );
        // After the replay record expires, the jti is purged and reusable.
        dpop_manager
            .check_proof(
                &request(&headers, None),
                NOW + DPOP_IAT_MAX_AGE + DPOP_CLOCK_TOLERANCE,
            )
            .unwrap()
            .unwrap();
    }

    #[test]
    fn htm_mismatch_rejected() {
        let mut claims = base_claims();
        claims.extra.insert("htm".to_string(), json!("GET"));
        assert_eq!(
            check_err(&claims),
            OAuthError::InvalidDpopProof("DPoP \"htm\" mismatch".to_string())
        );
        let mut claims = base_claims();
        claims.extra.insert("htm".to_string(), json!(5));
        assert_eq!(
            check_err(&claims),
            OAuthError::InvalidDpopProof("DPoP \"htm\" mismatch".to_string())
        );
        let mut claims = base_claims();
        claims.extra.remove("htm");
        assert_eq!(
            check_err(&claims),
            OAuthError::InvalidDpopProof("DPoP \"htm\" mismatch".to_string())
        );
    }

    #[test]
    fn htu_validation() {
        let mut claims = base_claims();
        claims.extra.remove("htu");
        assert_eq!(
            check_err(&claims),
            OAuthError::InvalidDpopProof("Invalid DPoP \"htu\" type".to_string())
        );

        let mut claims = base_claims();
        claims.extra.insert("htu".to_string(), json!(5));
        assert_eq!(
            check_err(&claims),
            OAuthError::InvalidDpopProof("Invalid DPoP \"htu\" type".to_string())
        );

        let mut claims = base_claims();
        claims.extra.insert(
            "htu".to_string(),
            json!("https://other.example.com/oauth/token"),
        );
        assert_eq!(
            check_err(&claims),
            OAuthError::InvalidDpopProof("DPoP \"htu\" mismatch".to_string())
        );

        let mut claims = base_claims();
        claims.extra.insert("htu".to_string(), json!("not a url"));
        assert_eq!(
            check_err(&claims),
            OAuthError::InvalidDpopProof("DPoP \"htu\" is not a valid URL".to_string())
        );

        let mut claims = base_claims();
        claims.extra.insert(
            "htu".to_string(),
            json!("https://user:pass@pds.example.com/oauth/token"),
        );
        assert_eq!(
            check_err(&claims),
            OAuthError::InvalidDpopProof("DPoP \"htu\" must not contain credentials".to_string())
        );

        let mut claims = base_claims();
        claims.extra.insert(
            "htu".to_string(),
            json!("ftp://pds.example.com/oauth/token"),
        );
        assert_eq!(
            check_err(&claims),
            OAuthError::InvalidDpopProof("DPoP \"htu\" must be http or https".to_string())
        );
    }

    #[test]
    fn htu_normalization_accepts_equivalent_urls() {
        // Legacy query/fragment, case-insensitive scheme and host, default
        // port elision, and dot-segment resolution all normalize away.
        for htu in [
            "https://pds.example.com/oauth/token?query=1#frag",
            "HTTPS://PDS.EXAMPLE.COM/oauth/token",
            "https://pds.example.com:443/oauth/token",
            "https://pds.example.com/oauth/../oauth/token",
        ] {
            let mut claims = base_claims();
            claims.extra.insert("htu".to_string(), json!(htu));
            let token = standard_proof(&claims);
            let result = manager()
                .check_proof(&request(&[token.as_str()], None), NOW)
                .unwrap()
                .unwrap();
            assert_eq!(result.htu, htu);
        }
    }

    #[test]
    fn ath_binding() {
        let access_token = "access-token-value";
        let expected_ath = URL_SAFE_NO_PAD.encode(Sha256::digest(access_token.as_bytes()));

        let mut claims = base_claims();
        claims.extra.insert("ath".to_string(), json!(expected_ath));
        let token = standard_proof(&claims);
        manager()
            .check_proof(&request(&[token.as_str()], Some(access_token)), NOW)
            .unwrap()
            .unwrap();

        // Wrong ath.
        let mut claims = base_claims();
        claims.extra.insert("ath".to_string(), json!("wrong"));
        let token = standard_proof(&claims);
        assert_eq!(
            manager()
                .check_proof(&request(&[token.as_str()], Some(access_token)), NOW)
                .unwrap_err(),
            OAuthError::InvalidDpopProof("DPoP \"ath\" mismatch".to_string())
        );

        // Missing ath while an access token is present.
        let token = standard_proof(&base_claims());
        assert_eq!(
            manager()
                .check_proof(&request(&[token.as_str()], Some(access_token)), NOW)
                .unwrap_err(),
            OAuthError::InvalidDpopProof("DPoP \"ath\" mismatch".to_string())
        );

        // ath present without an access token.
        let mut claims = base_claims();
        claims.extra.insert("ath".to_string(), json!(expected_ath));
        let token = standard_proof(&claims);
        assert_eq!(
            manager()
                .check_proof(&request(&[token.as_str()], None), NOW)
                .unwrap_err(),
            OAuthError::InvalidDpopProof("DPoP \"ath\" claim not allowed".to_string())
        );
    }

    #[test]
    fn nonce_required_and_rotation() {
        let dpop_manager = manager_with_nonce();
        let issued = dpop_manager.next_nonce(NOW).unwrap();
        assert!(manager().next_nonce(NOW).is_none());

        // Proof without a nonce claim: use_dpop_nonce.
        let token = standard_proof(&base_claims());
        let err = dpop_manager
            .check_proof(&request(&[token.as_str()], None), NOW)
            .unwrap_err();
        assert_eq!(err, OAuthError::use_dpop_nonce());
        assert!(err.requires_dpop_nonce());

        // Proof with the issued nonce is accepted.
        let mut claims = base_claims();
        claims.extra.insert("nonce".to_string(), json!(issued));
        let token = standard_proof(&claims);
        dpop_manager
            .check_proof(&request(&[token.as_str()], None), NOW)
            .unwrap()
            .unwrap();

        // A nonce issued one and two buckets ago is still accepted (the
        // current bucket's nonce is the previous "next").
        for age in [DEFAULT_ROTATION_INTERVAL, 2 * DEFAULT_ROTATION_INTERVAL] {
            let old = dpop_manager.next_nonce(NOW - age).unwrap();
            let mut claims = base_claims();
            claims.jti = Some(format!("jti-age-{age}"));
            claims.extra.insert("nonce".to_string(), json!(old));
            let token = standard_proof(&claims);
            dpop_manager
                .check_proof(&request(&[token.as_str()], None), NOW)
                .unwrap()
                .unwrap();
        }

        // Three buckets old is stale.
        let stale = dpop_manager
            .next_nonce(NOW - 3 * DEFAULT_ROTATION_INTERVAL)
            .unwrap();
        let mut claims = base_claims();
        claims.extra.insert("nonce".to_string(), json!(stale));
        let token = standard_proof(&claims);
        assert_eq!(
            dpop_manager
                .check_proof(&request(&[token.as_str()], None), NOW)
                .unwrap_err(),
            OAuthError::UseDpopNonce("DPoP \"nonce\" mismatch".to_string())
        );
    }

    #[test]
    fn nonce_type_and_disabled_manager() {
        let mut claims = base_claims();
        claims.extra.insert("nonce".to_string(), json!(5));
        assert_eq!(
            check_err(&claims),
            OAuthError::InvalidDpopProof("Invalid DPoP \"nonce\" type".to_string())
        );

        // A nonce claim when nonces are disabled mirrors upstream: rejected.
        let mut claims = base_claims();
        claims.extra.insert("nonce".to_string(), json!("anything"));
        assert_eq!(
            check_err(&claims),
            OAuthError::UseDpopNonce("DPoP \"nonce\" mismatch".to_string())
        );
    }

    #[test]
    fn nonce_check_near_epoch_skips_previous_bucket() {
        let nonce = DpopNonce::new([7u8; 32], DEFAULT_ROTATION_INTERVAL).unwrap();
        let now = DEFAULT_ROTATION_INTERVAL / 2;
        assert!(nonce.check(&nonce.next(now), now));
        assert!(!nonce.check("bogus", now));
    }

    #[test]
    fn nonce_constructor_validation() {
        assert!(DpopNonce::new([0u8; 32], 0).is_err());
        assert!(DpopNonce::new([0u8; 32], DEFAULT_ROTATION_INTERVAL + 1).is_err());
        let random = DpopNonce::new_random(DEFAULT_ROTATION_INTERVAL).unwrap();
        assert!(!random.next(NOW).is_empty());
    }

    #[test]
    fn replay_store_purges_expired_entries() {
        let store = InMemoryReplayStore::default();
        assert!(store.consume("a", NOW + 10, NOW));
        assert!(!store.consume("a", NOW + 10, NOW));
        assert!(store.consume("a", NOW + 30, NOW + 10));
        assert!(!format!("{store:?}").is_empty());
    }
}
