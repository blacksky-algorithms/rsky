use secp256k1::Keypair;
use crate::auth_verifier::AuthScope;
use anyhow::Result;
use jwt_simple::prelude::*;

pub struct CreateTokensOpts {
    pub did: String,
    pub jwt_key: Keypair,
    pub service_did: String,
    pub scope: Option<AuthScope>,
    pub jti: Option<String>,
    pub expires_in: Option<Duration>
}

pub struct AuthToken {
    pub scope: AuthScope,
    pub sub: String,
    pub exp: Duration
}

pub struct RefreshToken {
    pub scope: AuthScope, // AuthScope::Refresh
    pub sub: String,
    pub exp: Duration,
    pub jti: String
}

pub fn create_tokens (
    opts: CreateTokensOpts
) -> Result<()> {
    let CreateTokensOpts {
        did,
        jwt_key,
        service_did,
        scope,
        jti,
        expires_in
    } = opts;
    todo!()
}

pub fn create_access_token (
    opts: CreateTokensOpts
) -> Result<String> {
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
    todo!()
}
