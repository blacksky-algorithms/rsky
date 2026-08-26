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
use rsky_space::SpaceError;
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Distinct from `SpaceError::BadSignature`, whose message reads "commit
/// signature verification failed" — accurate for a repo commit, actively
/// misleading on a DPoP proof, which is where it kept surfacing.
#[derive(Debug, Error)]
pub enum ClientJwsError {
    #[error("presented signature does not verify against the key in its header")]
    BadSignature,
    #[error("unusable key: {0}")]
    Key(String),
    #[error("verification failed: {0}")]
    Crypto(String),
}

type Result<T> = std::result::Result<T, ClientJwsError>;

impl From<SpaceError> for ClientJwsError {
    fn from(error: SpaceError) -> Self {
        ClientJwsError::Key(error.to_string())
    }
}

fn malleable() -> Option<VerifyOptions> {
    Some(VerifyOptions {
        allow_malleable_sig: Some(true),
    })
}

fn outcome(ok: bool) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(ClientJwsError::BadSignature)
    }
}

/// ES256 over a JWT signing input, accepting a high-S signature.
pub fn verify_client_es256(jwk: &EcJwk, signing_input: &[u8], sig: &[u8]) -> Result<()> {
    jwk.require_crv(CRV_P256)?;
    let point = jwk.sec1_point()?;
    let ok = rsky_crypto::p256::operations::verify_sig(&point, signing_input, sig, malleable())
        .map_err(|e| ClientJwsError::Crypto(e.to_string()))?;
    outcome(ok)
}

/// ES256K over a JWT signing input, accepting a high-S signature.
///
/// `allow_malleable_sig` is not enough on this curve: it only waives the
/// encoding check, and libsecp256k1 refuses a high-S signature regardless. So
/// normalise the scalar first. The P-256 verifier needs no equivalent because
/// RustCrypto's does not enforce low-S in the first place.
pub fn verify_client_es256k(jwk: &EcJwk, signing_input: &[u8], sig: &[u8]) -> Result<()> {
    jwk.require_crv(CRV_SECP256K1)?;
    let point = jwk.sec1_point()?;
    let digest = Sha256::digest(signing_input);
    // A JWS signature is the fixed-width `r || s`, but DER has been accepted
    // here historically; leave that path exactly as it was.
    let normalised = match secp256k1::ecdsa::Signature::from_compact(sig) {
        Ok(mut parsed) => {
            parsed.normalize_s();
            Some(parsed.serialize_compact())
        }
        Err(_) => None,
    };
    let presented = normalised.as_ref().map(|s| s.as_slice()).unwrap_or(sig);
    let ok =
        rsky_crypto::secp256k1::operations::verify_sig(&point, &digest, presented, malleable())
            .map_err(|e| ClientJwsError::Crypto(e.to_string()))?;
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

    /// A secp256k1 key plus the JWK a client publishes for it.
    fn k256_key_and_jwk(secret: [u8; 32]) -> (secp256k1::SecretKey, EcJwk) {
        let secret = secp256k1::SecretKey::from_slice(&secret).expect("key");
        let public = secp256k1::PublicKey::from_secret_key(&secp256k1::Secp256k1::new(), &secret);
        let uncompressed = public.serialize_uncompressed();
        let jwk = EcJwk {
            kty: "EC".to_string(),
            crv: CRV_SECP256K1.to_string(),
            x: URL_SAFE_NO_PAD.encode(&uncompressed[1..33]),
            y: URL_SAFE_NO_PAD.encode(&uncompressed[33..65]),
            kid: None,
        };
        (secret, jwk)
    }

    /// A deliberately high-S ES256K signature: what the OAuth client emits
    /// about half the time.
    fn high_s_k256_signature(secret: &secp256k1::SecretKey, input: &[u8]) -> Vec<u8> {
        let digest = Sha256::digest(input);
        let message = secp256k1::Message::from_digest_slice(&digest).expect("digest");
        let signature = secp256k1::Secp256k1::new().sign_ecdsa(&message, secret);
        // rust-secp256k1 always hands back the normalised form, so flip it.
        let mut bytes = signature.serialize_compact();
        let n = [
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFE, 0xBA, 0xAE, 0xDC, 0xE6, 0xAF, 0x48, 0xA0, 0x3B, 0xBF, 0xD2, 0x5E, 0x8C,
            0xD0, 0x36, 0x41, 0x41,
        ];
        let mut borrow = 0i16;
        for i in (0..32).rev() {
            let diff = n[i] as i16 - bytes[32 + i] as i16 - borrow;
            if diff < 0 {
                bytes[32 + i] = (diff + 256) as u8;
                borrow = 1;
            } else {
                bytes[32 + i] = diff as u8;
                borrow = 0;
            }
        }
        bytes.to_vec()
    }

    #[test]
    fn high_s_es256k_jws_signature_verifies() {
        let (secret, jwk) = k256_key_and_jwk([0x51; 32]);
        let sig = high_s_k256_signature(&secret, SIGNING_INPUT);

        // The client's high-S proof is accepted here...
        verify_client_es256k(&jwk, SIGNING_INPUT, &sig)
            .expect("high-S client jws must verify on secp256k1");

        // ...and the commit-path verifier still refuses it. Both curves are
        // asserted because only the P-256 half was ever covered.
        assert!(
            rsky_space::jwk::verify_es256k(&jwk, SIGNING_INPUT, &sig).is_err(),
            "the commit-path verifier must keep rejecting a high-S signature"
        );
    }

    #[test]
    fn a_low_s_es256k_signature_verifies_on_both_paths() {
        let (secret, jwk) = k256_key_and_jwk([0x52; 32]);
        let digest = Sha256::digest(SIGNING_INPUT);
        let message = secp256k1::Message::from_digest_slice(&digest).expect("digest");
        let sig = secp256k1::Secp256k1::new()
            .sign_ecdsa(&message, &secret)
            .serialize_compact()
            .to_vec();

        verify_client_es256k(&jwk, SIGNING_INPUT, &sig).expect("client path");
        rsky_space::jwk::verify_es256k(&jwk, SIGNING_INPUT, &sig).expect("commit path");
    }

    #[test]
    fn a_wrong_key_is_refused_on_the_client_path_too() {
        let (key, _) = key_and_jwk([0x33; 32]);
        let (_, other_jwk) = key_and_jwk([0x34; 32]);
        let sig = high_s_signature(&key, SIGNING_INPUT);
        assert!(verify_client_es256(&other_jwk, SIGNING_INPUT, &sig).is_err());
    }
}
