use anyhow::Result;
use base64ct::{Base64, Encoding};
use chrono::offset::Utc as UtcOffset;
use chrono::DateTime;
use rand::{distributions::Alphanumeric, Rng};
use rsky_identity::did::atproto_data::VerificationMaterial;
use rsky_identity::types::DidDocument;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::thread;
use std::time::{Duration, SystemTime};
use thiserror::Error;
use url::Url;
use urlencoding::encode;

pub const RFC3339_VARIANT: &str = "%Y-%m-%dT%H:%M:%S%.3fZ";

#[derive(Error, Debug)]
pub enum BadContentTypeError {
    #[error("BadType: `{0}`")]
    BadType(String),
    #[error("Content-Type header is missing")]
    MissingType,
}

#[derive(Debug)]
pub struct GetServiceEndpointOpts {
    pub id: String,
    pub r#type: Option<String>,
}

pub fn now() -> String {
    let system_time = SystemTime::now();
    let dt: DateTime<UtcOffset> = system_time.into();
    format!("{}", dt.format(RFC3339_VARIANT))
}

pub fn wait(ms: u64) {
    thread::sleep(Duration::from_millis(ms))
}

pub fn beginning_of_time() -> String {
    let beginning_of_time = SystemTime::UNIX_EPOCH;
    let dt: DateTime<UtcOffset> = beginning_of_time.into();
    format!("{}", dt.format(RFC3339_VARIANT))
}

pub fn get_random_str() -> String {
    let token: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();
    token
}

pub fn struct_to_cbor<T: Serialize>(obj: &T) -> Result<Vec<u8>> {
    Ok(serde_ipld_dagcbor::to_vec(obj)?)
}

pub fn cbor_to_struct<T: DeserializeOwned>(bytes: Vec<u8>) -> Result<T> {
    Ok(serde_ipld_dagcbor::from_slice::<T>(bytes.as_slice())?)
}

pub fn json_to_b64url<T: Serialize>(obj: &T) -> Result<String> {
    Ok(Base64::encode_string((&serde_json::to_string(obj)?).as_ref()).replace("=", ""))
}

pub fn encode_uri_component(input: &String) -> String {
    encode(input).to_string()
}

// based on did-doc.ts
pub fn get_did(doc: &DidDocument) -> String {
    doc.id.clone()
}

pub fn get_handle(doc: &DidDocument) -> Option<String> {
    match &doc.also_known_as {
        None => None,
        Some(aka) => {
            let found = aka.into_iter().find(|name| name.starts_with("at://"));
            match found {
                None => None,
                // strip off at:// prefix
                Some(found) => Some(found[5..].to_string()),
            }
        }
    }
}

pub fn get_verification_material(
    doc: &DidDocument,
    key_id: &String,
) -> Option<VerificationMaterial> {
    let did = get_did(doc);
    let keys = &doc.verification_method;
    if let Some(keys) = keys {
        let found = keys
            .into_iter()
            .find(|key| key.id == format!("#{key_id}") || key.id == format!("{did}#{key_id}"));
        match found {
            Some(found) if found.public_key_multibase.is_some() => {
                let found = found.clone();
                Some(VerificationMaterial {
                    r#type: found.r#type,
                    public_key_multibase: found.public_key_multibase.unwrap(),
                })
            }
            _ => None,
        }
    } else {
        None
    }
}

pub fn get_notif_endpoint(doc: DidDocument) -> Option<String> {
    get_service_endpoint(
        doc,
        GetServiceEndpointOpts {
            id: "#bsky_notif".to_string(),
            r#type: Some("BskyNotificationService".to_string()),
        },
    )
}

#[tracing::instrument(skip_all)]
pub fn get_service_endpoint(doc: DidDocument, opts: GetServiceEndpointOpts) -> Option<String> {
    tracing::info!(
        "@LOG: common::get_service_endpoint() doc: {:?}; opts: {:?}",
        doc,
        opts
    );
    let did = get_did(&doc);
    match doc.service {
        None => None,
        Some(services) => {
            let found = services.iter().find(|service| {
                service.id == opts.id || service.id == format!("{}{}", did, opts.id)
            });
            match found {
                None => None,
                Some(found) => match opts.r#type {
                    None => validate_url(&found.service_endpoint),
                    Some(opts_type) if found.r#type == opts_type => {
                        validate_url(&found.service_endpoint)
                    }
                    _ => None,
                },
            }
        }
    }
}

// Check protocol and hostname to prevent potential SSRF
pub fn validate_url(url_str: &String) -> Option<String> {
    match Url::parse(url_str) {
        Err(_) => None,
        Ok(url) => {
            return if !vec!["http", "https"].contains(&url.scheme()) {
                None
            } else if url.host().is_none() {
                None
            } else {
                Some(url_str.clone())
            }
        }
    }
}

pub mod r#async;
pub mod env;
pub mod explicit_slurs;
pub mod ipld;
pub mod sign;
pub mod tid;
pub mod time;
pub mod models;

#[cfg(test)]
mod tests {
    use crate::{get_service_endpoint, validate_url, GetServiceEndpointOpts};
    use rsky_identity::types::{DidDocument, Service};

    #[test]
    fn test_validate_url_when_invalid() {
        let text = "rsky.com".to_string();
        let result = validate_url(&text);
        assert_eq!(result, None);
    }

    #[test]
    fn test_validate_url_when_valid() {
        let text = "https://rsky.com".to_string();
        let result = validate_url(&text);
        assert_eq!(result.is_some(), true);
    }

    #[test]
    fn test_get_service_endpoint() {
        let text = "https://rsky.com".to_string();
        let result = validate_url(&text);

        let mut service = Vec::new();
        service.push(Service {
            id: "#bsky_chat".to_string(),
            r#type: "BskyChatService".to_string(),
            service_endpoint: "https://api.bsky.chat".to_string(),
        });
        let doc = DidDocument {
            context: None,
            id: "#bsky_chat".to_string(),
            also_known_as: None,
            verification_method: None,
            service: Some(service),
        };
        let opts = GetServiceEndpointOpts {
            id: "#bsky_chat".to_string(),
            r#type: Some("BskyChatService".to_string()),
        };
        let result = get_service_endpoint(doc, opts);
        assert_eq!(result.is_some(), true);
    }
}
