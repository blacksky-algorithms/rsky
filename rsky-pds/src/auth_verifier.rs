use crate::account_manager::helpers::account::{ActorAccount, AvailabilityFlags};
use crate::account_manager::helpers::auth::CustomClaimObj;
use crate::account_manager::AccountManager;
use crate::common::env::env_str;
use crate::common::get_verification_material;
use crate::xrpc_server::auth::{verify_jwt as verify_service_jwt_server, ServiceJwtPayload};
use crate::SharedIdResolver;
use anyhow::{bail, Result};
use base64::{engine::general_purpose::STANDARD as base64pad, Engine as _};
use jwt_simple::claims::Audiences;
use jwt_simple::prelude::*;
use rocket::http::Status;
use rocket::request::{FromRequest, Outcome, Request};
use rocket::State;
use rsky_identity::did::atproto_data::get_did_key_from_multibase;
use rsky_identity::types::DidDocument;
use secp256k1::{Keypair, Secp256k1, SecretKey};
use std::env;
use std::str;
use thiserror::Error;

const INFINITY: u64 = u64::MAX;

#[derive(PartialEq, Clone, Debug)]
pub enum AuthScope {
    Access,
    Refresh,
    AppPass,
    AppPassPrivileged,
    SignupQueued,
}

impl AuthScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            AuthScope::Access => "com.atproto.access",
            AuthScope::Refresh => "com.atproto.refresh",
            AuthScope::AppPass => "com.atproto.appPass",
            AuthScope::AppPassPrivileged => "com.atproto.appPassPrivileged",
            AuthScope::SignupQueued => "com.atproto.signupQueued",
        }
    }

    pub fn from_str(scope: &str) -> Result<Self> {
        match scope {
            "com.atproto.access" => Ok(AuthScope::Access),
            "com.atproto.refresh" => Ok(AuthScope::Refresh),
            "com.atproto.appPass" => Ok(AuthScope::AppPass),
            "com.atproto.appPassPrivileged" => Ok(AuthScope::AppPassPrivileged),
            "com.atproto.signupQueued" => Ok(AuthScope::SignupQueued),
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
    pub is_privileged: Option<bool>,
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

pub struct ValidateAccessTokenOpts {
    pub check_takedown: Option<bool>,
    pub check_deactivated: Option<bool>,
}

pub struct VerifiedServiceJwt {
    pub aud: String,
    pub iss: String,
}

pub struct BasicAuth {
    pub username: String,
    pub password: String,
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
    #[error("BadJwtAudience: `{0}`")]
    BadJwtAudience(String),
    #[error("UntrustedIss: `{0}`")]
    UntrustedIss(String),
    #[error("AuthRequired: `{0}`")]
    AuthRequired(String),
    #[error("AccountNotFound: `{0}`")]
    AccountNotFound(String),
    #[error("AccountTakedown: `{0}`")]
    AccountTakedown(String),
    #[error("AccountDeactivated: `{0}`")]
    AccountDeactivated(String),
}

// verifier guards

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
                    is_privileged: None,
                }),
                artifacts: Some(token),
            },
        })
    }
}

pub async fn access_check<'r>(
    req: &'r Request<'_>,
    scopes: Vec<AuthScope>,
    opts: Option<ValidateAccessTokenOpts>,
) -> Outcome<AccessOutput, AuthError> {
    match validate_access_token(req, scopes, opts).await {
        Ok(access) => Outcome::Success(access),
        Err(error) => match error.downcast_ref() {
            Some(AuthError::AccountDeactivated(error)) => Outcome::Error((
                Status::BadRequest,
                AuthError::AccountDeactivated(error.to_string()),
            )),
            Some(AuthError::AccountNotFound(error)) => Outcome::Error((
                Status::BadRequest,
                AuthError::AccountNotFound(error.to_string()),
            )),
            Some(AuthError::AccountTakedown(error)) => Outcome::Error((
                Status::BadRequest,
                AuthError::AccountTakedown(error.to_string()),
            )),
            _ => Outcome::Error((Status::BadRequest, AuthError::BadJwt(error.to_string()))),
        },
    }
}

pub struct AccessFull {
    pub access: AccessOutput,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for AccessFull {
    type Error = AuthError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match access_check(req, vec![AuthScope::Access], None).await {
            Outcome::Success(access) => Outcome::Success(AccessFull { access }),
            Outcome::Error(error) => Outcome::Error(error),
            Outcome::Forward(_) => panic!("Outcome::Forward returned"),
        }
    }
}

pub struct AccessPrivileged {
    pub access: AccessOutput,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for AccessPrivileged {
    type Error = AuthError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match access_check(
            req,
            vec![AuthScope::Access, AuthScope::AppPassPrivileged],
            None,
        )
        .await
        {
            Outcome::Success(access) => Outcome::Success(Self { access }),
            Outcome::Error(error) => Outcome::Error(error),
            Outcome::Forward(_) => panic!("Outcome::Forward returned"),
        }
    }
}

pub struct AccessStandard {
    pub access: AccessOutput,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for AccessStandard {
    type Error = AuthError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match access_check(
            req,
            vec![
                AuthScope::Access,
                AuthScope::AppPass,
                AuthScope::AppPassPrivileged,
            ],
            None,
        )
        .await
        {
            Outcome::Success(access) => Outcome::Success(AccessStandard { access }),
            Outcome::Error(error) => Outcome::Error(error),
            Outcome::Forward(_) => panic!("Outcome::Forward returned"),
        }
    }
}

#[derive(Clone)]
pub struct AccessStandardIncludeChecks {
    pub access: AccessOutput,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for AccessStandardIncludeChecks {
    type Error = AuthError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match access_check(
            req,
            vec![
                AuthScope::Access,
                AuthScope::AppPass,
                AuthScope::AppPassPrivileged,
            ],
            Some(ValidateAccessTokenOpts {
                check_deactivated: Some(true),
                check_takedown: Some(true),
            }),
        )
        .await
        {
            Outcome::Success(access) => Outcome::Success(AccessStandardIncludeChecks { access }),
            Outcome::Error(error) => Outcome::Error(error),
            Outcome::Forward(_) => panic!("Outcome::Forward returned"),
        }
    }
}

#[derive(Clone)]
pub struct AccessStandardCheckTakedown {
    pub access: AccessOutput,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for AccessStandardCheckTakedown {
    type Error = AuthError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match access_check(
            req,
            vec![
                AuthScope::Access,
                AuthScope::AppPass,
                AuthScope::AppPassPrivileged,
            ],
            Some(ValidateAccessTokenOpts {
                check_deactivated: None,
                check_takedown: Some(true),
            }),
        )
        .await
        {
            Outcome::Success(access) => Outcome::Success(AccessStandardCheckTakedown { access }),
            Outcome::Error(error) => Outcome::Error(error),
            Outcome::Forward(_) => panic!("Outcome::Forward returned"),
        }
    }
}

pub struct AccessStandardSignupQueued {
    pub access: AccessOutput,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for AccessStandardSignupQueued {
    type Error = AuthError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match access_check(
            req,
            vec![
                AuthScope::Access,
                AuthScope::AppPass,
                AuthScope::AppPassPrivileged,
                AuthScope::SignupQueued,
            ],
            None,
        )
        .await
        {
            Outcome::Success(access) => Outcome::Success(AccessStandardSignupQueued { access }),
            Outcome::Error(error) => Outcome::Error(error),
            Outcome::Forward(_) => panic!("Outcome::Forward returned"),
        }
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
        let id_resolver = req.guard::<&State<SharedIdResolver>>().await.unwrap();
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
                        is_privileged: None,
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

pub struct UserDidAuthOptional {
    pub access: Option<AccessOutput>,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for UserDidAuthOptional {
    type Error = AuthError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        if is_bearer_token(req) {
            match UserDidAuth::from_request(req).await {
                Outcome::Success(output) => Outcome::Success(UserDidAuthOptional {
                    access: Some(output.access),
                }),
                Outcome::Error(err) => Outcome::Error(err),
                _ => panic!("Unexpected outcome during UserDidAuthOptional"),
            }
        } else {
            Outcome::Success(UserDidAuthOptional { access: None })
        }
    }
}

pub struct ModService {
    pub access: AccessOutput,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for ModService {
    type Error = AuthError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        if let Some(mod_service_did) = env_str("PDS_MOD_SERVICE_DID") {
            let id_resolver = req.guard::<&State<SharedIdResolver>>().await.unwrap();
            match verify_service_jwt(
                req,
                id_resolver,
                ServiceJwtOpts {
                    aud: None,
                    iss: Some(vec![
                        mod_service_did.clone(),
                        format!("{mod_service_did}#atproto_labeler"),
                    ]),
                },
            )
            .await
            {
                Ok(payload)
                    if Some(payload.aud.clone()) != env_str("PDS_SERVICE_DID")
                        && (env_str("PDS_ENTRYWAY_DID").is_none()
                            || Some(payload.aud.clone()) != env_str("PDS_ENTRYWAY_DID")) =>
                {
                    Outcome::Error((
                        Status::BadRequest,
                        AuthError::BadJwtAudience(
                            "jwt audience does not match service did".to_string(),
                        ),
                    ))
                }
                Ok(payload) => Outcome::Success(ModService {
                    access: AccessOutput {
                        credentials: Some(Credentials {
                            r#type: "mod_service".to_string(),
                            did: None,
                            scope: None,
                            audience: None,
                            token_id: None,
                            aud: Some(payload.aud),
                            iss: Some(payload.iss),
                            is_privileged: None,
                        }),
                        artifacts: None,
                    },
                }),
                Err(error) => {
                    Outcome::Error((Status::BadRequest, AuthError::BadJwt(error.to_string())))
                }
            }
        } else {
            Outcome::Error((
                Status::BadRequest,
                AuthError::UntrustedIss("Untrusted issuer".to_string()),
            ))
        }
    }
}

pub struct Moderator {
    pub access: AccessOutput,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for Moderator {
    type Error = AuthError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        if is_bearer_token(req) {
            match ModService::from_request(req).await {
                Outcome::Success(output) => Outcome::Success(Moderator {
                    access: output.access,
                }),
                Outcome::Error(err) => Outcome::Error(err),
                _ => panic!("Unexpected outcome during Moderator"),
            }
        } else {
            match AdminToken::from_request(req).await {
                Outcome::Success(output) => Outcome::Success(Moderator {
                    access: output.access,
                }),
                Outcome::Error(err) => Outcome::Error(err),
                _ => panic!("Unexpected outcome during Moderator"),
            }
        }
    }
}

pub struct AdminToken {
    pub access: AccessOutput,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for AdminToken {
    type Error = AuthError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let auth_header: &str = req.headers().get_one("Authorization").unwrap_or("");
        match parse_basic_auth(auth_header) {
            None => Outcome::Error((
                Status::BadRequest,
                AuthError::AuthRequired("AuthMissing".to_string()),
            )),
            Some(parsed) => {
                let BasicAuth { username, password } = parsed;

                if username != "admin" || password != env::var("PDS_ADMIN_PASS").unwrap() {
                    Outcome::Error((
                        Status::BadRequest,
                        AuthError::AuthRequired("BadAuth".to_string()),
                    ))
                } else {
                    Outcome::Success(AdminToken {
                        access: AccessOutput {
                            credentials: Some(Credentials {
                                r#type: "admin_token".to_string(),
                                did: None,
                                scope: None,
                                audience: None,
                                token_id: None,
                                aud: None,
                                iss: None,
                                is_privileged: None,
                            }),
                            artifacts: None,
                        },
                    })
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct OptionalAccessOrAdminToken {
    pub access: Option<AccessOutput>,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for OptionalAccessOrAdminToken {
    type Error = AuthError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        if is_bearer_token(req) {
            match AccessFull::from_request(req).await {
                Outcome::Success(output) => Outcome::Success(OptionalAccessOrAdminToken {
                    access: Some(output.access),
                }),
                Outcome::Error(err) => Outcome::Error(err),
                _ => panic!("Unexpected outcome during OptionalAccessOrAdminToken"),
            }
        } else if is_basic_token(req) {
            match AdminToken::from_request(req).await {
                Outcome::Success(output) => Outcome::Success(OptionalAccessOrAdminToken {
                    access: Some(output.access),
                }),
                Outcome::Error(err) => Outcome::Error(err),
                _ => panic!("Unexpected outcome during OptionalAccessOrAdminToken"),
            }
        } else {
            Outcome::Success(OptionalAccessOrAdminToken { access: None })
        }
    }
}

pub async fn validate_bearer_access_token<'r>(
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
    let is_privileged = vec![AuthScope::Access, AuthScope::AppPassPrivileged].contains(&scope);
    Ok(AccessOutput {
        credentials: Some(Credentials {
            r#type: "access".to_string(),
            did: Some(did),
            scope: Some(scope),
            audience,
            token_id: None,
            aud: None,
            iss: None,
            is_privileged: Some(is_privileged),
        }),
        artifacts: Some(token),
    })
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

// @TODO: Implement DPop/OAuth
pub async fn validate_access_token<'r>(
    request: &'r Request<'_>,
    scopes: Vec<AuthScope>,
    opts: Option<ValidateAccessTokenOpts>,
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
    let ValidateAccessTokenOpts {
        check_takedown,
        check_deactivated,
    } = opts.unwrap_or_else(|| ValidateAccessTokenOpts {
        check_takedown: Some(false),
        check_deactivated: Some(false),
    });
    let check_takedown = check_takedown.unwrap_or(false);
    let check_deactivated = check_deactivated.unwrap_or(false);
    if check_takedown || check_deactivated {
        let found: ActorAccount = match AccountManager::get_account(
            &did,
            Some(AvailabilityFlags {
                include_deactivated: None,
                include_taken_down: Some(true),
            }),
        )
        .await
        {
            Ok(Some(found)) => found,
            _ => {
                return Err(anyhow::Error::new(AuthError::AccountNotFound(
                    "Account not found".to_string(),
                )))
            }
        };
        if check_takedown && found.takedown_ref.is_some() {
            return Err(anyhow::Error::new(AuthError::AccountTakedown(
                "Account has been taken down".to_string(),
            )));
        }
        if check_deactivated && found.deactivated_at.is_some() {
            return Err(anyhow::Error::new(AuthError::AccountDeactivated(
                "Account is deactivated".to_string(),
            )));
        }
    }
    Ok(AccessOutput {
        credentials: Some(Credentials {
            r#type: "access".to_string(),
            did: Some(did),
            scope: Some(scope),
            audience,
            token_id: None,
            aud: None,
            iss: None,
            is_privileged: None,
        }),
        artifacts: Some(token),
    })
}

pub async fn verify_service_jwt<'r>(
    request: &'r Request<'_>,
    id_resolver: &State<SharedIdResolver>,
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
                futures::executor::block_on(lock.did.ensure_resolve(&did, Some(force_refresh)));
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

pub fn is_user_or_admin(auth: AccessOutput, did: &String) -> bool {
    match auth.credentials {
        Some(credentials) if credentials.did == Some("admin_token".to_string()) => true,
        Some(credentials) => credentials.did == Some(did.to_string()),
        None => false,
    }
}

// HELPERS
// ---------

const BEARER: &str = "Bearer ";
const BASIC: &str = "Basic ";

pub fn is_bearer_token(request: &Request) -> bool {
    match request.headers().get_one("Authorization") {
        None => false,
        Some(auth_header) => auth_header.starts_with(BEARER),
    }
}

pub fn is_basic_token(request: &Request) -> bool {
    match request.headers().get_one("Authorization") {
        None => false,
        Some(auth_header) => auth_header.starts_with(BASIC),
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

pub fn parse_basic_auth(token: &str) -> Option<BasicAuth> {
    if !token.starts_with(BASIC) {
        return None;
    }

    let b64 = &token[BASIC.len()..];
    let decoded: Vec<u8> = match base64pad.decode(b64) {
        Err(_) => return None,
        Ok(decoded) => decoded,
    };
    let parsed_str: &str = match str::from_utf8(&decoded) {
        Err(_) => return None,
        Ok(res) => res,
    };
    let parsed_parts = parsed_str.split(":").collect::<Vec<&str>>();

    match (parsed_parts.get(0), parsed_parts.get(1)) {
        (Some(username), Some(password)) => Some(BasicAuth {
            username: username.to_string(),
            password: password.to_string(),
        }),
        _ => None,
    }
}
