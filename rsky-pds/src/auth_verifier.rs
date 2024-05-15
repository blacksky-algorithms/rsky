use crate::account_manager::helpers::account::AvailabilityFlags;
use crate::account_manager::helpers::auth::CustomClaimObj;
use crate::account_manager::AccountManager;
use crate::common::get_verification_material;
use crate::xrpc_server::auth::{verify_jwt as verify_service_jwt_server, ServiceJwtPayload};
use crate::SharedDidResolver;
use anyhow::{bail, Result};
use jwt_simple::claims::Audiences;
use jwt_simple::prelude::*;
use rocket::http::Status;
use rocket::request::{FromRequest, Outcome, Request};
use rocket::State;
use rsky_identity::did::atproto_data::get_did_key_from_multibase;
use rsky_identity::types::DidDocument;
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

pub struct ServiceJwtOpts {
    pub aud: Option<String>,
    pub iss: Option<Vec<String>>,
}

pub struct VerifiedServiceJwt {
    pub aud: String,
    pub iss: String,
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
                Outcome::Error((Status::BadRequest, AuthError::BadJwt(error.to_string())))
            }
        }
    }
}

pub struct Refresh {
    pub access: AccessOutput,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for Refresh {
    type Error = AuthError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let mut options = VerificationOptions::default();
        options.allowed_audiences = Some(HashSet::from_strings(&[
            env::var("PDS_SERVICE_DID").unwrap()
        ]));
        let ValidatedBearer {
            did,
            scope,
            token,
            payload,
            audience,
        } = match validate_bearer_token(req, vec![AuthScope::Refresh], Some(options)).await {
            Ok(result) => {
                let payload = result.payload.clone();
                match payload.jti {
                    Some(_) => result,
                    None => {
                        return Outcome::Error((
                            Status::BadRequest,
                            AuthError::BadJwt("Unexpected missing refresh token id".to_owned()),
                        ));
                    }
                }
            }
            Err(error) => {
                return Outcome::Error((Status::BadRequest, AuthError::BadJwt(error.to_string())));
            }
        };
        Outcome::Success(Refresh {
            access: AccessOutput {
                credentials: Some(Credentials {
                    r#type: "refresh".to_string(),
                    did: Some(did),
                    scope: Some(scope),
                    audience,
                    token_id: payload.jti,
                    aud: None,
                    iss: None,
                }),
                artifacts: Some(token),
            },
        })
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
                Outcome::Error((Status::BadRequest, AuthError::BadJwt(error.to_string())))
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
                Outcome::Error((Status::BadRequest, AuthError::BadJwt(error.to_string())))
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
        let result = match validate_access_token(req, vec![AuthScope::Access, AuthScope::AppPass])
            .await
        {
            Ok(access) => AccessCheckTakedown { access },
            Err(error) => {
                return Outcome::Error((Status::BadRequest, AuthError::BadJwt(error.to_string())));
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
                return Outcome::Error((
                    Status::Forbidden,
                    AuthError::AccountNotFound("Account not found".to_string()),
                ));
            }
        };
        if found.takedown_ref.is_some() {
            return Outcome::Error((
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
                None => Outcome::Error((
                    Status::BadRequest,
                    AuthError::BadJwt("Unexpected missing refresh token id".to_owned()),
                )),
            },
            Err(error) => {
                Outcome::Error((Status::BadRequest, AuthError::BadJwt(error.to_string())))
            }
        }
    }
}

pub struct UserDidAuth {
    pub access: AccessOutput,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for UserDidAuth {
    type Error = AuthError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let id_resolver = req.guard::<&State<SharedDidResolver>>().await.unwrap();
        match verify_service_jwt(
            req,
            id_resolver,
            ServiceJwtOpts {
                aud: Some(env::var("PDS_SERVICE_DID").unwrap()),
                iss: None,
            },
        )
        .await
        {
            Ok(payload) => Outcome::Success(UserDidAuth {
                access: AccessOutput {
                    credentials: Some(Credentials {
                        r#type: "user_did".to_string(),
                        did: None,
                        scope: None,
                        audience: None,
                        token_id: None,
                        aud: Some(payload.aud),
                        iss: Some(payload.iss),
                    }),
                    artifacts: None,
                },
            }),
            Err(error) => {
                Outcome::Error((Status::BadRequest, AuthError::BadJwt(error.to_string())))
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

pub async fn verify_service_jwt<'r>(
    request: &'r Request<'_>,
    id_resolver: &State<SharedDidResolver>,
    opts: ServiceJwtOpts,
) -> Result<VerifiedServiceJwt> {
    let get_signing_key = |iss: String, force_refresh: bool| -> Result<String> {
        match &opts.iss {
            Some(opts_iss) if opts_iss.contains(&iss) => bail!("UntrustedIss: Untrusted issuer"),
            _ => (),
        }
        let parts = iss.split("#").collect::<Vec<&str>>();
        if let (Some(did), Some(service_id)) = (parts.get(0), parts.get(1)) {
            let (did, service_id) = (did.to_string(), *service_id);
            let key_id = if service_id == "atproto_labeler" {
                "atproto_label"
            } else {
                "atproto"
            };
            let mut lock = futures::executor::block_on(id_resolver.id_resolver.write());
            let did_doc: Result<DidDocument> =
                futures::executor::block_on(lock.ensure_resolve(&did, Some(force_refresh)));
            let did_doc: DidDocument = match did_doc {
                Err(err) => bail!("could not resolve iss did: `{err}`"),
                Ok(res) => res,
            };
            match get_verification_material(&did_doc, &key_id.to_string()) {
                None => bail!("missing or bad key in did doc"),
                Some(parsed_key) => match get_did_key_from_multibase(parsed_key)? {
                    None => bail!("missing or bad key in did doc"),
                    Some(did_key) => Ok(did_key),
                },
            }
        } else {
            bail!("could not resolve iss did")
        }
    };

    match bearer_token_from_req(request)? {
        None => bail!("MissingJwt: missing jwt"),
        Some(jwt_str) => {
            let payload: ServiceJwtPayload =
                verify_service_jwt_server(jwt_str, opts.aud, get_signing_key).await?;
            Ok(VerifiedServiceJwt {
                iss: payload.iss,
                aud: payload.aud,
            })
        }
    }
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
