use crate::common::sign::sign_without_indexmap;
use crate::common::tid::Ticker;
use crate::repo::types::{Commit, Lex, RecordPath, RepoRecord, UnsignedCommit, VersionedCommit};
use crate::storage::Ipld;
use anyhow::{bail, Result};
use futures::{Stream, StreamExt};
use lexicon_cid::Cid;
use secp256k1::Keypair;
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::Display;
use std::str::FromStr;

pub fn sign_commit(unsigned: UnsignedCommit, keypair: Keypair) -> Result<Commit> {
    let commit_sig = sign_without_indexmap(&unsigned, &keypair.secret_key())?;
    Ok(Commit {
        did: unsigned.did,
        version: unsigned.version,
        data: unsigned.data,
        rev: unsigned.rev,
        prev: unsigned.prev,
        sig: commit_sig.to_vec(),
    })
}

pub fn verify_commit_sig(commit: Commit, did_key: &String) -> Result<bool> {
    let sig = commit.sig;
    let rest = UnsignedCommit {
        did: commit.did,
        rev: commit.rev,
        data: commit.data,
        prev: commit.prev,
        version: commit.version,
    };
    let encoded = serde_ipld_dagcbor::to_vec(&rest)?;
    let hash = Sha256::digest(&*encoded);
    rsky_crypto::verify::verify_signature(did_key, hash.as_ref(), sig.as_slice(), None)
}

pub fn format_data_key<T: FromStr + Display>(collection: T, rkey: T) -> String {
    format!("{collection}/{rkey}")
}

pub fn lex_to_ipld(val: Lex) -> Ipld {
    match val {
        Lex::List(list) => Ipld::List(
            list.into_iter()
                .map(|item| lex_to_ipld(item))
                .collect::<Vec<Ipld>>(),
        ),
        Lex::Map(map) => {
            let mut to_return: BTreeMap<String, Ipld> = BTreeMap::new();
            for key in map.keys() {
                to_return.insert(key.to_owned(), lex_to_ipld(map.get(key).unwrap().clone()));
            }
            Ipld::Map(to_return)
        }
        Lex::Blob(blob) => {
            Ipld::Json(serde_json::to_value(blob.original).expect("Issue serializing blob"))
        }
        Lex::Ipld(ipld) => match ipld {
            Ipld::Json(json_val) => match serde_json::from_value::<Cid>(json_val.clone()) {
                Ok(cid) => Ipld::Link(cid),
                Err(_) => Ipld::Json(json_val),
            },
            _ => ipld,
        },
    }
}

pub fn ipld_to_lex(val: Ipld) -> Lex {
    match val {
        Ipld::List(list) => Lex::List(
            list.into_iter()
                .map(|item| ipld_to_lex(item))
                .collect::<Vec<Lex>>(),
        ),
        Ipld::Map(map) => {
            let mut to_return: BTreeMap<String, Lex> = BTreeMap::new();
            for key in map.keys() {
                to_return.insert(key.to_owned(), ipld_to_lex(map.get(key).unwrap().clone()));
            }
            Lex::Map(to_return)
        }
        Ipld::Json(blob)
            if blob.get("$type") == Some(&JsonValue::String("blob".to_string()))
                || (matches!(blob.get("cid"), Some(&JsonValue::String(_)))
                    && matches!(blob.get("mimeType"), Some(&JsonValue::String(_)))) =>
        {
            Lex::Blob(serde_json::from_value(blob).expect("Issue deserializing blob"))
        }
        _ => Lex::Ipld(val),
    }
}

pub fn cbor_to_lex(val: Vec<u8>) -> Result<Lex> {
    let obj: Ipld = serde_ipld_dagcbor::from_slice(val.as_slice())?; //cbordecode
    Ok(ipld_to_lex(obj))
}

pub fn cbor_to_lex_record(val: Vec<u8>) -> Result<RepoRecord> {
    let parsed = cbor_to_lex(val)?;
    match parsed {
        Lex::Map(map) => Ok(map),
        _ => bail!("Lexicon record should be a json object"),
    }
}

pub fn parse_data_key(key: &String) -> Result<RecordPath> {
    let parts: Vec<&str> = key.split("/").collect();
    if parts.len() != 2 {
        bail!("Invalid record key: `{key:?}`");
    }
    Ok(RecordPath {
        collection: parts[0].to_owned(),
        rkey: parts[1].to_owned(),
    })
}

pub fn ensure_v3_commit(commit: VersionedCommit) -> Commit {
    match commit {
        VersionedCommit::Commit(commit) if commit.version == 3 => commit,
        VersionedCommit::Commit(commit) => Commit {
            did: commit.did,
            version: 3,
            data: commit.data,
            rev: commit.rev,
            prev: commit.prev,
            sig: commit.sig,
        },
        VersionedCommit::LegacyV2Commit(commit) => Commit {
            did: commit.did,
            version: 3,
            data: commit.data,
            rev: commit.rev.unwrap_or(Ticker::new().next(None).0),
            prev: commit.prev,
            sig: commit.sig,
        },
    }
}

/// Flattens a collection of byte vectors into a single vector
pub fn flatten_u8_arrays(chunks: &[Vec<u8>]) -> Vec<u8> {
    let mut result = Vec::with_capacity(chunks.iter().map(|v| v.len()).sum());
    for chunk in chunks {
        result.extend_from_slice(chunk);
    }
    result
}

/// Collects a stream of byte chunks into a single buffer
pub async fn stream_to_buffer<S>(mut stream: S) -> Result<Vec<u8>>
where
    S: Stream<Item = Result<Vec<u8>>> + Unpin,
{
    let mut buffer = Vec::new();
    while let Some(chunk) = stream.next().await {
        buffer.extend_from_slice(&chunk?);
    }
    Ok(buffer)
}
