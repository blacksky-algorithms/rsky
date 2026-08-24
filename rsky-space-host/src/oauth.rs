//! Write-path authentication: DPoP-bound OAuth access tokens (D24).
//!
//! Space writes arrive with the same access token the account's PDS issued for
//! ordinary XRPC, so this verifies a token it did not mint:
//!
//! - the authorization server's signature over the token, against its JWKS;
//! - `iss` is the trusted authorization server;
//! - `aud` is the PDS service DID — a `did:` string, never an origin URL;
//! - the header `typ` is `at+jwt` and `exp` has not passed;
//! - `cnf.jkt` matches the thumbprint of the presented DPoP proof's key;
//! - `sub` is the account the caller claims to be writing as;
//! - `client_id` is on the first-party allowlist.
//!
//! Tokens carry no `scope` claim, so no scope check is possible and
//! `client_id` stands in for one. A token revoked before it expires still
//! verifies until `exp`.

use crate::client_jws::{verify_client_es256, verify_client_es256k};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rsky_space::jwk::{EcJwk, JwkSet};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::attestation::{JtiStore, MetadataFetcher, MAX_IAT_SKEW_SECS};
use crate::error::{HostError, Result};
use crate::pds_seam::VerifyOnlyHs256Secret;

pub const ACCESS_TOKEN_TYP: &str = "at+jwt";
pub const DPOP_TYP: &str = "dpop+jwt";
pub const SUPPORTED_ALG: &str = "ES256";
pub const HS256: &str = "HS256";
/// What an access token may be signed with. `HS256` is not a weakening: it is
/// what a standalone PDS actually issues (see `verify_as_signature`).
pub const SUPPORTED_TOKEN_ALGS: [&str; 2] = ["ES256", "HS256"];
pub const ES256K: &str = "ES256K";
pub const SUPPORTED_PROOF_ALGS: [&str; 2] = ["ES256", ES256K];

/// How long a DPoP proof stays acceptable after its `iat`.
pub const MAX_DPOP_AGE_SECS: u64 = 300;

/// The trust anchors the shim checks a token against.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// The authorization server's issuer identifier.
    pub issuer: String,
    /// The JWKS document the authorization server signs tokens with.
    pub jwks_uri: String,
    /// The PDS service DID tokens are audienced to (`did:web:…`).
    pub audience: String,
    /// First-party `client_id`s permitted to write into a space.
    pub client_ids: Vec<String>,
    /// The authorization server's symmetric signing secret, present when the
    /// PDS is its own authorization server (see `verify_as_signature`).
    /// `None` selects the JWKS path.
    pub hs256_secret: Option<VerifyOnlyHs256Secret>,
}

impl AuthConfig {
    pub fn validate(&self) -> std::result::Result<(), String> {
        if !self.audience.starts_with("did:") {
            return Err(format!(
                "audience must be a service DID, got {}",
                self.audience
            ));
        }
        if self.client_ids.is_empty() {
            return Err("client allowlist is empty".to_string());
        }
        Ok(())
    }
}

/// The parts of an inbound request the shim needs.
pub struct RequestAuth<'a> {
    pub authorization: Option<&'a str>,
    pub dpop: Option<&'a str>,
    pub method: &'a str,
    /// Absolute request URI as the client saw it, query and fragment stripped.
    pub url: &'a str,
}

/// A verified caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessContext {
    pub did: String,
    pub client_id: String,
    pub jkt: String,
}

impl AccessContext {
    /// A token authorizes writes only as its own subject, so a request naming
    /// another author is rejected rather than silently rewritten.
    pub fn require_author(&self, author_did: &str) -> Result<()> {
        if self.did == author_did {
            Ok(())
        } else {
            Err(auth_err("token subject is not the named author"))
        }
    }
}

#[derive(Debug, Deserialize)]
struct JwtHeader {
    #[serde(default)]
    typ: String,
    #[serde(default)]
    alg: String,
    #[serde(default)]
    kid: Option<String>,
    #[serde(default)]
    jwk: Option<EcJwk>,
}

#[derive(Debug, Deserialize)]
struct AccessClaims {
    iss: String,
    aud: Audience,
    sub: String,
    exp: u64,
    #[serde(default)]
    client_id: Option<String>,
    #[serde(default)]
    cnf: Option<Confirmation>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

impl Audience {
    fn contains(&self, want: &str) -> bool {
        match self {
            Self::One(aud) => aud == want,
            Self::Many(auds) => auds.iter().any(|a| a == want),
        }
    }
}

#[derive(Debug, Deserialize)]
struct Confirmation {
    #[serde(default)]
    jkt: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DpopClaims {
    jti: String,
    htm: String,
    htu: String,
    iat: u64,
    #[serde(default)]
    ath: Option<String>,
}

struct DecodedJwt<C> {
    header: JwtHeader,
    claims: C,
    signing_input: Vec<u8>,
    signature: Vec<u8>,
}

fn auth_err(msg: impl Into<String>) -> HostError {
    HostError::Delegation(msg.into())
}

fn decode_part<T: for<'de> Deserialize<'de>>(part: &str, what: &str) -> Result<T> {
    let bytes = URL_SAFE_NO_PAD
        .decode(part)
        .map_err(|e| auth_err(format!("{what} is not base64url: {e}")))?;
    serde_json::from_slice(&bytes).map_err(|e| auth_err(format!("malformed {what}: {e}")))
}

fn decode_jwt<C: for<'de> Deserialize<'de>>(jwt: &str) -> Result<DecodedJwt<C>> {
    let parts: Vec<&str> = jwt.split('.').collect();
    if parts.len() != 3 {
        return Err(auth_err("jwt must have three parts"));
    }
    Ok(DecodedJwt {
        header: decode_part(parts[0], "jwt header")?,
        claims: decode_part(parts[1], "jwt claims")?,
        signing_input: format!("{}.{}", parts[0], parts[1]).into_bytes(),
        signature: URL_SAFE_NO_PAD
            .decode(parts[2])
            .map_err(|e| auth_err(format!("signature is not base64url: {e}")))?,
    })
}

/// RFC 7638 JWK thumbprint: SHA-256 over the required members in
/// lexicographic order, base64url-encoded.
pub fn jwk_thumbprint(jwk: &EcJwk) -> String {
    let canonical = format!(
        r#"{{"crv":"{}","kty":"{}","x":"{}","y":"{}"}}"#,
        jwk.crv, jwk.kty, jwk.x, jwk.y
    );
    URL_SAFE_NO_PAD.encode(Sha256::digest(canonical.as_bytes()))
}

fn access_token_hash(token: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(token.as_bytes()))
}

fn split_scheme(header: &str) -> Result<(&str, &str)> {
    header
        .split_once(' ')
        .map(|(scheme, value)| (scheme, value.trim()))
        .ok_or_else(|| auth_err("malformed authorization header"))
}

/// Strip query and fragment; `htu` compares only scheme, host and path.
fn htu_of(url: &str) -> &str {
    let end = url.find(['?', '#']).unwrap_or(url.len());
    &url[..end]
}

/// Verify a DPoP-bound access token on an inbound write and return the caller.
pub async fn verify_access(
    request: &RequestAuth<'_>,
    config: &AuthConfig,
    fetcher: &dyn MetadataFetcher,
    jti_store: &dyn JtiStore,
    now: u64,
) -> Result<AccessContext> {
    let authorization = request
        .authorization
        .ok_or_else(|| auth_err("missing authorization header"))?;
    let (scheme, token) = split_scheme(authorization)?;
    if !scheme.eq_ignore_ascii_case("DPoP") {
        return Err(auth_err(format!("unsupported auth scheme {scheme}")));
    }
    let proof = request
        .dpop
        .ok_or_else(|| auth_err("missing DPoP proof header"))?;

    let jkt = verify_dpop_proof(proof, token, request, jti_store, now).await?;
    let decoded: DecodedJwt<AccessClaims> = decode_jwt(token)?;

    if decoded.header.typ != ACCESS_TOKEN_TYP {
        return Err(auth_err(format!(
            "token typ {} != {ACCESS_TOKEN_TYP}",
            decoded.header.typ
        )));
    }
    if !SUPPORTED_TOKEN_ALGS.contains(&decoded.header.alg.as_str()) {
        return Err(auth_err(format!(
            "unsupported token alg {}",
            decoded.header.alg
        )));
    }
    let claims = &decoded.claims;
    if claims.iss != config.issuer {
        return Err(auth_err(
            "token issuer is not the trusted authorization server",
        ));
    }
    if !claims.aud.contains(&config.audience) {
        return Err(auth_err("token audience is not this pds service did"));
    }
    if now >= claims.exp {
        return Err(auth_err("token expired"));
    }
    let bound = claims
        .cnf
        .as_ref()
        .and_then(|c| c.jkt.as_deref())
        .ok_or_else(|| auth_err("token is not dpop-bound"))?;
    if bound != jkt {
        return Err(auth_err("token is bound to a different key"));
    }
    let client_id = claims
        .client_id
        .clone()
        .ok_or_else(|| auth_err("token carries no client_id"))?;
    if !config.client_ids.iter().any(|c| *c == client_id) {
        return Err(auth_err("client_id is not first-party"));
    }

    verify_as_signature(&decoded, config, fetcher).await?;

    Ok(AccessContext {
        did: claims.sub.clone(),
        client_id,
        jkt,
    })
}

/// Verify the token's signature against the authorization server.
///
/// **A PDS that is its own authorization server signs access tokens with
/// HS256**, using the same symmetric secret it uses for legacy session JWTs
/// (`pds/src/context.ts`, the `keyset` given to `OAuthProvider`). It therefore
/// publishes an *empty* JWKS: a symmetric key has no public half. The
/// asymmetric path applies only when an entryway issues the tokens instead.
///
/// So a co-located verifier has two options, and only two: hold the same
/// secret, or ask the PDS. This holds the secret — the host already reads
/// every local account's signing key from the actor store, which is strictly
/// more sensitive, and asking would put a network call in every write. When
/// the secret is not configured the asymmetric path is used unchanged, so an
/// entryway deployment needs no different build.
async fn verify_as_signature(
    decoded: &DecodedJwt<AccessClaims>,
    config: &AuthConfig,
    fetcher: &dyn MetadataFetcher,
) -> Result<()> {
    if decoded.header.alg == HS256 {
        let secret = config
            .hs256_secret
            .as_ref()
            .ok_or_else(|| auth_err("token is HS256 but no shared secret is configured"))?;
        return secret
            .verify(&decoded.signing_input, &decoded.signature)
            .then_some(())
            .ok_or_else(|| auth_err("token signature: hs256 verification failed"));
    }
    let jwks: JwkSet = fetcher.jwks(&config.jwks_uri).await?;
    let jwk = match decoded.header.kid.as_deref() {
        Some(kid) => jwks
            .find(kid)
            .ok_or_else(|| auth_err(format!("authorization server has no key {kid}")))?,
        None => jwks
            .keys
            .first()
            .ok_or_else(|| auth_err("authorization server jwks is empty"))?,
    };
    verify_client_es256(jwk, &decoded.signing_input, &decoded.signature)
        .map_err(|e| auth_err(format!("token signature: {e}")))
}

async fn verify_dpop_proof(
    proof: &str,
    token: &str,
    request: &RequestAuth<'_>,
    jti_store: &dyn JtiStore,
    now: u64,
) -> Result<String> {
    let decoded: DecodedJwt<DpopClaims> = decode_jwt(proof)?;
    if decoded.header.typ != DPOP_TYP {
        return Err(auth_err(format!(
            "proof typ {} != {DPOP_TYP}",
            decoded.header.typ
        )));
    }
    if !SUPPORTED_PROOF_ALGS.contains(&decoded.header.alg.as_str()) {
        return Err(auth_err(format!(
            "unsupported proof alg {}",
            decoded.header.alg
        )));
    }
    let jwk = decoded
        .header
        .jwk
        .as_ref()
        .ok_or_else(|| auth_err("proof carries no jwk"))?;
    let verified = match decoded.header.alg.as_str() {
        ES256K => verify_client_es256k(jwk, &decoded.signing_input, &decoded.signature),
        _ => verify_client_es256(jwk, &decoded.signing_input, &decoded.signature),
    };
    verified.map_err(|e| auth_err(format!("proof signature: {e}")))?;

    let claims = &decoded.claims;
    if !claims.htm.eq_ignore_ascii_case(request.method) {
        return Err(auth_err("proof htm does not match the request method"));
    }
    if htu_of(&claims.htu) != htu_of(request.url) {
        return Err(auth_err("proof htu does not match the request url"));
    }
    if claims.ath.as_deref() != Some(access_token_hash(token).as_str()) {
        return Err(auth_err("proof ath does not match the access token"));
    }
    if claims.iat > now + MAX_IAT_SKEW_SECS {
        return Err(auth_err("proof iat is in the future"));
    }
    let expires = claims.iat.saturating_add(MAX_DPOP_AGE_SECS);
    if now >= expires {
        return Err(auth_err("proof is too old"));
    }
    if !jti_store.consume(&claims.jti, expires).await? {
        return Err(auth_err("proof jti replayed"));
    }
    Ok(jwk_thumbprint(jwk))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::attestation::{ClientMetadata, InMemoryJtiStore};
    use async_trait::async_trait;
    use p256::ecdsa::signature::hazmat::PrehashSigner;
    use p256::ecdsa::{Signature, SigningKey};
    use serde_json::json;

    pub(crate) const NOW: u64 = 1_700_000_000;
    pub(crate) const ISSUER: &str = "https://pds.example.com";
    pub(crate) const PDS_DID: &str = "did:web:pds.example.com";
    pub(crate) const CLIENT: &str = "https://blacksky.community/oauth-client-metadata.json";
    pub(crate) const AUTHOR: &str = "did:plc:member";
    pub(crate) const URL: &str = "https://pds.example.com/xrpc/com.atproto.space.createRecord";

    fn as_key() -> SigningKey {
        SigningKey::from_slice(&[0x31u8; 32]).unwrap()
    }

    pub(crate) fn client_key() -> SigningKey {
        SigningKey::from_slice(&[0x32u8; 32]).unwrap()
    }

    pub(crate) fn jwk_of(key: &SigningKey, kid: Option<&str>) -> EcJwk {
        let point = key.verifying_key().to_encoded_point(false);
        let bytes = point.as_bytes();
        EcJwk {
            kty: "EC".to_string(),
            crv: "P-256".to_string(),
            x: URL_SAFE_NO_PAD.encode(&bytes[1..33]),
            y: URL_SAFE_NO_PAD.encode(&bytes[33..65]),
            kid: kid.map(str::to_string),
        }
    }

    pub(crate) fn sign_jwt(
        key: &SigningKey,
        header: serde_json::Value,
        claims: serde_json::Value,
    ) -> String {
        let header = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap());
        let claims = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap());
        let input = format!("{header}.{claims}");
        let digest = Sha256::digest(input.as_bytes());
        let sig: Signature = key.sign_prehash(&digest).unwrap();
        let sig = sig.normalize_s().unwrap_or(sig);
        format!("{input}.{}", URL_SAFE_NO_PAD.encode(sig.to_vec()))
    }

    pub(crate) struct AsJwks;
    #[async_trait]
    impl MetadataFetcher for AsJwks {
        async fn client_metadata(&self, _client_id: &str) -> Result<ClientMetadata> {
            Err(auth_err("not used"))
        }
        async fn jwks(&self, _url: &str) -> Result<JwkSet> {
            Ok(JwkSet {
                keys: vec![jwk_of(&as_key(), Some("as-key-1"))],
            })
        }
    }

    pub(crate) fn config() -> AuthConfig {
        AuthConfig {
            issuer: ISSUER.to_string(),
            jwks_uri: format!("{ISSUER}/oauth/jwks"),
            audience: PDS_DID.to_string(),
            client_ids: vec![CLIENT.to_string()],
            hs256_secret: None,
        }
    }

    /// The claim set of a real stateful-mode token: no `scope`, DID `aud`.
    pub(crate) fn token_claims() -> serde_json::Value {
        json!({
            "iss": ISSUER,
            "aud": PDS_DID,
            "sub": AUTHOR,
            "exp": NOW + 3600,
            "iat": NOW,
            "jti": "tok-1",
            "client_id": CLIENT,
            "cnf": {"jkt": jwk_thumbprint(&jwk_of(&client_key(), None))},
        })
    }

    pub(crate) fn token_with(claims: serde_json::Value) -> String {
        sign_jwt(
            &as_key(),
            json!({"typ": ACCESS_TOKEN_TYP, "alg": "ES256", "kid": "as-key-1"}),
            claims,
        )
    }

    pub(crate) fn token() -> String {
        token_with(token_claims())
    }

    pub(crate) fn proof_claims(token: &str) -> serde_json::Value {
        json!({
            "jti": "proof-1",
            "htm": "POST",
            "htu": URL,
            "iat": NOW,
            "ath": access_token_hash(token),
        })
    }

    pub(crate) fn proof_with(claims: serde_json::Value) -> String {
        sign_jwt(
            &client_key(),
            json!({
                "typ": DPOP_TYP,
                "alg": "ES256",
                "jwk": jwk_of(&client_key(), None),
            }),
            claims,
        )
    }

    async fn check_at(
        token: &str,
        proof: &str,
        method: &str,
        url: &str,
        now: u64,
        jti: &InMemoryJtiStore,
    ) -> Result<AccessContext> {
        let authorization = format!("DPoP {token}");
        verify_access(
            &RequestAuth {
                authorization: Some(&authorization),
                dpop: Some(proof),
                method,
                url,
            },
            &config(),
            &AsJwks,
            jti,
            now,
        )
        .await
    }

    async fn check(token: &str, proof: &str) -> Result<AccessContext> {
        check_at(token, proof, "POST", URL, NOW, &InMemoryJtiStore::default()).await
    }

    pub(crate) fn k1_client_key() -> secp256k1::SecretKey {
        secp256k1::SecretKey::from_slice(&[0x33u8; 32]).unwrap()
    }

    pub(crate) fn k1_jwk_of(key: &secp256k1::SecretKey) -> EcJwk {
        let point = key
            .public_key(secp256k1::SECP256K1)
            .serialize_uncompressed();
        EcJwk {
            kty: "EC".to_string(),
            crv: "secp256k1".to_string(),
            x: URL_SAFE_NO_PAD.encode(&point[1..33]),
            y: URL_SAFE_NO_PAD.encode(&point[33..65]),
            kid: None,
        }
    }

    pub(crate) fn k1_sign_jwt(
        key: &secp256k1::SecretKey,
        header: serde_json::Value,
        claims: serde_json::Value,
    ) -> String {
        let header = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap());
        let claims = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap());
        let input = format!("{header}.{claims}");
        let digest = Sha256::digest(input.as_bytes());
        let message = secp256k1::Message::from_digest_slice(&digest).unwrap();
        let sig = secp256k1::SECP256K1.sign_ecdsa(&message, key);
        format!(
            "{input}.{}",
            URL_SAFE_NO_PAD.encode(sig.serialize_compact())
        )
    }

    /// A proof whose header names `jwk_key` but whose signature is made by
    /// `signing_key`; equal keys give an ordinary well-formed proof.
    fn k1_proof(
        jwk_key: &secp256k1::SecretKey,
        signing_key: &secp256k1::SecretKey,
        claims: serde_json::Value,
    ) -> String {
        k1_sign_jwt(
            signing_key,
            json!({"typ": DPOP_TYP, "alg": ES256K, "jwk": k1_jwk_of(jwk_key)}),
            claims,
        )
    }

    fn token_bound_to(jkt: String) -> String {
        let mut claims = token_claims();
        claims["cnf"] = json!({ "jkt": jkt });
        token_with(claims)
    }

    #[tokio::test]
    async fn accepts_an_es256k_dpop_proof() {
        let key = k1_client_key();
        let token = token_bound_to(jwk_thumbprint(&k1_jwk_of(&key)));
        let proof = k1_proof(&key, &key, proof_claims(&token));
        let context = check(&token, &proof).await.unwrap();
        assert_eq!(context.did, AUTHOR);
        assert_eq!(context.jkt, jwk_thumbprint(&k1_jwk_of(&key)));
    }

    #[tokio::test]
    async fn rejects_an_es256k_proof_signed_by_another_key() {
        let key = k1_client_key();
        let impostor = secp256k1::SecretKey::from_slice(&[0x34u8; 32]).unwrap();
        let token = token_bound_to(jwk_thumbprint(&k1_jwk_of(&key)));
        let proof = k1_proof(&key, &impostor, proof_claims(&token));
        assert!(check(&token, &proof).await.is_err());
    }

    /// The bound key is the one the token names, not whichever key presents a
    /// self-consistent proof.
    #[tokio::test]
    async fn rejects_an_es256k_proof_for_an_unbound_key() {
        let bound = k1_client_key();
        let other = secp256k1::SecretKey::from_slice(&[0x35u8; 32]).unwrap();
        let token = token_bound_to(jwk_thumbprint(&k1_jwk_of(&bound)));
        let proof = k1_proof(&other, &other, proof_claims(&token));
        assert!(check(&token, &proof).await.is_err());
    }

    #[tokio::test]
    async fn rejects_an_es256k_proof_carrying_a_p256_key() {
        let token = token();
        let proof = sign_jwt(
            &client_key(),
            json!({"typ": DPOP_TYP, "alg": ES256K, "jwk": jwk_of(&client_key(), None)}),
            proof_claims(&token),
        );
        assert!(check(&token, &proof).await.is_err());
    }

    #[tokio::test]
    async fn rejects_an_es256_proof_carrying_a_secp256k1_key() {
        let key = k1_client_key();
        let token = token_bound_to(jwk_thumbprint(&k1_jwk_of(&key)));
        let proof = k1_sign_jwt(
            &key,
            json!({"typ": DPOP_TYP, "alg": "ES256", "jwk": k1_jwk_of(&key)}),
            proof_claims(&token),
        );
        assert!(check(&token, &proof).await.is_err());
    }

    #[tokio::test]
    async fn rejects_an_unsupported_proof_alg() {
        let key = k1_client_key();
        let token = token_bound_to(jwk_thumbprint(&k1_jwk_of(&key)));
        let proof = k1_sign_jwt(
            &key,
            json!({"typ": DPOP_TYP, "alg": "ES512", "jwk": k1_jwk_of(&key)}),
            proof_claims(&token),
        );
        assert!(check(&token, &proof).await.is_err());
    }

    const HS_SECRET: &[u8] = b"a-pds-jwt-secret";

    fn hs256_token(claims: serde_json::Value) -> String {
        use hmac::{Hmac, Mac};
        let header = json!({"typ": ACCESS_TOKEN_TYP, "alg": "HS256"});
        let input = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap()),
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
        );
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(HS_SECRET).unwrap();
        mac.update(input.as_bytes());
        format!(
            "{input}.{}",
            URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
        )
    }

    fn hs_config() -> AuthConfig {
        AuthConfig {
            hs256_secret: VerifyOnlyHs256Secret::new(HS_SECRET.to_vec()),
            ..config()
        }
    }

    /// A PDS that is its own authorization server signs with HS256 and
    /// publishes no public key, so this is the only path that can verify a
    /// real token from one.
    #[tokio::test]
    async fn accepts_an_hs256_token_from_a_standalone_pds() {
        let token = hs256_token(token_claims());
        let proof = proof_with(proof_claims(&token));
        let authorization = format!("DPoP {token}");
        let context = verify_access(
            &RequestAuth {
                authorization: Some(&authorization),
                dpop: Some(&proof),
                method: "POST",
                url: URL,
            },
            &hs_config(),
            &AsJwks,
            &InMemoryJtiStore::default(),
            NOW,
        )
        .await
        .expect("hs256 token rejected");
        assert_eq!(context.did, AUTHOR);
    }

    #[tokio::test]
    async fn an_hs256_token_is_refused_without_the_secret_or_with_the_wrong_one() {
        let token = hs256_token(token_claims());
        let proof = proof_with(proof_claims(&token));
        let authorization = format!("DPoP {token}");
        let attempt = |config: AuthConfig| {
            let authorization = authorization.clone();
            let proof = proof.clone();
            async move {
                verify_access(
                    &RequestAuth {
                        authorization: Some(&authorization),
                        dpop: Some(&proof),
                        method: "POST",
                        url: URL,
                    },
                    &config,
                    &AsJwks,
                    &InMemoryJtiStore::default(),
                    NOW,
                )
                .await
            }
        };
        // Unconfigured: an HS256 token must not fall through to the JWKS path.
        assert!(attempt(config()).await.is_err());
        assert!(attempt(AuthConfig {
            hs256_secret: VerifyOnlyHs256Secret::new(b"not-the-secret".to_vec()),
            ..config()
        })
        .await
        .is_err());
    }

    #[tokio::test]
    async fn a_tampered_hs256_token_is_refused() {
        let mut claims = token_claims();
        claims["sub"] = json!("did:plc:someone-else");
        let token = hs256_token(token_claims());
        let forged = format!(
            "{}.{}.{}",
            token.split('.').next().unwrap(),
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap()),
            token.split('.').nth(2).unwrap()
        );
        let proof = proof_with(proof_claims(&forged));
        let authorization = format!("DPoP {forged}");
        assert!(verify_access(
            &RequestAuth {
                authorization: Some(&authorization),
                dpop: Some(&proof),
                method: "POST",
                url: URL,
            },
            &hs_config(),
            &AsJwks,
            &InMemoryJtiStore::default(),
            NOW,
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn accepts_a_real_shaped_token() {
        let token = token();
        let context = check(&token, &proof_with(proof_claims(&token)))
            .await
            .unwrap();
        assert_eq!(context.did, AUTHOR);
        assert_eq!(context.client_id, CLIENT);
        assert_eq!(context.jkt, jwk_thumbprint(&jwk_of(&client_key(), None)));
    }

    #[tokio::test]
    async fn accepts_a_token_with_no_scope_claim() {
        // Stateful-mode tokens omit `scope` entirely; that must not be fatal.
        let claims = token_claims();
        assert!(claims.get("scope").is_none());
        let token = token_with(claims);
        assert!(check(&token, &proof_with(proof_claims(&token)))
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn rejects_an_origin_url_audience() {
        // The regression this suite exists for: `aud` is the service DID, and a
        // vector built from the origin would pass a lenient check but 401 in
        // production.
        let mut claims = token_claims();
        claims["aud"] = json!(ISSUER);
        let token = token_with(claims);
        assert!(check(&token, &proof_with(proof_claims(&token)))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn accepts_a_did_audience_in_a_list() {
        let mut claims = token_claims();
        claims["aud"] = json!(["did:web:other.example", PDS_DID]);
        let token = token_with(claims);
        assert!(check(&token, &proof_with(proof_claims(&token)))
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn rejects_bad_token_claims() {
        for (name, mutate) in [
            ("wrong iss", json!("https://evil.example")),
            ("wrong aud", json!("did:web:other.example")),
        ] {
            let mut claims = token_claims();
            let field = if name == "wrong iss" { "iss" } else { "aud" };
            claims[field] = mutate;
            let token = token_with(claims);
            assert!(
                check(&token, &proof_with(proof_claims(&token)))
                    .await
                    .is_err(),
                "{name} was accepted"
            );
        }

        // Expired.
        let mut claims = token_claims();
        claims["exp"] = json!(NOW - 1);
        let token = token_with(claims);
        assert!(check(&token, &proof_with(proof_claims(&token)))
            .await
            .is_err());

        // Not dpop-bound.
        let mut claims = token_claims();
        claims["cnf"] = json!({});
        let token = token_with(claims);
        assert!(check(&token, &proof_with(proof_claims(&token)))
            .await
            .is_err());

        // Bound to another key.
        let mut claims = token_claims();
        claims["cnf"] = json!({"jkt": "someone-elses-thumbprint"});
        let token = token_with(claims);
        assert!(check(&token, &proof_with(proof_claims(&token)))
            .await
            .is_err());

        // No client_id.
        let mut claims = token_claims();
        claims.as_object_mut().unwrap().remove("client_id");
        let token = token_with(claims);
        assert!(check(&token, &proof_with(proof_claims(&token)))
            .await
            .is_err());

        // Non-allowlisted client_id.
        let mut claims = token_claims();
        claims["client_id"] = json!("https://third-party.example/client-metadata.json");
        let token = token_with(claims);
        assert!(check(&token, &proof_with(proof_claims(&token)))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn rejects_a_wrong_token_typ() {
        let token = sign_jwt(
            &as_key(),
            json!({"typ": "JWT", "alg": "ES256", "kid": "as-key-1"}),
            token_claims(),
        );
        assert!(check(&token, &proof_with(proof_claims(&token)))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn rejects_a_token_signed_by_another_key() {
        let token = sign_jwt(
            &client_key(),
            json!({"typ": ACCESS_TOKEN_TYP, "alg": "ES256", "kid": "as-key-1"}),
            token_claims(),
        );
        assert!(check(&token, &proof_with(proof_claims(&token)))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn rejects_a_tampered_token_signature() {
        let token = token();
        let mut tampered: Vec<&str> = token.split('.').collect();
        let flipped = format!("{}A", &tampered[2][..tampered[2].len() - 1]);
        tampered[2] = &flipped;
        let tampered = tampered.join(".");
        assert!(check(&tampered, &proof_with(proof_claims(&tampered)))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn rejects_an_unknown_signing_key_id() {
        let token = sign_jwt(
            &as_key(),
            json!({"typ": ACCESS_TOKEN_TYP, "alg": "ES256", "kid": "rotated-away"}),
            token_claims(),
        );
        assert!(check(&token, &proof_with(proof_claims(&token)))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn rejects_bad_dpop_proofs() {
        let token = token();

        // Wrong htu.
        let mut claims = proof_claims(&token);
        claims["htu"] = json!("https://pds.example.com/xrpc/com.atproto.space.deleteRecord");
        assert!(check(&token, &proof_with(claims)).await.is_err());

        // Wrong htm.
        let mut claims = proof_claims(&token);
        claims["htm"] = json!("GET");
        assert!(check(&token, &proof_with(claims)).await.is_err());

        // ath bound to a different token.
        let mut claims = proof_claims(&token);
        claims["ath"] = json!(access_token_hash("some.other.token"));
        assert!(check(&token, &proof_with(claims)).await.is_err());

        // Stale proof.
        let mut claims = proof_claims(&token);
        claims["iat"] = json!(NOW - MAX_DPOP_AGE_SECS - 1);
        assert!(check(&token, &proof_with(claims)).await.is_err());

        // Proof from the future.
        let mut claims = proof_claims(&token);
        claims["iat"] = json!(NOW + MAX_IAT_SKEW_SECS + 10);
        assert!(check(&token, &proof_with(claims)).await.is_err());

        // Proof signed by a key other than the one it embeds.
        let forged = sign_jwt(
            &as_key(),
            json!({"typ": DPOP_TYP, "alg": "ES256", "jwk": jwk_of(&client_key(), None)}),
            proof_claims(&token),
        );
        assert!(check(&token, &forged).await.is_err());

        // Proof with no embedded key.
        let no_jwk = sign_jwt(
            &client_key(),
            json!({"typ": DPOP_TYP, "alg": "ES256"}),
            proof_claims(&token),
        );
        assert!(check(&token, &no_jwk).await.is_err());

        // Wrong proof typ.
        let wrong_typ = sign_jwt(
            &client_key(),
            json!({"typ": "JWT", "alg": "ES256", "jwk": jwk_of(&client_key(), None)}),
            proof_claims(&token),
        );
        assert!(check(&token, &wrong_typ).await.is_err());
    }

    #[tokio::test]
    async fn rejects_a_replayed_proof() {
        let jti = InMemoryJtiStore::default();
        let token = token();
        let proof = proof_with(proof_claims(&token));
        assert!(check_at(&token, &proof, "POST", URL, NOW, &jti)
            .await
            .is_ok());
        assert!(check_at(&token, &proof, "POST", URL, NOW, &jti)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn htu_ignores_query_and_fragment() {
        let token = token();
        let mut claims = proof_claims(&token);
        claims["htu"] = json!(format!("{URL}?x=1"));
        let jti = InMemoryJtiStore::default();
        assert!(check_at(
            &token,
            &proof_with(claims),
            "POST",
            &format!("{URL}#frag"),
            NOW,
            &jti
        )
        .await
        .is_ok());
    }

    #[tokio::test]
    async fn rejects_malformed_and_missing_headers() {
        let token = token();
        let proof = proof_with(proof_claims(&token));
        let jti = InMemoryJtiStore::default();

        // Bearer instead of DPoP.
        let bearer = format!("Bearer {token}");
        assert!(verify_access(
            &RequestAuth {
                authorization: Some(&bearer),
                dpop: Some(&proof),
                method: "POST",
                url: URL,
            },
            &config(),
            &AsJwks,
            &jti,
            NOW,
        )
        .await
        .is_err());

        // No authorization header.
        assert!(verify_access(
            &RequestAuth {
                authorization: None,
                dpop: Some(&proof),
                method: "POST",
                url: URL,
            },
            &config(),
            &AsJwks,
            &jti,
            NOW,
        )
        .await
        .is_err());

        // No DPoP proof header.
        let authorization = format!("DPoP {token}");
        assert!(verify_access(
            &RequestAuth {
                authorization: Some(&authorization),
                dpop: None,
                method: "POST",
                url: URL,
            },
            &config(),
            &AsJwks,
            &jti,
            NOW,
        )
        .await
        .is_err());

        // Header with no scheme separator.
        assert!(verify_access(
            &RequestAuth {
                authorization: Some("DPoPnospace"),
                dpop: Some(&proof),
                method: "POST",
                url: URL,
            },
            &config(),
            &AsJwks,
            &jti,
            NOW,
        )
        .await
        .is_err());

        // Structurally broken JWTs.
        assert!(check("not.a.jwt", &proof).await.is_err());
        assert!(check(&token, "only.two").await.is_err());
    }

    #[tokio::test]
    async fn a_token_may_only_write_as_its_own_subject() {
        let token = token();
        let context = check(&token, &proof_with(proof_claims(&token)))
            .await
            .unwrap();
        assert!(context.require_author(AUTHOR).is_ok());
        assert!(context.require_author("did:plc:someoneelse").is_err());
    }

    #[test]
    fn config_requires_a_did_audience_and_a_client_allowlist() {
        assert!(config().validate().is_ok());

        let mut origin_aud = config();
        origin_aud.audience = ISSUER.to_string();
        assert!(origin_aud.validate().is_err());

        let mut no_clients = config();
        no_clients.client_ids.clear();
        assert!(no_clients.validate().is_err());
    }

    #[test]
    fn thumbprint_uses_the_canonical_member_order() {
        let jwk = EcJwk {
            kty: "EC".to_string(),
            crv: "P-256".to_string(),
            x: "eA".to_string(),
            y: "eQ".to_string(),
            kid: Some("ignored".to_string()),
        };
        let expected = URL_SAFE_NO_PAD.encode(Sha256::digest(
            r#"{"crv":"P-256","kty":"EC","x":"eA","y":"eQ"}"#,
        ));
        assert_eq!(jwk_thumbprint(&jwk), expected);
    }
}
