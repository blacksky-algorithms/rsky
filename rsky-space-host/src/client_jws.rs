//! JWS verification for tokens a *client* presents: OAuth access tokens and
//! DPoP proofs.
//!
//! These are signed by browsers and SDKs, and WebCrypto emits a high-S ECDSA
//! signature about half the time. Such a signature is perfectly valid ECDSA, so
//! rejecting it fails roughly every other request from a legitimate client.
//! `rsky_space::jwk`'s verifiers are strict for a reason — they also serve repo
//! commits, where low-S is required because a malleable commit signature would
//! be a second valid signature over the same content — so these are separate
//! entry points rather than a relaxation of those.
//!
//! The asymmetry is the point: malleable here, strict for commits.

use rsky_crypto::types::VerifyOptions;
use rsky_space::jwk::{EcJwk, CRV_P256, CRV_SECP256K1};
use rsky_space::{Result, SpaceError};
use sha2::{Digest, Sha256};

fn malleable() -> Option<VerifyOptions> {
    Some(VerifyOptions {
        allow_malleable_sig: Some(true),
    })
}

fn outcome(ok: bool) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(SpaceError::BadSignature)
    }
}

/// ES256 over a JWT signing input, accepting a high-S signature.
pub fn verify_client_es256(jwk: &EcJwk, signing_input: &[u8], sig: &[u8]) -> Result<()> {
    jwk.require_crv(CRV_P256)?;
    let point = jwk.sec1_point()?;
    let ok = rsky_crypto::p256::operations::verify_sig(&point, signing_input, sig, malleable())
        .map_err(|e| SpaceError::Crypto(e.to_string()))?;
    outcome(ok)
}

/// ES256K over a JWT signing input, accepting a high-S signature.
pub fn verify_client_es256k(jwk: &EcJwk, signing_input: &[u8], sig: &[u8]) -> Result<()> {
    jwk.require_crv(CRV_SECP256K1)?;
    let point = jwk.sec1_point()?;
    let digest = Sha256::digest(signing_input);
    let ok = rsky_crypto::secp256k1::operations::verify_sig(&point, &digest, sig, malleable())
        .map_err(|e| SpaceError::Crypto(e.to_string()))?;
    outcome(ok)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use p256::ecdsa::signature::hazmat::PrehashSigner;
    use p256::ecdsa::SigningKey;

    const SIGNING_INPUT: &[u8] = b"eyJhbGciOiJFUzI1NiJ9.eyJodG0iOiJQT1NUIn0";

    /// A P-256 key plus the JWK a client would publish for it.
    fn key_and_jwk(secret: [u8; 32]) -> (SigningKey, EcJwk) {
        let key = SigningKey::from_slice(&secret).expect("key");
        let point = key.verifying_key().to_encoded_point(false);
        let jwk = EcJwk {
            kty: "EC".to_string(),
            crv: CRV_P256.to_string(),
            x: URL_SAFE_NO_PAD.encode(point.x().expect("x")),
            y: URL_SAFE_NO_PAD.encode(point.y().expect("y")),
            kid: None,
        };
        (key, jwk)
    }

    /// A deliberately high-S `r || s`, i.e. what WebCrypto may hand us.
    fn high_s_signature(key: &SigningKey, input: &[u8]) -> Vec<u8> {
        let digest = Sha256::digest(input);
        let sig: p256::ecdsa::Signature = key.sign_prehash(&digest).expect("sign");
        // `normalize_s` yields Some only when it changed something, so this is
        // the low-S form either way; its counterpart is then always high-S.
        let low = sig.normalize_s().unwrap_or(sig);
        malleable_counterpart(&low).to_bytes().to_vec()
    }

    /// (r, n - s) — the other valid signature over the same message.
    fn malleable_counterpart(sig: &p256::ecdsa::Signature) -> p256::ecdsa::Signature {
        use p256::elliptic_curve::scalar::IsHigh;
        let (r, s) = sig.split_scalars();
        let flipped = -*s;
        assert!(bool::from(flipped.is_high()), "counterpart must be high-S");
        p256::ecdsa::Signature::from_scalars(*r, flipped).expect("signature")
    }

    #[test]
    fn high_s_es256_jws_signature_verifies() {
        let (key, jwk) = key_and_jwk([0x31; 32]);
        let sig = high_s_signature(&key, SIGNING_INPUT);

        // The whole point: a client's high-S proof is accepted here.
        verify_client_es256(&jwk, SIGNING_INPUT, &sig).expect("high-S client jws must verify");

        // And the strict path — the one repo commits go through — still refuses
        // it. The asymmetry is deliberate, so assert both halves together.
        assert!(
            rsky_space::jwk::verify_es256(&jwk, SIGNING_INPUT, &sig).is_err(),
            "the commit-path verifier must keep rejecting a high-S signature"
        );
    }

    #[test]
    fn a_low_s_signature_verifies_on_both_paths() {
        let (key, jwk) = key_and_jwk([0x32; 32]);
        let digest = Sha256::digest(SIGNING_INPUT);
        let sig: p256::ecdsa::Signature = key.sign_prehash(&digest).expect("sign");
        let sig = sig.normalize_s().unwrap_or(sig);
        let bytes = sig.to_bytes().to_vec();

        verify_client_es256(&jwk, SIGNING_INPUT, &bytes).expect("client path");
        rsky_space::jwk::verify_es256(&jwk, SIGNING_INPUT, &bytes).expect("commit path");
    }

    #[test]
    fn a_wrong_key_is_refused_on_the_client_path_too() {
        let (key, _) = key_and_jwk([0x33; 32]);
        let (_, other_jwk) = key_and_jwk([0x34; 32]);
        let sig = high_s_signature(&key, SIGNING_INPUT);
        assert!(verify_client_es256(&other_jwk, SIGNING_INPUT, &sig).is_err());
    }
}
