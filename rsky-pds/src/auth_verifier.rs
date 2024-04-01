use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::helpers::auth::CustomClaimObj;
use crate::account_manager::AccountManager;
use anyhow::{bail, Result};
use jwt_simple::claims::Audiences;
use jwt_simple::prelude::*;
use rocket::http::Status;
use rocket::request::{FromRequest, Outcome, Request};
use secp256k1::{Keypair, Secp256k1, SecretKey};
use std::env;
use thiserror::Error;

const INFINITY: u64 = u64::MAX;

#[derive(PartialEq, Clone)]
pub enum AuthScope {
    Access,
    Refresh,
    AppPass,
    Deactivated,
}

impl AuthScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            AuthScope::Access => "com.atproto.access",
            AuthScope::Refresh => "com.atproto.refresh",
            AuthScope::AppPass => "com.atproto.appPass",
            AuthScope::Deactivated => "com.atproto.deactivated",
        }
    }

    pub fn from_str(scope: &str) -> Result<Self> {
        match scope {
            "com.atproto.access" => Ok(AuthScope::Access),
            "com.atproto.refresh" => Ok(AuthScope::Refresh),
            "com.atproto.appPass" => Ok(AuthScope::AppPass),
            "com.atproto.deactivated" => Ok(AuthScope::Deactivated),
            _ => bail!("Invalid AuthScope: `{scope:?}` is not a valid auth scope"),
        }
    }
}

pub enum RoleStatus {
    Valid,
    Invalid,
    Missing,
}

#[derive(Clone)]
pub struct Credentials {
    pub r#type: String,
    pub did: Option<String>,
    pub scope: Option<AuthScope>,
    pub audience: Option<String>,
    pub token_id: Option<String>,
    pub aud: Option<String>,
    pub iss: Option<String>,
}

#[derive(Clone)]
pub struct AccessOutput {
    pub credentials: Option<Credentials>,
    pub artifacts: Option<String>,
}

pub struct ValidatedBearer {
    pub did: String,
    pub scope: AuthScope,
    pub token: String,
    pub payload: JwtPayload,
    pub audience: Option<String>,
}

pub struct AuthVerifierDids {
    pub pds: String,
    pub entryway: Option<String>,
    pub mod_service: Option<String>,
}

pub struct AuthVerifierOpts {
    jwt_key: Keypair,
    admin_pass: String,
    dids: AuthVerifierDids,
}

#[derive(Clone)]
pub struct JwtPayload {
    pub scope: AuthScope,
    pub sub: Option<String>,
    pub aud: Option<Audiences>,
    pub exp: Option<Duration>,
    pub iat: Option<Duration>,
    pub jti: Option<String>,
}

#[derive(Error, Debug)]
pub enum AuthError {
    #[error("BadJwt: `{0}`")]
    BadJwt(String),
    #[error("AccountNotFound: `{0}`")]
    AccountNotFound(String),
    #[error("AccountTakedown: `{0}`")]
    AccountTakedown(String),
}

// verifier guards

pub struct Access {
    pub access: AccessOutput,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for Access {
    type Error = AuthError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match validate_access_token(req, vec![AuthScope::Access]).await {
            Ok(access) => Outcome::Success(Access { access }),
            Err(error) => {
                Outcome::Failure((Status::BadRequest, AuthError::BadJwt(error.to_string())))
            }
        }
    }
}

pub struct AccessNotAppPassword {
    pub access: AccessOutput,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for AccessNotAppPassword {
    type Error = AuthError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match validate_access_token(req, vec![AuthScope::Access, AuthScope::AppPass]).await {
            Ok(access) => Outcome::Success(AccessNotAppPassword { access }),
            Err(error) => {
                Outcome::Failure((Status::BadRequest, AuthError::BadJwt(error.to_string())))
            }
        }
    }
}

pub struct AccessDeactivated {
    pub access: AccessOutput,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for AccessDeactivated {
    type Error = AuthError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match validate_access_token(
            req,
            vec![
                AuthScope::Access,
                AuthScope::AppPass,
                AuthScope::Deactivated,
            ],
        )
        .await
        {
            Ok(access) => Outcome::Success(AccessDeactivated { access }),
            Err(error) => {
                Outcome::Failure((Status::BadRequest, AuthError::BadJwt(error.to_string())))
            }
        }
    }
}

#[derive(Clone)]
pub struct AccessCheckTakedown {
    pub access: AccessOutput,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for AccessCheckTakedown {
    type Error = AuthError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let result =
            match validate_access_token(req, vec![AuthScope::Access, AuthScope::AppPass]).await {
                Ok(access) => AccessCheckTakedown { access },
                Err(error) => {
                    return Outcome::Failure((
                        Status::BadRequest,
                        AuthError::BadJwt(error.to_string()),
                    ));
                }
            };
        let requester = result.clone().access.credentials.unwrap().did.unwrap();
        let found = match AccountManager::get_account(
            &requester,
            Some(AvailabilityFlags {
                include_deactivated: Some(true),
                include_taken_down: None,
            }),
        )
        .await
        {
            Ok(Some(found)) => found,
            _ => {
                return Outcome::Failure((
                    Status::Forbidden,
                    AuthError::AccountNotFound("Account not found".to_string()),
                ));
            }
        };
        if found.takedown_ref.is_some() {
            return Outcome::Failure((
                Status::Unauthorized,
                AuthError::AccountTakedown("Account has been taken down".to_string()),
            ));
        }
        Outcome::Success(result)
    }
}

pub struct RevokeRefreshToken {
    pub id: String,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for RevokeRefreshToken {
    type Error = AuthError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let mut options = VerificationOptions::default();
        options.max_validity = Some(Duration::from_secs(INFINITY));
        match validate_bearer_token(req, vec![AuthScope::Refresh], Some(options)).await {
            Ok(result) => match result.payload.jti {
                Some(jti) => Outcome::Success(RevokeRefreshToken { id: jti }),
                None => Outcome::Failure((
                    Status::BadRequest,
                    AuthError::BadJwt("Unexpected missing refresh token id".to_owned()),
                )),
            },
            Err(error) => {
                Outcome::Failure((Status::BadRequest, AuthError::BadJwt(error.to_string())))
            }
        }
    }
}

pub async fn validate_bearer_token<'r>(
    request: &'r Request<'_>,
    scopes: Vec<AuthScope>,
    verify_options: Option<VerificationOptions>,
) -> Result<ValidatedBearer> {
    let token = bearer_token_from_req(request)?;
    if let Some(token) = token {
        let secp = Secp256k1::new();
        let private_key = env::var("PDS_JWT_KEY_K256_PRIVATE_KEY_HEX").unwrap();
        let secret_key =
            SecretKey::from_slice(&hex::decode(private_key.as_bytes()).unwrap()).unwrap();
        let jwt_key = Keypair::from_secret_key(&secp, &secret_key);
        let payload = verify_jwt(token.clone(), jwt_key, verify_options).await?;
        let JwtPayload {
            sub, aud, scope, ..
        } = payload.clone();
        let sub = sub.unwrap();
        let aud = aud.unwrap();
        if !sub.starts_with("did:") {
            bail!("Malformed token")
        }
        if let Audiences::AsString(aud) = aud {
            if !aud.starts_with("did:") {
                bail!("Malformed token")
            }
            if scopes.len() > 0 && !scopes.contains(&scope) {
                bail!("Bad token scope")
                /*{
                    "error": "InvalidToken",
                    "message": "Bad token scope"
                }*/
            }
            Ok(ValidatedBearer {
                did: sub,
                scope,
                audience: Some(aud),
                token,
                payload,
            })
        } else {
            bail!("Malformed token")
        }
    } else {
        bail!("AuthMissing")
    }
}

pub async fn validate_access_token<'r>(
    request: &'r Request<'_>,
    scopes: Vec<AuthScope>,
) -> Result<AccessOutput> {
    let mut options = VerificationOptions::default();
    options.allowed_audiences = Some(HashSet::from_strings(&[
        env::var("PDS_SERVICE_DID").unwrap()
    ]));
    let ValidatedBearer {
        did,
        scope,
        token,
        audience,
        ..
    } = validate_bearer_token(request, scopes, Some(options)).await?;
    Ok(AccessOutput {
        credentials: Some(Credentials {
            r#type: "access".to_string(),
            did: Some(did),
            scope: Some(scope),
            audience,
            token_id: None,
            aud: None,
            iss: None,
        }),
        artifacts: Some(token),
    })
}

pub fn bearer_token_from_req(request: &Request) -> Result<Option<String>> {
    match request.headers().get_one("authorization") {
        Some(header) if !header.starts_with("Bearer ") => Ok(None),
        Some(header) => {
            let slice = &header["Bearer ".len()..];
            Ok(Some(slice.to_string()))
        }
        None => Ok(None),
    }
}

pub async fn verify_jwt(
    jwt: String,
    jwt_key: Keypair,
    verify_options: Option<VerificationOptions>,
) -> Result<JwtPayload> {
    let key = ES256kKeyPair::from_bytes(jwt_key.secret_bytes().as_slice())?;
    let public_key = key.public_key();
    let claims = public_key.verify_token::<CustomClaimObj>(&jwt, verify_options)?;

    Ok(JwtPayload {
        scope: AuthScope::from_str(&claims.custom.scope)?,
        sub: claims.subject,
        aud: claims.audiences,
        exp: claims.expires_at,
        iat: claims.issued_at,
        jti: claims.jwt_id,
    })
}
