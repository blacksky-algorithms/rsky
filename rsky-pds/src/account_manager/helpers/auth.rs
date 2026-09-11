use crate::account_manager::helpers::password::AppPassDescript;
use crate::auth_verifier::AuthScope;
use crate::db::sqlite::Db;
use anyhow::{bail, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::DateTime;
use hmac::{Hmac, Mac};
use rsky_common::env::env_str;
use rsky_common::{get_random_str, json_to_b64url, RFC3339_VARIANT};
use rusqlite::{params, OptionalExtension, Transaction};
use secp256k1::ecdsa::Signature;
use secp256k1::{Keypair, Message, Secp256k1, SecretKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::LazyLock;
use std::time::SystemTime;
use thiserror::Error;

pub const ACCESS_TOKEN_TYP: &str = "at+jwt";
pub const REFRESH_TOKEN_TYP: &str = "refresh+jwt";
const ACCESS_TOKEN_LIFETIME_SECS: u64 = 2 * 60 * 60;
const REFRESH_TOKEN_LIFETIME_SECS: u64 = 90 * 24 * 60 * 60;

pub struct CreateTokensOpts {
    pub did: String,
    pub service_did: String,
    pub scope: Option<AuthScope>,
    pub jti: Option<String>,
    pub expires_in_secs: Option<u64>,
    pub issued_at: Option<u64>,
}

pub struct RefreshGracePeriodOpts {
    pub id: String,
    pub expires_at: String,
    pub next_id: String,
}

#[derive(Debug)]
pub struct AuthToken {
    pub scope: AuthScope,
    pub sub: String,
    pub exp: u64,
}

#[derive(Debug)]
pub struct RefreshToken {
    pub scope: AuthScope, // AuthScope::Refresh
    pub sub: String,
    pub exp: u64,
    pub jti: String,
}

/// A `refresh_token` row joined with the app password it was issued for.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredRefreshToken {
    pub id: String,
    pub did: String,
    pub expires_at: String,
    pub next_id: Option<String>,
    pub app_password: Option<AppPassDescript>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ServiceJwtPayload {
    pub iss: String,
    pub aud: String,
    pub exp: Option<u64>,
    pub lxm: Option<String>,
    pub jti: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ServiceJwtHeader {
    pub typ: String,
    pub alg: String,
}

pub struct ServiceJwtParams {
    pub iss: String,
    pub aud: String,
    pub exp: Option<u64>,
    pub lxm: Option<String>,
    pub jti: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct CustomClaimObj {
    pub scope: String,
}

#[derive(Error, Debug)]
pub enum AuthHelperError {
    #[error("ConcurrentRefreshError")]
    ConcurrentRefresh,
}

/// Why a session token was rejected. The messages are the ones the reference
/// PDS returns for the same token.
#[derive(Error, Debug, PartialEq)]
pub enum SessionTokenError {
    #[error("Token has expired")]
    Expired,
    #[error("Token could not be verified")]
    Invalid,
    #[error("Malformed token")]
    Malformed,
}

/// The claims of a verified session token.
#[derive(Debug, Clone, Deserialize)]
pub struct SessionClaims {
    pub scope: String,
    pub sub: Option<String>,
    #[serde(default)]
    pub aud: Option<serde_json::Value>,
    pub exp: Option<u64>,
    pub nbf: Option<u64>,
    pub iat: Option<u64>,
    pub jti: Option<String>,
    #[serde(default)]
    lxm: Option<String>,
    #[serde(default)]
    cnf: Option<serde_json::Value>,
}

/// Standard-claim checks applied after the signature is verified.
#[derive(Debug, Clone, Default)]
pub struct SessionVerifyOptions {
    pub audience: Option<String>,
    pub allow_expired: bool,
}

#[derive(Serialize)]
struct SessionHeader<'a> {
    typ: &'a str,
    alg: &'a str,
}

#[derive(Serialize)]
struct AccessClaims<'a> {
    scope: &'a str,
    aud: &'a str,
    sub: &'a str,
    iat: u64,
    exp: u64,
}

#[derive(Serialize)]
struct RefreshClaims<'a> {
    scope: &'a str,
    jti: &'a str,
    aud: &'a str,
    sub: &'a str,
    iat: u64,
    exp: u64,
}

/// Signs and verifies session tokens. HMAC-SHA256 over `PDS_JWT_SECRET` is the
/// reference PDS's scheme, so tokens are interchangeable with it; ES256K over
/// `PDS_JWT_KEY_K256_PRIVATE_KEY_HEX` is rsky's original scheme. One is active
/// per process and a token signed under the other is rejected.
pub enum JwtSigner {
    Hmac { secret: Vec<u8> },
    K256 { keypair: Keypair },
}

pub static PDS_JWT_SIGNER: LazyLock<JwtSigner> =
    LazyLock::new(|| JwtSigner::from_env().expect("session token signing key"));

impl JwtSigner {
    pub fn hmac(secret: &[u8]) -> Self {
        JwtSigner::Hmac {
            secret: secret.to_vec(),
        }
    }

    pub fn k256(private_key_hex: &str) -> Result<Self> {
        let secret_key = SecretKey::from_slice(&hex::decode(private_key_hex)?)?;
        let keypair = Keypair::from_secret_key(&Secp256k1::new(), &secret_key);
        Ok(JwtSigner::K256 { keypair })
    }

    pub fn from_env() -> Result<Self> {
        Self::from_config(
            env_str("PDS_JWT_SECRET"),
            env_str("PDS_JWT_KEY_K256_PRIVATE_KEY_HEX"),
        )
    }

    /// The shared secret wins when both are configured, so a deployment
    /// interoperating with the reference PDS never issues ES256K tokens.
    pub fn from_config(secret: Option<String>, private_key_hex: Option<String>) -> Result<Self> {
        if let Some(secret) = secret {
            return Ok(Self::hmac(secret.as_bytes()));
        }
        match private_key_hex {
            Some(private_key_hex) => Self::k256(&private_key_hex),
            None => bail!("PDS_JWT_SECRET or PDS_JWT_KEY_K256_PRIVATE_KEY_HEX must be set"),
        }
    }

    pub fn alg(&self) -> &'static str {
        match self {
            JwtSigner::Hmac { .. } => "HS256",
            JwtSigner::K256 { .. } => "ES256K",
        }
    }

    fn signature(&self, signing_input: &[u8]) -> Vec<u8> {
        match self {
            JwtSigner::Hmac { secret } => {
                let mut mac =
                    Hmac::<Sha256>::new_from_slice(secret).expect("hmac accepts any key length");
                mac.update(signing_input);
                mac.finalize().into_bytes().to_vec()
            }
            JwtSigner::K256 { keypair } => {
                let message = Message::from_digest(Sha256::digest(signing_input).into());
                let mut sig = keypair.secret_key().sign_ecdsa(message);
                sig.normalize_s();
                sig.serialize_compact().to_vec()
            }
        }
    }

    fn signature_is_valid(&self, signing_input: &[u8], signature: &[u8]) -> bool {
        match self {
            JwtSigner::Hmac { secret } => {
                let mut mac =
                    Hmac::<Sha256>::new_from_slice(secret).expect("hmac accepts any key length");
                mac.update(signing_input);
                mac.verify_slice(signature).is_ok()
            }
            JwtSigner::K256 { keypair } => {
                let Ok(mut sig) = Signature::from_compact(signature) else {
                    return false;
                };
                sig.normalize_s();
                let message = Message::from_digest(Sha256::digest(signing_input).into());
                sig.verify(&message, &keypair.public_key()).is_ok()
            }
        }
    }

    /// Signs `claims` the way the reference PDS does: a `{"typ","alg"}` header
    /// and the claims in the order the struct declares them.
    pub fn sign<C: Serialize>(&self, typ: &str, claims: &C) -> Result<String> {
        let header = SessionHeader {
            typ,
            alg: self.alg(),
        };
        let signing_input = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header)?),
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims)?)
        );
        let signature = URL_SAFE_NO_PAD.encode(self.signature(signing_input.as_bytes()));
        Ok(format!("{signing_input}.{signature}"))
    }

    /// Verifies the signature, the algorithm, the token type, and the
    /// standard claims, with the reference PDS's rules: `exp` is checked
    /// without tolerance, `iat` is not checked, and a token carrying `lxm`
    /// or `cnf` is not a session token.
    pub fn verify(
        &self,
        jwt: &str,
        expected_typ: &str,
        options: &SessionVerifyOptions,
    ) -> Result<SessionClaims, SessionTokenError> {
        let mut parts = jwt.split('.');
        let (Some(header), Some(payload), Some(signature), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(SessionTokenError::Invalid);
        };
        let header: serde_json::Value = URL_SAFE_NO_PAD
            .decode(header)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .ok_or(SessionTokenError::Invalid)?;
        if header["alg"].as_str() != Some(self.alg()) {
            return Err(SessionTokenError::Invalid);
        }
        let typ_matches = match header["typ"].as_str() {
            Some(typ) => normalize_typ(typ) == normalize_typ(expected_typ),
            None => false,
        };
        // tokens rsky issued before it set a token type carry jose's default
        let legacy_typ = matches!(self, JwtSigner::K256 { .. })
            && matches!(header["typ"].as_str(), None | Some("JWT"));
        if !typ_matches && !legacy_typ {
            return Err(SessionTokenError::Invalid);
        }
        let signature = URL_SAFE_NO_PAD
            .decode(signature)
            .map_err(|_| SessionTokenError::Invalid)?;
        let signing_input = &jwt[..header_and_payload_len(jwt)];
        if !self.signature_is_valid(signing_input.as_bytes(), &signature) {
            return Err(SessionTokenError::Invalid);
        }
        let claims: SessionClaims = URL_SAFE_NO_PAD
            .decode(payload)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .ok_or(SessionTokenError::Invalid)?;
        if claims.lxm.is_some() || claims.cnf.is_some() {
            return Err(SessionTokenError::Malformed);
        }
        let now = now_secs();
        if let Some(nbf) = claims.nbf {
            if nbf > now {
                return Err(SessionTokenError::Invalid);
            }
        }
        if !options.allow_expired {
            match claims.exp {
                Some(exp) if exp > now => {}
                _ => return Err(SessionTokenError::Expired),
            }
        }
        if let Some(audience) = &options.audience {
            if !claims.has_audience(audience) {
                return Err(SessionTokenError::Invalid);
            }
        }
        Ok(claims)
    }
}

impl SessionClaims {
    pub fn has_audience(&self, audience: &str) -> bool {
        match &self.aud {
            Some(serde_json::Value::String(aud)) => aud == audience,
            Some(serde_json::Value::Array(auds)) => auds.iter().any(|aud| aud == audience),
            _ => false,
        }
    }

    /// The single audience string, or `None` for a missing or list audience.
    pub fn audience(&self) -> Option<String> {
        match &self.aud {
            Some(serde_json::Value::String(aud)) => Some(aud.clone()),
            _ => None,
        }
    }
}

fn header_and_payload_len(jwt: &str) -> usize {
    jwt.rfind('.').unwrap_or(jwt.len())
}

fn normalize_typ(typ: &str) -> String {
    typ.to_lowercase()
        .trim_start_matches("application/")
        .to_owned()
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("timestamp since UNIX epoch")
        .as_secs()
}

pub fn create_tokens(opts: CreateTokensOpts) -> Result<(String, String)> {
    create_tokens_with(&PDS_JWT_SIGNER, opts)
}

pub fn create_tokens_with(signer: &JwtSigner, opts: CreateTokensOpts) -> Result<(String, String)> {
    let CreateTokensOpts {
        did,
        service_did,
        scope,
        jti,
        expires_in_secs,
        issued_at,
    } = opts;
    let access_jwt = create_access_token_with(
        signer,
        CreateTokensOpts {
            did: did.clone(),
            service_did: service_did.clone(),
            scope,
            expires_in_secs,
            issued_at,
            jti: None,
        },
    )?;
    let refresh_jwt = create_refresh_token_with(
        signer,
        CreateTokensOpts {
            did,
            service_did,
            jti,
            expires_in_secs,
            issued_at,
            scope: None,
        },
    )?;
    Ok((access_jwt, refresh_jwt))
}

pub fn create_access_token_with(signer: &JwtSigner, opts: CreateTokensOpts) -> Result<String> {
    let CreateTokensOpts {
        did,
        service_did,
        scope,
        expires_in_secs,
        issued_at,
        ..
    } = opts;
    let scope = scope.unwrap_or(AuthScope::Access);
    let iat = issued_at.unwrap_or_else(now_secs);
    let exp = iat + expires_in_secs.unwrap_or(ACCESS_TOKEN_LIFETIME_SECS);
    signer.sign(
        ACCESS_TOKEN_TYP,
        &AccessClaims {
            scope: scope.as_str(),
            aud: &service_did,
            sub: &did,
            iat,
            exp,
        },
    )
}

pub fn create_refresh_token_with(signer: &JwtSigner, opts: CreateTokensOpts) -> Result<String> {
    let CreateTokensOpts {
        did,
        service_did,
        jti,
        expires_in_secs,
        issued_at,
        ..
    } = opts;
    let jti = jti.unwrap_or_else(get_refresh_token_id);
    let iat = issued_at.unwrap_or_else(now_secs);
    let exp = iat + expires_in_secs.unwrap_or(REFRESH_TOKEN_LIFETIME_SECS);
    signer.sign(
        REFRESH_TOKEN_TYP,
        &RefreshClaims {
            scope: AuthScope::Refresh.as_str(),
            jti: &jti,
            aud: &service_did,
            sub: &did,
            iat,
            exp,
        },
    )
}

/// The session scope for a login: taken-down accounts get a restricted
/// scope, app passwords their own, everything else full access.
pub fn format_scope(app_password: Option<&AppPassDescript>, is_soft_deleted: bool) -> AuthScope {
    if is_soft_deleted {
        return AuthScope::Takendown;
    }
    match app_password {
        None => AuthScope::Access,
        Some(app_password) if app_password.privileged => AuthScope::AppPassPrivileged,
        Some(_) => AuthScope::AppPass,
    }
}

/// `keypair` must be the signing key of the account named in `params.iss`:
/// verifiers resolve `iss` to its DID document and check the signature there.
pub async fn create_service_jwt(params: ServiceJwtParams, keypair: &Keypair) -> Result<String> {
    let ServiceJwtParams { iss, aud, .. } = params;
    // `exp` is seconds since the epoch (RFC 7519 §4.1.4).
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("timestamp since UNIX epoch")
        .as_secs();
    let exp = params.exp.unwrap_or(now + 60);
    let lxm = params.lxm;
    let jti = get_random_str();
    let header = ServiceJwtHeader {
        typ: "JWT".to_string(),
        alg: "ES256K".to_string(),
    };
    let payload = ServiceJwtPayload {
        iss,
        aud,
        exp: Some(exp),
        lxm,
        jti: Some(jti),
    };
    let to_sign_str = format!(
        "{0}.{1}",
        json_to_b64url(&header)?,
        json_to_b64url(&payload)?
    );
    let hash = Sha256::digest(to_sign_str.clone());
    let message = Message::from_digest_slice(hash.as_ref())?;
    let mut sig = keypair.secret_key().sign_ecdsa(message);
    // Convert to low-s
    sig.normalize_s();
    // ASN.1 encoded per decode_dss_signature
    let compact_sig = sig.serialize_compact();
    Ok(format!(
        "{0}.{1}",
        to_sign_str,
        base64_url::encode(&compact_sig).replace("=", "") // Base 64 encode signature bytes
    ))
}

// @NOTE unsafe for verification, should only be used w/ direct output from createRefreshToken() or createTokens()
pub fn decode_refresh_token(jwt: String) -> Result<RefreshToken> {
    decode_refresh_token_with(&PDS_JWT_SIGNER, jwt)
}

pub fn decode_refresh_token_with(signer: &JwtSigner, jwt: String) -> Result<RefreshToken> {
    let claims = signer.verify(&jwt, REFRESH_TOKEN_TYP, &SessionVerifyOptions::default())?;
    let scope = AuthScope::from_str(&claims.scope)?;
    if scope != AuthScope::Refresh {
        bail!("not a refresh token");
    }
    Ok(RefreshToken {
        scope,
        sub: claims
            .sub
            .ok_or_else(|| anyhow::anyhow!("refresh token has no subject"))?,
        exp: claims
            .exp
            .ok_or_else(|| anyhow::anyhow!("refresh token has no expiry"))?,
        jti: claims
            .jti
            .ok_or_else(|| anyhow::anyhow!("refresh token has no id"))?,
    })
}

pub async fn store_refresh_token(
    payload: RefreshToken,
    app_password_name: Option<String>,
    db: &Db,
) -> Result<()> {
    db.run(move |conn| {
        let tx = conn.transaction()?;
        store_refresh_token_tx(&tx, &payload, app_password_name.as_deref())?;
        Ok(tx.commit()?)
    })
    .await
}

fn store_refresh_token_tx(
    tx: &Transaction,
    payload: &RefreshToken,
    app_password_name: Option<&str>,
) -> Result<()> {
    let exp = DateTime::from_timestamp(payload.exp as i64, 0)
        .ok_or_else(|| anyhow::anyhow!("token expiry out of range"))?;
    let expires_at = format!("{}", exp.format(RFC3339_VARIANT));
    // ON CONFLICT DO NOTHING e.g. when re-granting during a refresh grace period
    tx.execute(
        "INSERT INTO refresh_token (id, did, \"appPasswordName\", \"expiresAt\") \
         VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT (id) DO NOTHING",
        params![payload.jti, payload.sub, app_password_name, expires_at],
    )?;
    Ok(())
}

pub async fn revoke_refresh_token(id: String, db: &Db) -> Result<bool> {
    db.run(move |conn| {
        let deleted = conn.execute("DELETE FROM refresh_token WHERE id = ?1", params![id])?;
        Ok(deleted > 0)
    })
    .await
}

pub async fn revoke_refresh_tokens_by_did(did: &str, db: &Db) -> Result<bool> {
    let did = did.to_owned();
    db.run(move |conn| {
        let deleted = conn.execute("DELETE FROM refresh_token WHERE did = ?1", params![did])?;
        Ok(deleted > 0)
    })
    .await
}

pub async fn revoke_app_password_refresh_token(
    did: &str,
    app_pass_name: &str,
    db: &Db,
) -> Result<bool> {
    let did = did.to_owned();
    let app_pass_name = app_pass_name.to_owned();
    db.run(move |conn| {
        let deleted = conn.execute(
            "DELETE FROM refresh_token WHERE did = ?1 AND \"appPasswordName\" = ?2",
            params![did, app_pass_name],
        )?;
        Ok(deleted > 0)
    })
    .await
}

pub async fn get_refresh_token(id: &str, db: &Db) -> Result<Option<StoredRefreshToken>> {
    let id = id.to_owned();
    db.run(move |conn| {
        Ok(conn
            .query_row(
                "SELECT refresh_token.id, refresh_token.did, refresh_token.\"expiresAt\", \
                 refresh_token.\"nextId\", refresh_token.\"appPasswordName\", app_password.privileged \
                 FROM refresh_token \
                 LEFT JOIN app_password ON app_password.did = refresh_token.did \
                 AND app_password.name = refresh_token.\"appPasswordName\" \
                 WHERE refresh_token.id = ?1",
                params![id],
                |row| {
                    let app_password_name: Option<String> = row.get(4)?;
                    let privileged: Option<i64> = row.get(5)?;
                    Ok(StoredRefreshToken {
                        id: row.get(0)?,
                        did: row.get(1)?,
                        expires_at: row.get(2)?,
                        next_id: row.get(3)?,
                        app_password: app_password_name.map(|name| AppPassDescript {
                            name,
                            privileged: privileged == Some(1),
                        }),
                    })
                },
            )
            .optional()?)
    })
    .await
}

pub async fn delete_expired_refresh_tokens(did: &str, now: String, db: &Db) -> Result<()> {
    let did = did.to_owned();
    db.run(move |conn| {
        conn.execute(
            "DELETE FROM refresh_token WHERE did = ?1 AND \"expiresAt\" <= ?2",
            params![did, now],
        )?;
        Ok(())
    })
    .await
}

pub async fn add_refresh_grace_period(opts: RefreshGracePeriodOpts, db: &Db) -> Result<()> {
    db.run(move |conn| {
        let tx = conn.transaction()?;
        add_refresh_grace_period_tx(&tx, &opts)?;
        Ok(tx.commit()?)
    })
    .await
}

fn add_refresh_grace_period_tx(tx: &Transaction, opts: &RefreshGracePeriodOpts) -> Result<()> {
    let updated = tx.execute(
        "UPDATE refresh_token SET \"expiresAt\" = ?1, \"nextId\" = ?2 \
         WHERE id = ?3 AND (\"nextId\" IS NULL OR \"nextId\" = ?2)",
        params![opts.expires_at, opts.next_id, opts.id],
    )?;
    if updated < 1 {
        return Err(anyhow::Error::new(AuthHelperError::ConcurrentRefresh));
    }
    Ok(())
}

/// Shortens the old token to its grace period and stores its successor in
/// one transaction, so a crash between the two cannot strand a session.
pub async fn rotate_refresh_token_rows(
    grace: RefreshGracePeriodOpts,
    next: RefreshToken,
    app_password_name: Option<String>,
    db: &Db,
) -> Result<()> {
    db.run(move |conn| {
        let tx = conn.transaction()?;
        add_refresh_grace_period_tx(&tx, &grace)?;
        store_refresh_token_tx(&tx, &next, app_password_name.as_deref())?;
        Ok(tx.commit()?)
    })
    .await
}

pub fn get_refresh_token_id() -> String {
    get_random_str()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const SECRET: &[u8] = b"0f0e0d0c0b0a09080706050403020100ffeeddccbbaa99887766554433221100";
    const K256_HEX: &str = "9d5907143471e8f0e8df0f8b9512a8c5377878ee767f18fcf961055ecfc071cd";

    fn fixture_manifest() -> serde_json::Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/ts-pds-0.5.27/manifest.json");
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    fn opts(
        did: &str,
        scope: Option<AuthScope>,
        jti: Option<&str>,
        iat: u64,
        exp: u64,
    ) -> CreateTokensOpts {
        CreateTokensOpts {
            did: did.to_owned(),
            service_did: "did:web:fixture.test".to_owned(),
            scope,
            jti: jti.map(str::to_owned),
            expires_in_secs: Some(exp - iat),
            issued_at: Some(iat),
        }
    }

    /// Tokens minted over the fixture's secret are byte-identical to the
    /// ones the reference PDS minted with jose.
    #[test]
    fn hmac_tokens_match_the_reference_bytes() {
        let manifest = fixture_manifest();
        let secret = manifest["secrets"]["PDS_JWT_SECRET"].as_str().unwrap();
        let signer = JwtSigner::hmac(secret.as_bytes());
        let did = manifest["accounts"]["alice"]["did"].as_str().unwrap();
        let tokens = &manifest["tokens"];
        let iat = tokens["iat"].as_u64().unwrap();
        let exp = tokens["exp"].as_u64().unwrap();
        for (name, scope) in [
            ("alice_access", AuthScope::Access),
            ("alice_app_password_access", AuthScope::AppPass),
            (
                "alice_app_password_privileged_access",
                AuthScope::AppPassPrivileged,
            ),
        ] {
            let token =
                create_access_token_with(&signer, opts(did, Some(scope), None, iat, exp)).unwrap();
            assert_eq!(token, tokens[name].as_str().unwrap(), "{name}");
        }
        let jti = tokens["alice_refresh_jti"].as_str().unwrap();
        let refresh =
            create_refresh_token_with(&signer, opts(did, None, Some(jti), iat, exp)).unwrap();
        assert_eq!(refresh, tokens["alice_refresh"].as_str().unwrap());
        let decoded = decode_refresh_token_with(&signer, refresh).unwrap();
        assert_eq!(decoded.jti, jti);
        assert_eq!(decoded.sub, did);
        assert_eq!(decoded.exp, exp);
    }

    #[test]
    fn verification_enforces_algorithm_type_expiry_and_audience() {
        let manifest = fixture_manifest();
        let signer = JwtSigner::hmac(
            manifest["secrets"]["PDS_JWT_SECRET"]
                .as_str()
                .unwrap()
                .as_bytes(),
        );
        let tokens = &manifest["tokens"];
        let aud = SessionVerifyOptions {
            audience: Some("did:web:fixture.test".to_owned()),
            allow_expired: false,
        };
        let access = tokens["alice_access"].as_str().unwrap();
        assert!(signer.verify(access, ACCESS_TOKEN_TYP, &aud).is_ok());
        assert_eq!(
            signer.verify(access, REFRESH_TOKEN_TYP, &aud).unwrap_err(),
            SessionTokenError::Invalid
        );
        assert_eq!(
            signer
                .verify(
                    tokens["alice_access_typ_jwt"].as_str().unwrap(),
                    ACCESS_TOKEN_TYP,
                    &aud
                )
                .unwrap_err(),
            SessionTokenError::Invalid
        );
        assert_eq!(
            signer
                .verify(
                    tokens["alice_expired_access"].as_str().unwrap(),
                    ACCESS_TOKEN_TYP,
                    &aud
                )
                .unwrap_err(),
            SessionTokenError::Expired
        );
        let expired_ok = SessionVerifyOptions {
            allow_expired: true,
            ..aud.clone()
        };
        assert!(signer
            .verify(
                tokens["alice_expired_access"].as_str().unwrap(),
                ACCESS_TOKEN_TYP,
                &expired_ok
            )
            .is_ok());
        let other_aud = SessionVerifyOptions {
            audience: Some("did:web:other.test".to_owned()),
            allow_expired: false,
        };
        assert_eq!(
            signer
                .verify(access, ACCESS_TOKEN_TYP, &other_aud)
                .unwrap_err(),
            SessionTokenError::Invalid
        );
        // an HS256 token is never accepted by a K256 deployment, whatever its key
        let k256 = JwtSigner::k256(K256_HEX).unwrap();
        assert_eq!(
            k256.verify(access, ACCESS_TOKEN_TYP, &aud).unwrap_err(),
            SessionTokenError::Invalid
        );
        // and a token signed with a different secret fails at the signature
        let other = JwtSigner::hmac(b"another secret");
        assert_eq!(
            other.verify(access, ACCESS_TOKEN_TYP, &aud).unwrap_err(),
            SessionTokenError::Invalid
        );
        // structural garbage
        for garbage in ["", "a.b", "a.b.c.d", "!!.@@.##"] {
            let error = signer.verify(garbage, ACCESS_TOKEN_TYP, &aud).unwrap_err();
            assert_eq!(error, SessionTokenError::Invalid);
        }
        let mut broken = access.to_owned();
        broken.replace_range(0..1, "z");
        assert_eq!(
            signer.verify(&broken, ACCESS_TOKEN_TYP, &aud).unwrap_err(),
            SessionTokenError::Invalid
        );
    }

    #[test]
    fn service_and_pop_tokens_are_not_session_tokens() {
        let signer = JwtSigner::hmac(SECRET);
        let now = now_secs();
        let with_lxm = signer
            .sign(
                ACCESS_TOKEN_TYP,
                &serde_json::json!({"scope": "com.atproto.access", "aud": "did:web:a", "sub": "did:plc:a", "lxm": "app.bsky.feed.getFeed", "exp": now + 60}),
            )
            .unwrap();
        let with_cnf = signer
            .sign(
                ACCESS_TOKEN_TYP,
                &serde_json::json!({"scope": "com.atproto.access", "aud": "did:web:a", "sub": "did:plc:a", "cnf": {"jkt": "x"}, "exp": now + 60}),
            )
            .unwrap();
        let not_yet = signer
            .sign(
                ACCESS_TOKEN_TYP,
                &serde_json::json!({"scope": "com.atproto.access", "aud": ["did:web:a"], "sub": "did:plc:a", "nbf": now + 600, "exp": now + 900}),
            )
            .unwrap();
        let list_aud = signer
            .sign(
                ACCESS_TOKEN_TYP,
                &serde_json::json!({"scope": "com.atproto.access", "aud": ["did:web:a"], "sub": "did:plc:a", "nbf": now - 10, "exp": now + 900}),
            )
            .unwrap();
        let options = SessionVerifyOptions {
            audience: Some("did:web:a".to_owned()),
            allow_expired: false,
        };
        assert_eq!(
            signer
                .verify(&with_lxm, ACCESS_TOKEN_TYP, &options)
                .unwrap_err(),
            SessionTokenError::Malformed
        );
        assert_eq!(
            signer
                .verify(&with_cnf, ACCESS_TOKEN_TYP, &options)
                .unwrap_err(),
            SessionTokenError::Malformed
        );
        assert_eq!(
            signer
                .verify(&not_yet, ACCESS_TOKEN_TYP, &options)
                .unwrap_err(),
            SessionTokenError::Invalid
        );
        let claims = signer
            .verify(&list_aud, ACCESS_TOKEN_TYP, &options)
            .unwrap();
        assert!(claims.has_audience("did:web:a"));
        assert_eq!(claims.audience(), None);
        assert!(!SessionClaims {
            aud: None,
            ..claims.clone()
        }
        .has_audience("did:web:a"));
    }

    #[test]
    fn k256_tokens_round_trip_and_accept_legacy_headers() {
        let signer = JwtSigner::k256(K256_HEX).unwrap();
        let now = now_secs();
        let (access, refresh) = create_tokens_with(
            &signer,
            CreateTokensOpts {
                did: "did:plc:a".to_owned(),
                service_did: "did:web:a".to_owned(),
                scope: None,
                jti: None,
                expires_in_secs: None,
                issued_at: None,
            },
        )
        .unwrap();
        let options = SessionVerifyOptions {
            audience: Some("did:web:a".to_owned()),
            allow_expired: false,
        };
        let claims = signer.verify(&access, ACCESS_TOKEN_TYP, &options).unwrap();
        assert_eq!(claims.scope, "com.atproto.access");
        assert!(claims.exp.unwrap() >= now + ACCESS_TOKEN_LIFETIME_SECS);
        let decoded = decode_refresh_token_with(&signer, refresh).unwrap();
        assert_eq!(decoded.sub, "did:plc:a");
        // an access token is not a refresh token
        assert!(decode_refresh_token_with(&signer, access.clone()).is_err());
        // rsky 1.1 tokens carried typ "JWT" or none; the scope still separates them
        let legacy = signer
            .sign(
                "JWT",
                &serde_json::json!({"scope": "com.atproto.access", "aud": "did:web:a", "sub": "did:plc:a", "exp": now + 60}),
            )
            .unwrap();
        assert!(signer.verify(&legacy, ACCESS_TOKEN_TYP, &options).is_ok());
        assert!(signer.verify(&legacy, REFRESH_TOKEN_TYP, &options).is_ok());
        // the type is legacy, so only the scope says it is not a refresh token
        let error = decode_refresh_token_with(&signer, legacy.clone()).unwrap_err();
        assert_eq!(error.to_string(), "not a refresh token");
        // a signature that is not even the right length
        let short = format!("{}.AAAA", &legacy[..legacy.rfind('.').unwrap()]);
        assert_eq!(
            signer
                .verify(&short, ACCESS_TOKEN_TYP, &options)
                .unwrap_err(),
            SessionTokenError::Invalid
        );
        let no_typ = {
            let header = URL_SAFE_NO_PAD.encode(b"{\"alg\":\"ES256K\"}");
            let payload = URL_SAFE_NO_PAD.encode(
                serde_json::to_vec(&serde_json::json!({"scope": "com.atproto.access", "aud": "did:web:a", "sub": "did:plc:a", "exp": now + 60})).unwrap(),
            );
            let input = format!("{header}.{payload}");
            format!(
                "{input}.{}",
                URL_SAFE_NO_PAD.encode(signer.signature(input.as_bytes()))
            )
        };
        assert!(signer.verify(&no_typ, ACCESS_TOKEN_TYP, &options).is_ok());
        // the HMAC signer never accepts a typ-less token
        let hmac = JwtSigner::hmac(SECRET);
        assert_eq!(
            hmac.verify(&no_typ, ACCESS_TOKEN_TYP, &options)
                .unwrap_err(),
            SessionTokenError::Invalid
        );
        // a tampered signature fails
        let mut tampered: Vec<char> = access.chars().collect();
        let index = tampered.len() - 10;
        tampered[index] = if tampered[index] == 'A' { 'B' } else { 'A' };
        let tampered: String = tampered.into_iter().collect();
        assert!(signer
            .verify(&tampered, ACCESS_TOKEN_TYP, &options)
            .is_err());
        assert!(JwtSigner::k256("not hex").is_err());
    }

    #[test]
    fn signer_configuration_prefers_the_shared_secret() {
        let secret = Some("secret".to_owned());
        let k256 = Some(K256_HEX.to_owned());
        assert!(matches!(
            JwtSigner::from_config(secret.clone(), k256.clone()).unwrap(),
            JwtSigner::Hmac { .. }
        ));
        assert!(matches!(
            JwtSigner::from_config(None, k256).unwrap(),
            JwtSigner::K256 { .. }
        ));
        assert!(JwtSigner::from_config(None, None).is_err());
    }

    #[test]
    fn scope_formatting_matches_the_reference() {
        let phone = AppPassDescript {
            name: "phone".to_owned(),
            privileged: false,
        };
        let chat = AppPassDescript {
            name: "chat".to_owned(),
            privileged: true,
        };
        assert_eq!(format_scope(None, false), AuthScope::Access);
        assert_eq!(format_scope(Some(&phone), false), AuthScope::AppPass);
        assert_eq!(
            format_scope(Some(&chat), false),
            AuthScope::AppPassPrivileged
        );
        assert_eq!(format_scope(Some(&chat), true), AuthScope::Takendown);
        assert_eq!(normalize_typ("application/AT+JWT"), "at+jwt");
    }
}
