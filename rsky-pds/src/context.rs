use crate::account_manager::helpers::auth::ServiceJwtParams;
use crate::xrpc_server::auth::create_service_auth_headers;
use anyhow::Result;
use reqwest::header::HeaderMap;
use secp256k1::SecretKey;
use std::env;

pub async fn service_auth_headers(did: &String, aud: &String, lxm: &String) -> Result<HeaderMap> {
    let private_key = env::var("PDS_REPO_SIGNING_KEY_K256_PRIVATE_KEY_HEX").unwrap();
    let keypair = SecretKey::from_slice(&hex::decode(private_key.as_bytes()).unwrap()).unwrap();
    create_service_auth_headers(ServiceJwtParams {
        iss: did.clone(),
        aud: aud.clone(),
        exp: None,
        lxm: Some(lxm.clone()),
        jti: None,
        keypair,
    })
    .await
}
