//! parse an atproto DID document
//!
//! https://atproto.com/specs/did#blessed-did-methods#did-documents
//!
//! thin wrapper for jacquard_identity's DidDocResponse entry

use std::time::SystemTime;

use jacquard_identity::resolver::DidDocResponse;
use reqwest::StatusCode;

use crate::storage::repo::RepoIdentity;
use crate::{Did, HostRegistry};

#[derive(Debug, thiserror::Error, Clone)]
pub enum DidDocError {
    #[error("could not parse DID document: {0}")]
    FailedParse(String),
    #[error("bad DID document: {0}")]
    BadDocument(String),
    #[error("DID document missing required field: {0}")]
    MissingField(&'static str),
}

pub fn parse(
    raw: Vec<u8>,
    for_did: &Did,
    hosts: &HostRegistry,
    now: SystemTime,
) -> Result<RepoIdentity, DidDocError> {
    // a "response" for jacquard to parse
    let fake_resp = DidDocResponse {
        buffer: raw.into(),
        status: StatusCode::OK,
        requested: Some(for_did.into()),
    };

    let doc = fake_resp
        .parse_validated()
        .map_err(|e| DidDocError::FailedParse(e.to_string()))?;

    let pds = doc
        .pds_endpoint()
        .ok_or(DidDocError::MissingField("pds endpoint"))?;
    let pds_name = pds
        .authority()
        .map(|a| a.host().to_string())
        .ok_or(DidDocError::BadDocument(format!("invalid pds: {pds}")))?;
    let pds_host = hosts
        .get(&pds_name)
        .map_err(|e| DidDocError::BadDocument(format!("pds hostname: {e}")))?;

    let signing_key = doc
        .atproto_multikey()
        .ok_or(DidDocError::MissingField("atproto signing key"))?
        .parse()
        .map_err(|e| DidDocError::BadDocument(format!("signing key: {e}")))?;

    let supposed_handle = doc.handles().first().map(|h| h.to_string());

    Ok(RepoIdentity {
        pds_host,
        signing_key,
        supposed_handle,
        resolved_at: now,
    })
}
