use crate::auth_verifier::{AuthScope, PDS_JWT_KEYPAIR};
use crate::context::PDS_REPO_SIGNING_KEYPAIR;
use crate::db::sqlite::Db;
use crate::models;
use anyhow::Result;
use chrono::DateTime;
use jwt_simple::prelude::*;
use rsky_common::time::MINUTE;
use rsky_common::{get_random_str, json_to_b64url, RFC3339_VARIANT};
use rusqlite::{params, OptionalExtension};
use secp256k1::Message;
use sha2::{Digest, Sha256};
use std::time::SystemTime;
use thiserror::Error;

pub struct CreateTokensOpts {
    pub did: String,
    pub service_did: String,
    pub scope: Option<AuthScope>,
    pub jti: Option<String>,
    pub expires_in: Option<Duration>,
}

pub struct RefreshGracePeriodOpts {
    pub id: String,
    pub expires_at: String,
    pub next_id: String,
}

pub struct AuthToken {
    pub scope: AuthScope,
    pub sub: String,
    pub exp: Duration,
}

pub struct RefreshToken {
    pub scope: AuthScope, // AuthScope::Refresh
    pub sub: String,
    pub exp: Duration,
    pub jti: String,
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

pub fn create_tokens(opts: CreateTokensOpts) -> Result<(String, String)> {
    let CreateTokensOpts {
        did,
        service_did,
        scope,
        jti,
        expires_in,
    } = opts;
    let access_jwt = create_access_token(CreateTokensOpts {
        did: did.clone(),
        service_did: service_did.clone(),
        scope,
        expires_in,
        jti: None,
    })?;
    let refresh_jwt = create_refresh_token(CreateTokensOpts {
        did,
        service_did,
        jti,
        expires_in,
        scope: None,
    })?;
    Ok((access_jwt, refresh_jwt))
}

pub fn create_access_token(opts: CreateTokensOpts) -> Result<String> {
    let CreateTokensOpts {
        did,
        service_did,
        scope,
        expires_in,
        ..
    } = opts;
    let scope = scope.unwrap_or(AuthScope::Access);
    let expires_in = expires_in.unwrap_or_else(|| Duration::from_hours(2));
    let claims = Claims::with_custom_claims(
        CustomClaimObj {
            scope: scope.as_str().to_owned(),
        },
        expires_in,
    )
    .with_audience(service_did)
    .with_subject(did);

    PDS_JWT_KEYPAIR.sign(claims)
}

pub fn create_refresh_token(opts: CreateTokensOpts) -> Result<String> {
    let CreateTokensOpts {
        did,
        service_did,
        jti,
        expires_in,
        ..
    } = opts;
    let jti = jti.unwrap_or_else(get_random_str);
    let expires_in = expires_in.unwrap_or_else(|| Duration::from_days(90));
    let claims = Claims::with_custom_claims(
        CustomClaimObj {
            scope: AuthScope::Refresh.as_str().to_owned(),
        },
        expires_in,
    )
    .with_audience(service_did)
    .with_subject(did)
    .with_jwt_id(jti);

    PDS_JWT_KEYPAIR.sign(claims)
}

pub async fn create_service_jwt(params: ServiceJwtParams) -> Result<String> {
    let ServiceJwtParams { iss, aud, .. } = params;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("timestamp in micros since UNIX epoch")
        .as_micros() as usize;
    let exp = params
        .exp
        .unwrap_or(((now + MINUTE as usize) / 1000) as u64);
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
    let mut sig = PDS_REPO_SIGNING_KEYPAIR.secret_key().sign_ecdsa(message);
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
    let claims = PDS_JWT_KEYPAIR
        .public_key()
        .verify_token::<CustomClaimObj>(&jwt, None)?;
    assert_eq!(
        claims.custom.scope,
        AuthScope::Refresh.as_str().to_owned(),
        "not a refresh token"
    );
    Ok(RefreshToken {
        scope: AuthScope::from_str(&claims.custom.scope)?,
        sub: claims.subject.unwrap(),
        exp: claims.expires_at.unwrap(),
        jti: claims.jwt_id.unwrap(),
    })
}

pub async fn store_refresh_token(
    payload: RefreshToken,
    app_password_name: Option<String>,
    db: &Db,
) -> Result<()> {
    let exp = DateTime::from_timestamp_millis(payload.exp.as_millis() as i64)
        .ok_or_else(|| anyhow::anyhow!("token expiry out of range"))?;
    let expires_at = format!("{}", exp.format(RFC3339_VARIANT));

    db.run(move |conn| {
        // ON CONFLICT DO NOTHING e.g. when re-granting during a refresh grace period
        conn.execute(
            "INSERT INTO refresh_token (id, did, \"appPasswordName\", \"expiresAt\") \
             VALUES (?1, ?2, ?3, ?4) \
             ON CONFLICT (id) DO NOTHING",
            params![payload.jti, payload.sub, app_password_name, expires_at],
        )?;
        Ok(())
    })
    .await
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

pub async fn get_refresh_token(id: &str, db: &Db) -> Result<Option<models::RefreshToken>> {
    let id = id.to_owned();
    db.run(move |conn| {
        Ok(conn
            .query_row(
                "SELECT id, did, \"expiresAt\", \"nextId\", \"appPasswordName\" \
                 FROM refresh_token WHERE id = ?1",
                params![id],
                |row| {
                    Ok(models::RefreshToken {
                        id: row.get(0)?,
                        did: row.get(1)?,
                        expires_at: row.get(2)?,
                        next_id: row.get(3)?,
                        app_password_name: row.get(4)?,
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
    let RefreshGracePeriodOpts {
        id,
        expires_at,
        next_id,
    } = opts;
    db.run(move |conn| {
        let updated = conn.execute(
            "UPDATE refresh_token SET \"expiresAt\" = ?1, \"nextId\" = ?2 \
             WHERE id = ?3 AND (\"nextId\" IS NULL OR \"nextId\" = ?2)",
            params![expires_at, next_id, id],
        )?;
        if updated < 1 {
            return Err(anyhow::Error::new(AuthHelperError::ConcurrentRefresh));
        }
        Ok(())
    })
    .await
}

pub fn get_refresh_token_id() -> String {
    get_random_str()
}
