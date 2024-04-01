use crate::auth_verifier::AuthScope;
use crate::common::time::MINUTE;
use crate::common::{get_random_str, json_to_b64url, RFC3339_VARIANT};
use crate::db::establish_connection;
use crate::models;
use anyhow::Result;
use chrono::{DateTime, NaiveDateTime, Utc};
use diesel::*;
use jwt_simple::prelude::*;
use secp256k1::{Keypair, Message, SecretKey};
use sha2::{Digest, Sha256};
use std::time::SystemTime;

pub struct CreateTokensOpts {
    pub did: String,
    pub jwt_key: Keypair,
    pub service_did: String,
    pub scope: Option<AuthScope>,
    pub jti: Option<String>,
    pub expires_in: Option<Duration>,
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
    pub keypair: SecretKey,
}

#[derive(Serialize, Deserialize)]
pub struct CustomClaimObj {
    pub scope: String,
}

pub fn create_tokens(opts: CreateTokensOpts) -> Result<(String, String)> {
    let CreateTokensOpts {
        did,
        jwt_key,
        service_did,
        scope,
        jti,
        expires_in,
    } = opts;
    let access_jwt = create_access_token(CreateTokensOpts {
        did: did.clone(),
        jwt_key,
        service_did: service_did.clone(),
        scope,
        expires_in,
        jti: None,
    })?;
    let refresh_jwt = create_refresh_token(CreateTokensOpts {
        did,
        jwt_key,
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
        jwt_key,
        service_did,
        scope,
        expires_in,
        ..
    } = opts;
    let scope = scope.unwrap_or_else(|| AuthScope::Access);
    let expires_in = expires_in.unwrap_or_else(|| Duration::from_hours(2));
    let claims = Claims::with_custom_claims(
        CustomClaimObj {
            scope: scope.as_str().to_owned(),
        },
        expires_in,
    )
    .with_audience(service_did)
    .with_subject(did);
    // alg ES256K
    let key = ES256kKeyPair::from_bytes(jwt_key.secret_bytes().as_slice())?;
    let token = key.sign(claims)?;
    Ok(token)
}

pub fn create_refresh_token(opts: CreateTokensOpts) -> Result<String> {
    let CreateTokensOpts {
        did,
        jwt_key,
        service_did,
        jti,
        expires_in,
        ..
    } = opts;
    let jti = jti.unwrap_or_else(|| get_random_str());
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
    // alg ES256K
    let key = ES256kKeyPair::from_bytes(jwt_key.secret_bytes().as_slice())?;
    let token = key.sign(claims)?;
    Ok(token)
}

pub async fn create_service_jwt(params: ServiceJwtParams) -> Result<String> {
    let ServiceJwtParams {
        iss, aud, keypair, ..
    } = params;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("timestamp in micros since UNIX epoch")
        .as_micros() as usize;
    let exp = params
        .exp
        .unwrap_or(((now + MINUTE as usize) / 1000) as u64);
    let header = ServiceJwtHeader {
        typ: "JWT".to_string(),
        alg: "ES256K".to_string(),
    };
    let payload = ServiceJwtPayload {
        iss,
        aud,
        exp: Some(exp),
    };
    let to_sign_str = format!(
        "{0}.{1}",
        json_to_b64url(&header)?,
        json_to_b64url(&payload)?
    );
    let hash = Sha256::digest(to_sign_str.clone());
    let message = Message::from_digest_slice(hash.as_ref())?;
    let mut sig = keypair.sign_ecdsa(message);
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
pub fn decode_refresh_token(jwt: String, jwt_key: Keypair) -> Result<RefreshToken> {
    let key = ES256kKeyPair::from_bytes(jwt_key.secret_bytes().as_slice())?;
    let public_key = key.public_key();
    let claims = public_key.verify_token::<CustomClaimObj>(&jwt, None)?;
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

#[allow(deprecated)]
pub fn store_refresh_token(payload: RefreshToken, app_password_name: Option<String>) -> Result<()> {
    use crate::schema::pds::refresh_token::dsl as RefreshTokenSchema;
    let conn = &mut establish_connection()?;

    let nanoseconds = 230 * 1000000;
    let exp = DateTime::<Utc>::from_utc(
        NaiveDateTime::from_timestamp((payload.exp.as_millis() / 1000) as i64, nanoseconds),
        Utc,
    );

    insert_into(RefreshTokenSchema::refresh_token)
        .values((
            RefreshTokenSchema::id.eq(payload.jti),
            RefreshTokenSchema::did.eq(payload.sub),
            RefreshTokenSchema::appPasswordName.eq(app_password_name),
            RefreshTokenSchema::expiresAt.eq(format!("{}", exp.format(RFC3339_VARIANT))),
        ))
        .on_conflict_do_nothing() // E.g. when re-granting during a refresh grace period
        .execute(conn)?;
    Ok(())
}

pub async fn revoke_refresh_token(id: String) -> Result<bool> {
    use crate::schema::pds::refresh_token::dsl as RefreshTokenSchema;
    let conn = &mut establish_connection()?;

    let deleted_rows = delete(RefreshTokenSchema::refresh_token)
        .filter(RefreshTokenSchema::id.eq(id))
        .get_results::<models::RefreshToken>(conn)?;

    Ok(deleted_rows.len() > 0)
}
