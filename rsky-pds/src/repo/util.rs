use crate::common::tid::Ticker;
use crate::repo::types::{Commit, Lex, RecordPath, RepoRecord, UnsignedCommit, VersionedCommit};
use crate::storage::Ipld;
use anyhow::{bail, Result};
use indexmap::IndexMap;
use secp256k1::{Keypair, Message};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub fn sign_commit(unsigned: UnsignedCommit, keypair: Keypair) -> Result<Commit> {
    let json = serde_json::to_string(&unsigned).unwrap();
    let map_unsigned: IndexMap<String, Value> = serde_json::from_str(&json).unwrap();
    let unsigned_bytes = serde_ipld_dagcbor::to_vec(&map_unsigned).unwrap();
    let hash = Sha256::digest(&*unsigned_bytes);
    let message = Message::from_digest_slice(hash.as_ref()).unwrap();
    let mut sig = keypair.secret_key().sign_ecdsa(message);
    sig.normalize_s();
    let commit_sig = sig.serialize_compact();
    Ok(Commit {
        did: unsigned.did,
        version: unsigned.version,
        data: unsigned.data,
        rev: unsigned.rev,
        prev: unsigned.prev,
        sig: commit_sig.to_vec(),
    })
}

pub fn format_data_key(collection: String, rkey: String) -> String {
    format!("{collection}/{rkey}")
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
