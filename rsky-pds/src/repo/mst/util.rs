use super::{Leaf, NodeData, NodeEntry, TreeEntry, MST};
use crate::common;
use crate::common::ipld;
use crate::common::ipld::cid_for_cbor;
use crate::common::tid::Ticker;
use crate::storage::SqlRepoReader;
use anyhow::{anyhow, Result};
use lazy_static::lazy_static;
use lexicon_cid::Cid;
use rand::{thread_rng, Rng};
use regex::Regex;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::str;

fn is_valid_chars(input: &str) -> bool {
    lazy_static! {
        static ref RE: Regex = Regex::new(r"^[a-zA-Z0-9_\-:.]*$").unwrap();
    }
    RE.is_match(input)
}

// * Restricted to a subset of ASCII characters — the allowed characters are
// alphanumeric (A-Za-z0-9), period, dash, underscore, colon, or tilde (.-_:~)
// * Must have at least 1 and at most 512 characters
// * The specific record key values . and .. are not allowed
pub fn is_valid_repo_mst_path(key: &String) -> Result<bool> {
    let split: Vec<&str> = key.split("/").collect();

    return if key.len() <= 256
        && split.len() == 2
        && split[0].len() > 0
        && split[1].len() > 0
        && is_valid_chars(split[0])
        && is_valid_chars(split[1])
    {
        Ok(true)
    } else {
        Ok(false)
    };
}

pub fn ensure_valid_mst_key(key: &String) -> Result<()> {
    let result = is_valid_repo_mst_path(key)?;
    match result {
        true => Ok(()),
        _ => Err(anyhow!("Invalid MST Key: {}", key)),
    }
}

pub fn cid_for_entries(entries: Vec<NodeEntry>) -> Result<Cid> {
    let data = serialize_node_data(entries)?;
    ipld::cid_for_cbor(&data)
}

pub fn count_prefix_len(a: String, b: String) -> Result<usize> {
    let mut x = 0;
    for i in 0..a.len() {
        match (a.chars().nth(i), b.chars().nth(i)) {
            (Some(a), Some(b)) if a == b => x += 1,
            _ => break,
        }
    }
    Ok(x)
}

pub fn serialize_node_data(entries: Vec<NodeEntry>) -> Result<NodeData> {
    let mut data = NodeData {
        l: None,
        e: Vec::new(),
    };
    let mut i = 0;
    if let Some(NodeEntry::MST(e)) = entries.get(0) {
        i += 1;
        data.l = Some(e.clone().get_pointer()?);
    }
    let mut last_key = "";
    while i < entries.len() {
        let leaf = &entries[i];
        let next = entries.get(i + 1);
        if let NodeEntry::Leaf(l) = leaf {
            i += 1;
            let mut subtree: Option<Cid> = None;
            match next {
                Some(NodeEntry::MST(tree)) => {
                    subtree = Some(tree.clone().get_pointer()?);
                    i += 1;
                }
                _ => (),
            };
            ensure_valid_mst_key(&l.key)?;
            let prefix_len = count_prefix_len(last_key.to_owned(), l.key.to_owned())?;
            data.e.push(TreeEntry {
                p: u8::try_from(prefix_len)?,
                k: l.key[prefix_len..].to_owned().into_bytes(),
                v: l.value,
                t: subtree,
            });
            last_key = &l.key;
        } else {
            return Err(anyhow!("Not a valid node: two subtrees next to each other"));
        }
    }
    Ok(data)
}

pub fn deserialize_node_data(
    storage: &SqlRepoReader,
    data: NodeData,
    layer: Option<u32>,
) -> Result<Vec<NodeEntry>> {
    let mut entries: Vec<NodeEntry> = Vec::new();
    if let Some(l) = data.l {
        let new_layer: Option<u32>;
        if let Some(layer) = layer {
            new_layer = Some(layer - 1);
        } else {
            new_layer = None;
        }
        let mst = MST::load(storage.clone(), l, new_layer)?;
        let mst = NodeEntry::MST(mst);
        entries.push(mst)
    }
    let mut last_key: String = "".to_owned();
    for entry in data.e {
        let key_str = str::from_utf8(entry.k.as_ref())?;
        let p = usize::try_from(entry.p)?;
        let key = format!("{}{}", &last_key[0..p], key_str);
        ensure_valid_mst_key(&key)?;
        entries.push(NodeEntry::Leaf(Leaf {
            key: key.clone(),
            value: entry.v,
        }));
        last_key = key;
        if let Some(t) = entry.t {
            let new_layer: Option<u32>;
            if let Some(layer) = layer {
                new_layer = Some(layer - 1);
            } else {
                new_layer = None;
            }
            let mst = MST::load(storage.clone(), t, new_layer)?;
            let mst = NodeEntry::MST(mst);
            entries.push(mst)
        }
    }
    Ok(entries)
}

pub fn layer_for_entries(entries: Vec<NodeEntry>) -> Result<Option<u32>> {
    let first_leaf = entries.into_iter().find(|entry| entry.is_leaf());
    if let Some(f) = first_leaf {
        match f {
            NodeEntry::MST(_) => Ok(None),
            NodeEntry::Leaf(l) => Ok(Some(leading_zeros_on_hash(&l.key.to_owned().into_bytes())?)),
        }
    } else {
        return Ok(None);
    }
}

pub fn leading_zeros_on_hash(key: &Vec<u8>) -> Result<u32> {
    let digest = Sha256::digest(&*key);
    let hash: &[u8] = digest.as_ref();
    let mut leading_zeros = 0;
    for byte in hash {
        if *byte < 64 {
            leading_zeros += 1
        };
        if *byte < 16 {
            leading_zeros += 1
        };
        if *byte < 4 {
            leading_zeros += 1
        };
        if *byte == 0 {
            leading_zeros += 1;
        } else {
            break;
        }
    }
    Ok(leading_zeros)
}

pub type IdMapping = BTreeMap<String, Cid>;

pub fn random_cid(storage: &mut Option<&mut SqlRepoReader>) -> Result<Cid> {
    let record = json!({ "test": random_str(50) });
    let cid = cid_for_cbor(&record)?;
    let bytes = common::struct_to_cbor(record)?;
    if let Some(ref mut storage) = storage {
        storage.blocks.set(cid, bytes);
    }
    Ok(cid)
}

pub fn generate_bulk_data_keys(
    count: usize,
    mut blockstore: Option<&mut SqlRepoReader>,
) -> Result<IdMapping> {
    let mut obj: IdMapping = BTreeMap::new();
    for _ in 0..count {
        let key = format!(
            "com.example.record/{}",
            Ticker::new().next(None).to_string()
        );
        obj.insert(key, random_cid(&mut blockstore)?);
    }
    Ok(obj)
}

pub fn random_str(len: usize) -> String {
    const CHARSET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    let charset_len = CHARSET.len();
    let mut rng = thread_rng();

    let result: String = (0..len)
        .map(|_| {
            let idx = rng.gen_range(0..charset_len);
            CHARSET[idx] as char
        })
        .collect();
    result
}

pub async fn save_mst(storage: &SqlRepoReader, mst: &mut MST) -> Result<Cid> {
    let diff = mst.get_unstored_blocks()?;
    storage
        .put_many(diff.blocks, Ticker::new().next(None).to_string())
        .await?;
    Ok(diff.root)
}
