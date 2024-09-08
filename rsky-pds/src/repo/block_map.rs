use crate::common::ipld::data_to_cbor_block;
use crate::repo::types::{CidAndBytes, Lex, RepoRecord};
use crate::repo::util::lex_to_ipld;
use anyhow::Result;
use lexicon_cid::Cid;
use serde::Serialize;
use std::collections::BTreeMap;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct BlockMap {
    pub map: BTreeMap<String, Vec<u8>>,
}

impl BlockMap {
    pub fn new() -> Self {
        BlockMap {
            map: BTreeMap::new(),
        }
    }

    pub fn add<T: Serialize>(&mut self, value: T) -> Result<Cid> {
        let json = serde_json::to_value(&value)?;
        let record: RepoRecord = serde_json::from_value(json)?;
        let block = data_to_cbor_block(&lex_to_ipld(Lex::Map(record)))?;
        self.set(
            *block.cid(),
            block.data().to_vec(), //bytes
        );
        Ok(*block.cid())
    }

    pub fn set(&mut self, cid: Cid, bytes: Vec<u8>) -> () {
        self.map.insert(cid.to_string(), bytes);
        ()
    }

    pub fn get(&self, cid: Cid) -> Option<&Vec<u8>> {
        self.map.get(&cid.to_string())
    }
    pub fn delete(&mut self, cid: Cid) -> Result<()> {
        self.map.remove(&cid.to_string());
        Ok(())
    }

    pub fn get_many(&mut self, cids: Vec<Cid>) -> Result<BlocksAndMissing> {
        let mut missing: Vec<Cid> = Vec::new();
        let mut blocks = BlockMap::new();
        for cid in cids {
            let got = self.map.get(&cid.to_string());
            if let Some(bytes) = got {
                blocks.set(cid, bytes.clone());
            } else {
                missing.push(cid);
            }
        }
        Ok(BlocksAndMissing { blocks, missing })
    }

    pub fn has(&self, cid: Cid) -> bool {
        self.map.contains_key(&cid.to_string())
    }

    pub fn clear(&mut self) -> () {
        self.map.clear()
    }

    // Not really using. Issues with closures
    pub fn for_each(&self, cb: impl Fn(&Vec<u8>, Cid) -> ()) -> Result<()> {
        for (key, val) in self.map.iter() {
            cb(val, Cid::from_str(&key)?);
        }
        Ok(())
    }

    pub fn entries(&self) -> Result<Vec<CidAndBytes>> {
        let mut entries: Vec<CidAndBytes> = Vec::new();
        for (cid, bytes) in self.map.iter() {
            entries.push(CidAndBytes {
                cid: Cid::from_str(cid)?,
                bytes: bytes.clone(),
            });
        }
        Ok(entries)
    }

    pub fn cids(&self) -> Result<Vec<Cid>> {
        Ok(self.entries()?.into_iter().map(|e| e.cid).collect())
    }

    pub fn add_map(&mut self, to_add: BlockMap) -> Result<()> {
        let results = for (cid, bytes) in to_add.map.iter() {
            self.set(Cid::from_str(cid)?, bytes.clone());
        };
        Ok(results)
    }

    pub fn size(&self) -> usize {
        self.map.len()
    }

    pub fn byte_size(&self) -> Result<usize> {
        let mut size = 0;
        for (_, bytes) in self.map.iter() {
            size += bytes.len();
        }
        Ok(size)
    }

    pub fn equals(&self, other: BlockMap) -> Result<bool> {
        if self.size() != other.size() {
            return Ok(false);
        }
        for entry in self.entries()? {
            let other_bytes = other.get(entry.cid);
            if let Some(o) = other_bytes {
                if &entry.bytes != o {
                    return Ok(false);
                }
            } else {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

#[derive(Debug)]
pub struct BlocksAndMissing {
    pub blocks: BlockMap,
    pub missing: Vec<Cid>,
}
