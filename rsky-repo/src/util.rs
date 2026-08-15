use crate::data_diff::DataDiff;
use crate::storage::Ipld;
use crate::types::{
    Commit, Lex, RecordCreateOrDeleteDescript, RecordPath, RecordUpdateDescript,
    RecordWriteDescript, RepoRecord, UnsignedCommit, VersionedCommit, WriteOpAction,
};
use anyhow::{bail, Result};
use futures::{stream, Stream, StreamExt, TryStreamExt};
use lexicon_cid::Cid;
use rsky_common::sign::sign_without_indexmap;
use rsky_common::tid::Ticker;
use rsky_lexicon::blob_refs::{BlobRef, JsonBlobRef};
use secp256k1::Keypair;
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::Display;
use std::str::FromStr;
use tokio::try_join;

pub fn sign_commit(unsigned: UnsignedCommit, keypair: &Keypair) -> Result<Commit> {
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
        Lex::List(list) => Ipld::List(list.into_iter().map(lex_to_ipld).collect::<Vec<Ipld>>()),
        Lex::Map(map) => {
            let mut to_return: BTreeMap<String, Ipld> = BTreeMap::new();
            for key in map.keys() {
                to_return.insert(key.to_owned(), lex_to_ipld(map.get(key).unwrap().clone()));
            }
            Ipld::Map(to_return)
        }
        // A blob ref's `ref` must round-trip as a real IPLD link so DAG-CBOR
        // writes CBOR tag 42; a JSON-shaped `$link` map produces a different
        // block and a different CID than every other implementation.
        Lex::Blob(blob) => match blob.original {
            JsonBlobRef::Typed(typed) => match typed.r#ref.link.parse::<Cid>() {
                Ok(cid) => Ipld::Map(BTreeMap::from([
                    ("$type".to_string(), Ipld::String("blob".to_string())),
                    ("ref".to_string(), Ipld::Link(cid)),
                    ("mimeType".to_string(), Ipld::String(typed.mime_type)),
                    (
                        "size".to_string(),
                        Ipld::Json(serde_json::Value::from(typed.size)),
                    ),
                ])),
                Err(_) => Ipld::Json(
                    serde_json::to_value(JsonBlobRef::Typed(typed))
                        .expect("Issue serializing blob"),
                ),
            },
            JsonBlobRef::Untyped(legacy) => Ipld::Map(BTreeMap::from([
                ("cid".to_string(), Ipld::String(legacy.cid)),
                ("mimeType".to_string(), Ipld::String(legacy.mime_type)),
            ])),
        },
        Lex::Ipld(ipld) => match ipld {
            Ipld::Json(json_val) => match serde_json::from_value::<Cid>(json_val.clone()) {
                Ok(cid) => Ipld::Link(cid),
                Err(_) => Ipld::Json(json_val),
            },
            _ => ipld,
        },
    }
}

/// A DAG-CBOR blob reference decodes as an `Ipld::Map` whose `ref` is an
/// `Ipld::Link`; recognize that shape here or every blob ref parsed from a
/// CAR dissolves into a plain map and record-blob associations are lost.
fn ipld_map_as_blob(map: &BTreeMap<String, Ipld>) -> Option<BlobRef> {
    let typed = matches!(map.get("$type"), Some(Ipld::String(t)) if t == "blob");
    let legacy = matches!(map.get("cid"), Some(Ipld::String(_)))
        && matches!(map.get("mimeType"), Some(Ipld::String(_)));
    if !typed && !legacy {
        return None;
    }
    let mut obj = serde_json::Map::new();
    for (key, value) in map {
        let json = match value {
            Ipld::Link(cid) => serde_json::json!({ "$link": cid.to_string() }),
            Ipld::String(s) => JsonValue::String(s.clone()),
            Ipld::Json(value) => value.clone(),
            _ => return None,
        };
        obj.insert(key.clone(), json);
    }
    serde_json::from_value::<JsonBlobRef>(JsonValue::Object(obj))
        .ok()
        .map(|original| BlobRef { original })
}

pub fn ipld_to_lex(val: Ipld) -> Lex {
    match val {
        Ipld::List(list) => Lex::List(list.into_iter().map(ipld_to_lex).collect::<Vec<Lex>>()),
        Ipld::Map(map) => {
            if let Some(blob) = ipld_map_as_blob(&map) {
                return Lex::Blob(blob);
            }
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

pub fn ensure_creates(
    descripts: Vec<RecordWriteDescript>,
) -> Result<Vec<RecordCreateOrDeleteDescript>> {
    let mut creates: Vec<RecordCreateOrDeleteDescript> = Default::default();
    for descript in descripts {
        match descript {
            RecordWriteDescript::Create(create) => creates.push(create),
            _ => bail!("Unexpected action: {}", descript.action()),
        }
    }
    Ok(creates)
}

pub async fn diff_to_write_descripts(diff: &DataDiff) -> Result<Vec<RecordWriteDescript>> {
    let (add_list, update_list, delete_list) = try_join!(
        // Process add_list
        stream::iter(diff.add_list())
            .then(|add| async move {
                let RecordPath { collection, rkey } = parse_data_key(&add.key)?;
                Ok::<RecordWriteDescript, anyhow::Error>(RecordWriteDescript::Create(
                    RecordCreateOrDeleteDescript {
                        action: WriteOpAction::Create,
                        collection,
                        rkey,
                        cid: add.cid,
                    },
                ))
            })
            .try_collect::<Vec<_>>(),
        // Process update_list
        stream::iter(diff.update_list())
            .then(|upd| async move {
                let RecordPath { collection, rkey } = parse_data_key(&upd.key)?;
                Ok::<RecordWriteDescript, anyhow::Error>(RecordWriteDescript::Update(
                    RecordUpdateDescript {
                        action: WriteOpAction::Update,
                        collection,
                        rkey,
                        cid: upd.cid,
                        prev: upd.prev,
                    },
                ))
            })
            .try_collect::<Vec<_>>(),
        // Process delete_list
        stream::iter(diff.delete_list())
            .then(|del| async move {
                let RecordPath { collection, rkey } = parse_data_key(&del.key)?;
                Ok::<RecordWriteDescript, anyhow::Error>(RecordWriteDescript::Delete(
                    RecordCreateOrDeleteDescript {
                        action: WriteOpAction::Delete,
                        collection,
                        rkey,
                        cid: del.cid,
                    },
                ))
            })
            .try_collect::<Vec<_>>()
    )?;
    Ok([add_list, update_list, delete_list].concat())
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The encode direction: a `Lex::Blob` written to DAG-CBOR must emit a
    /// real tag-42 CID link and re-encode byte-for-byte to what a compliant
    /// implementation produced, or native writes diverge from every other
    /// PDS and imported records index under the wrong CID.
    #[test]
    fn blob_ref_encodes_as_tag42_and_round_trips() {
        let cid: Cid = "bafkreiey6e2xp4ncufvsyfmubucbsz5xujbc7lguospuziohtgfdik3pr4"
            .parse()
            .unwrap();
        let original = Ipld::Map(BTreeMap::from([
            (
                "$type".to_string(),
                Ipld::String("app.bsky.actor.profile".to_string()),
            ),
            (
                "avatar".to_string(),
                Ipld::Map(BTreeMap::from([
                    ("$type".to_string(), Ipld::String("blob".to_string())),
                    ("ref".to_string(), Ipld::Link(cid)),
                    (
                        "mimeType".to_string(),
                        Ipld::String("image/jpeg".to_string()),
                    ),
                    ("size".to_string(), Ipld::Json(JsonValue::from(924586))),
                ])),
            ),
        ]));
        let original_bytes = serde_ipld_dagcbor::to_vec(&original).unwrap();
        // tag 42 present in the source encoding
        assert!(original_bytes
            .windows(2)
            .any(|window| window == [0xd8, 0x2a]));
        // decode -> Lex (Lex::Blob) -> re-encode must reproduce the bytes
        let record = cbor_to_lex_record(original_bytes.clone()).unwrap();
        assert!(matches!(record.get("avatar"), Some(Lex::Blob(_))));
        let reencoded = serde_ipld_dagcbor::to_vec(&lex_to_ipld(Lex::Map(record))).unwrap();
        assert_eq!(reencoded, original_bytes);
    }

    /// A blob ref decoded from real DAG-CBOR (map with a tag-42 link) must
    /// come out as `Lex::Blob`, or record-blob associations are silently
    /// lost on every CAR import.
    #[test]
    fn cbor_blob_ref_parses_as_lex_blob() {
        let cid: Cid = "bafkreiey6e2xp4ncufvsyfmubucbsz5xujbc7lguospuziohtgfdik3pr4"
            .parse()
            .unwrap();
        let record = Ipld::Map(BTreeMap::from([
            (
                "$type".to_string(),
                Ipld::String("app.bsky.actor.profile".to_string()),
            ),
            (
                "avatar".to_string(),
                Ipld::Map(BTreeMap::from([
                    ("$type".to_string(), Ipld::String("blob".to_string())),
                    ("ref".to_string(), Ipld::Link(cid)),
                    (
                        "mimeType".to_string(),
                        Ipld::String("image/jpeg".to_string()),
                    ),
                    ("size".to_string(), Ipld::Json(JsonValue::from(924586))),
                ])),
            ),
        ]));
        let bytes = serde_ipld_dagcbor::to_vec(&record).unwrap();
        let parsed = cbor_to_lex_record(bytes).unwrap();
        match parsed.get("avatar") {
            Some(Lex::Blob(blob)) => {
                assert_eq!(blob.get_cid().unwrap(), cid);
            }
            other => panic!("avatar should parse as Lex::Blob, got {other:?}"),
        }
    }
}
