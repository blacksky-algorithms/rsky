use std::collections::BTreeMap;
use std::str::FromStr;
use libipld::Cid;
use anyhow::Result;
use serde::Serialize;
use crate::common;
use crate::common::ipld;

#[derive(Debug, Deserialize, Serialize)]
pub struct BlockMap {
    pub map: BTreeMap<String, Vec<u8>>
}

impl BlockMap {
    pub fn new() -> BlockMap {
        BlockMap {
            map: BTreeMap::new()
        }
    }

    pub fn add<T: Serialize>(
        &mut self,
        value: T
    ) -> Result<Cid> {
        let cid = ipld::cid_for_cbor(&value)?;
        self.set(
            cid,
            common::struct_to_cbor(value)? //bytes
        );
        Ok(cid)
    }

    pub fn set (
        &mut self,
        cid: Cid,
        bytes: Vec<u8>
    ) -> () {
        self.map.insert(cid.to_string(), bytes);
        ()
    }

    pub fn get (
        &self,
        cid: Cid
    ) -> Option<&Vec<u8>> {
        self.map.get(&cid.to_string())
    }
    pub fn delete (
        &mut self,
        cid: Cid
    ) -> Result<()> {
        self.map.remove(&cid.to_string());
        Ok(())
    }

    pub fn get_many (
        &mut self,
        cids: Vec<Cid>
    ) -> Result<BlocksAndMissing> {
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
        Ok(BlocksAndMissing {
            blocks,
            missing
        })
    }

    pub fn has (
        &self,
        cid: Cid
    ) -> bool {
        self.map.contains_key(&cid.to_string())
    }

    pub fn clear (
        &mut self
    ) -> () {
       self.map.clear()
    }

    pub fn for_each (
        &self,
        cb: impl Fn(&Vec<u8>, Cid) -> ()
    ) -> Result<()> {
        for (key, val) in self.map.iter() {
            cb(val, Cid::from_str(&key)?);
        }
        Ok(())
    }

    pub fn entries (
        &self
    ) -> Result<Vec<Entry>> {
        let mut entries: Vec<Entry> = Vec::new();
        self.for_each (
            |bytes, cid| {
                entries.push(Entry { cid, bytes: bytes.clone() })
            }
        )?;
        Ok(entries)
    }

    pub fn cids(&self) -> Result<Vec<Cid>> {
        Ok(self
            .entries()?
            .into_iter()
            .map(|e| e.cid)
            .collect())
    }

    pub fn add_map(
        &mut self,
        to_add: BlockMap
    ) -> Result<()> {
        let results = to_add.for_each(
            |bytes, cid| {
                self.set(cid, bytes.clone());
            }
        )?;
        Ok(results)
    }

    pub fn size(&self) -> usize {
        self.map.len()
    }

    pub fn byte_size(&self) -> Result<usize> {
        let mut size = 0;
        self.for_each(|bytes, _| {
            size += bytes.len();
        })?;
        Ok(size)
    }

    pub fn equals(
        &self,
        other: BlockMap
    ) -> Result<bool> {
        if (self.size() != other.size()) {
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

pub struct BlocksAndMissing {
    pub blocks: BlockMap,
    pub missing: Vec<Cid>
}

struct Entry {
    cid: Cid,
    bytes: Vec<u8>
}