extern crate unsigned_varint;
use crate::common::env::{env_int, env_str};
use crate::common::sign::atproto_sign;
use crate::models::*;
use crate::{plc, SharedIdResolver, APP_USER_AGENT};
use anyhow::{bail, Result};
use data_encoding::BASE32;
use diesel::prelude::*;
use diesel::PgConnection;
use indexmap::IndexMap;
use multibase::Base::Base58Btc;
use rand::{distributions::Alphanumeric, Rng};
use reqwest;
use rocket::form::validate::Contains;
use rocket::State;
use rsky_identity::types::DidDocument;
use rsky_lexicon::com::atproto::server::CreateAccountInput;
use secp256k1::{Keypair, PublicKey, Secp256k1, SecretKey};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::env;
use unsigned_varint::encode::u16 as encode_varint;

const DID_KEY_PREFIX: &str = "did:key:";

/// Important to user `preserve_order` with serde_json so these bytes are ordered
/// correctly when encoding.
#[derive(Debug, Deserialize, Serialize)]
pub struct AtprotoPdsService {
    #[serde(rename(deserialize = "type", serialize = "type"))]
    pub r#type: String,
    pub endpoint: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PlcGenesisServices {
    pub atproto_pds: AtprotoPdsService,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PlcGenesisVerificationMethods {
    pub atproto: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PlcGenesisOperation {
    #[serde(rename(deserialize = "type", serialize = "type"))]
    pub r#type: String,
    #[serde(rename(deserialize = "rotationKeys", serialize = "rotationKeys"))]
    pub rotation_keys: Vec<String>,
    #[serde(rename(deserialize = "verificationMethods", serialize = "verificationMethods"))]
    pub verification_methods: PlcGenesisVerificationMethods,
    #[serde(rename(deserialize = "alsoKnownAs", serialize = "alsoKnownAs"))]
    pub also_known_as: Vec<String>,
    pub services: PlcGenesisServices,
    pub prev: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sig: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AssertionContents {
    pub signing_key: Option<String>,
    pub pds_endpoint: Option<String>,
    pub rotation_keys: Option<Vec<String>>,
}

/// Formatted xxxxx-xxxxx
pub fn get_random_token() -> String {
    let token: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();
    token[0..5].to_owned() + "-" + &token[5..10]
}

pub async fn safe_resolve_did_doc(
    id_resolver: &State<SharedIdResolver>,
    did: &String,
    force_refresh: Option<bool>,
) -> Result<Option<DidDocument>> {
    let mut lock = id_resolver.id_resolver.write().await;
    match lock.did.resolve(did.clone(), force_refresh).await {
        Ok(did_doc) => Ok(did_doc),
        Err(err) => {
            eprintln!(
                "@LOG: failed to resolve did doc for `{did}` with error: `{}`",
                err.to_string()
            );
            Ok(None)
        }
    }
}

/// generate an invite code preceded by the hostname
/// with '.'s replaced by '-'s, so it is not mistakable for a link
/// ex: blacksky-app-abc234-567xy
/// regex: blacksky-app-[a-z2-7]{5}-[a-z2-7]{5}
pub fn gen_invite_code() -> String {
    env::var("PDS_HOSTNAME")
        .unwrap_or("localhost".to_owned())
        .replace(".", "-")
        + "-"
        + &get_random_token().to_lowercase()
}

pub fn gen_invite_codes(count: i32) -> Vec<String> {
    let mut codes = Vec::new();
    for _i in 0..count {
        codes.push(gen_invite_code());
    }
    codes
}

pub fn validate_handle(handle: &str) -> bool {
    let suffix: String = env::var("PDS_HOSTNAME").unwrap_or("localhost".to_owned());
    let s_slice: &str = &suffix[..]; // take a full slice of the string
    handle.ends_with(s_slice)
    // Need to check suffix here and need to make sure handle doesn't include "." after trumming it
}

pub fn lookup_user_by_handle(handle: &str, conn: &mut PgConnection) -> Result<Actor> {
    use crate::schema::pds::actor::dsl as ActorSchema;

    let result = ActorSchema::actor
        .filter(ActorSchema::handle.eq(handle))
        .select(Actor::as_select())
        .first(conn)
        .map_err(|error| {
            let context = format!("no user found with handle '{}'", handle);
            anyhow::Error::new(error).context(context)
        })?;
    Ok(result)
}

pub fn sign(mut genesis: PlcGenesisOperation, private_key: &SecretKey) -> PlcGenesisOperation {
    let genesis_sig = atproto_sign(&genesis, private_key).unwrap();
    // Base 64 encode signature bytes
    genesis.sig = Some(base64_url::encode(&genesis_sig).replace("=", ""));
    genesis
}

/// https://github.com/gnunicorn/rust-multicodec/blob/master/src/lib.rs#L249-L260
pub fn multicodec_wrap(bytes: Vec<u8>) -> Vec<u8> {
    let mut buf = [0u8; 3];
    encode_varint(0xe7, &mut buf);
    let mut v: Vec<u8> = Vec::new();
    for b in &buf {
        v.push(*b);
        // varint uses first bit to indicate another byte follows, stop if not the case
        if *b <= 127 {
            break;
        }
    }
    v.extend(bytes);
    v
}

pub fn encode_did_key(pubkey: &PublicKey) -> String {
    let pk_compact = pubkey.serialize();
    let pk_wrapped = multicodec_wrap(pk_compact.to_vec());
    let pk_multibase = multibase::encode(Base58Btc, pk_wrapped.as_slice());
    format!("{DID_KEY_PREFIX}{pk_multibase}")
}

pub fn get_keys_from_private_key_str(private_key: String) -> Result<(SecretKey, PublicKey)> {
    let secp = Secp256k1::new();
    let decoded_key = hex::decode(private_key.as_bytes()).map_err(|error| {
        let context = format!("Issue decoding hex '{}'", private_key);
        anyhow::Error::new(error).context(context)
    })?;
    let secret_key = SecretKey::from_slice(&decoded_key).map_err(|error| {
        let context = format!("Issue creating secret key from input '{}'", private_key);
        anyhow::Error::new(error).context(context)
    })?;
    let public_key = secret_key.public_key(&secp);
    Ok((secret_key, public_key))
}

pub async fn create_did_and_plc_op(
    handle: &str,
    input: &CreateAccountInput,
    signing_key: Keypair,
) -> Result<String> {
    let private_key: String;
    if let Some(recovery_key) = &input.recovery_key {
        private_key = recovery_key.clone();
    } else {
        private_key = env::var("PDS_PLC_ROTATION_KEY_K256_PRIVATE_KEY_HEX").unwrap();
    }
    let (secret_key, public_key) = get_keys_from_private_key_str(private_key)?;

    println!("Generating and signing PLC directory genesis operation...");
    let mut create_op = PlcGenesisOperation {
        r#type: "plc_operation".to_owned(),
        rotation_keys: vec![encode_did_key(&public_key)],
        verification_methods: PlcGenesisVerificationMethods {
            atproto: encode_did_key(&signing_key.public_key()),
        },
        also_known_as: vec![format!("at://{handle}")],
        services: PlcGenesisServices {
            atproto_pds: AtprotoPdsService {
                r#type: "AtprotoPersonalDataServer".to_owned(),
                endpoint: format!(
                    "https://{}",
                    env::var("PDS_HOSTNAME").unwrap_or("localhost".to_owned())
                ),
            },
        },
        prev: None,
        sig: None,
    };
    create_op = sign(create_op, &secret_key);
    let json = serde_json::to_string(&create_op).unwrap();
    let hashmap_genesis: IndexMap<String, Value> = serde_json::from_str(&json).unwrap();
    let signed_genesis_bytes = serde_ipld_dagcbor::to_vec(&hashmap_genesis).unwrap();
    let mut hasher: Sha256 = Digest::new();
    hasher.update(signed_genesis_bytes.as_slice());
    let hash = hasher.finalize();
    let did_plc = &format!("did:plc:{}", BASE32.encode(&hash[..]))[..32].to_lowercase();
    println!("Created DID {did_plc:#}");
    println!("publishing......");

    // @TODO: Use plc::Client instead
    let plc_url = format!(
        "{0}/{1}",
        env::var("PLC_SERVER").unwrap_or("plc.directory".to_owned()),
        did_plc
    );
    println!("Publishing to {plc_url}");
    let client = reqwest::Client::builder()
        .user_agent(APP_USER_AGENT)
        .build()?;
    let response = client.post(plc_url).json(&create_op).send().await?;
    let res = &response;
    match res.error_for_status_ref() {
        Ok(_res) => Ok(did_plc.into()),
        Err(error) => Err(anyhow::Error::new(error).context(response.text().await?)),
    }
}

pub async fn is_valid_did_doc_for_service(did: String) -> Result<bool> {
    match assert_valid_did_documents_for_service(did).await {
        Ok(()) => Ok(true),
        Err(_) => Ok(false),
    }
}

pub async fn assert_valid_did_documents_for_service(did: String) -> Result<()> {
    if did.starts_with("did:plc") {
        let plc_url = env_str("PDS_DID_PLC_URL").unwrap_or("https://plc.directory".to_owned());
        let plc_client = plc::Client::new(plc_url);
        let resolved = plc_client.get_document_data(&did).await?;
        let pds_endpoint = match resolved.services.get("atproto_pds") {
            Some(service) => Some(service.endpoint.clone()),
            None => None,
        };
        let signing_key = match resolved.verification_methods.get("atproto") {
            Some(key) => Some(key.clone()),
            None => None,
        };
        assert_valid_doc_contents(AssertionContents {
            pds_endpoint,
            signing_key,
            rotation_keys: Some(resolved.rotation_keys),
        })
        .await?;
    } else {
        bail!("Not yet supporting did:web")
    }
    Ok(())
}

pub async fn assert_valid_doc_contents(contents: AssertionContents) -> Result<()> {
    let AssertionContents {
        signing_key,
        pds_endpoint,
        rotation_keys,
    } = contents;
    let private_key = env::var("PDS_PLC_ROTATION_KEY_K256_PRIVATE_KEY_HEX").unwrap();
    let (_, plc_rotation_key) = get_keys_from_private_key_str(private_key)?;
    let plc_rotation_key = encode_did_key(&plc_rotation_key);

    if let Some(rotation_keys) = rotation_keys {
        if !rotation_keys.contains(plc_rotation_key) {
            bail!("Server rotation key not included in PLC DID data")
        }
    }
    // @TODO: Move next 3 lines to a shared config context
    let port = env_int("PDS_PORT").unwrap_or(2583);
    let hostname = env_str("PDS_HOSTNAME").unwrap_or("localhost".to_owned());
    let public_url = if hostname == "localhost" {
        format!("http://localhost:{port}")
    } else {
        format!("https://{hostname}")
    };

    if pds_endpoint.is_none() || pds_endpoint.unwrap() != public_url {
        bail!("DID document atproto_pds service endpoint does not match PDS public url")
    }

    let repo_signing_key = env::var("PDS_REPO_SIGNING_KEY_K256_PRIVATE_KEY_HEX").unwrap();
    let repo_signing_keypair =
        SecretKey::from_slice(&hex::decode(repo_signing_key.as_bytes()).unwrap()).unwrap();
    let secp = Secp256k1::new();
    let repo_public_key = repo_signing_keypair.public_key(&secp);
    if signing_key.is_none() || signing_key.unwrap() != encode_did_key(&repo_public_key) {
        bail!("DID document verification method does not match expected signing key")
    }
    Ok(())
}

/*
pub fn validate_existing_did(
    handle: &str,
    input_did: &str,
    signing_key: Keypair
) -> Result<String> {
    todo!()
}*/

pub mod activate_account;
pub mod check_account_status;
pub mod confirm_email;
pub mod create_account;
pub mod create_app_password;
pub mod create_invite_code;
pub mod create_invite_codes;
pub mod create_session;
pub mod deactivate_account;
pub mod delete_account;
pub mod delete_session;
pub mod describe_server;
pub mod get_account_invite_codes;
pub mod get_service_auth;
pub mod get_session;
pub mod list_app_passwords;
pub mod refresh_session;
pub mod request_account_delete;
pub mod request_email_confirmation;
pub mod request_email_update;
pub mod request_password_reset;
pub mod reserve_signing_key;
pub mod reset_password;
pub mod revoke_app_password;
pub mod update_email;
