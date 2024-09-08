use crate::repo::block_map::BlockMap;
use crate::repo::cid_set::CidSet;
use crate::repo::mst::diff::mst_diff;
use crate::repo::mst::{NodeEntry, MST};
use anyhow::Result;
use lexicon_cid::Cid;
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct DataAdd {
    key: String,
    cid: Cid,
}

#[derive(Debug)]
pub struct DataUpdate {
    key: String,
    prev: Cid,
    cid: Cid,
}

#[derive(Debug)]
pub struct DataDelete {
    key: String,
    cid: Cid,
}

#[derive(Debug)]
pub struct DataDiff {
    pub adds: BTreeMap<String, DataAdd>,
    pub updates: BTreeMap<String, DataUpdate>,
    pub deletes: BTreeMap<String, DataDelete>,

    pub new_mst_blocks: BlockMap,
    pub new_leaf_cids: CidSet,
    pub removed_cids: CidSet,
}

impl DataDiff {
    pub fn new() -> Self {
        DataDiff {
            adds: BTreeMap::new(),
            updates: BTreeMap::new(),
            deletes: BTreeMap::new(),
            new_mst_blocks: BlockMap::new(),
            new_leaf_cids: CidSet::new(None),
            removed_cids: CidSet::new(None),
        }
    }

    pub fn of(curr: &mut MST, prev: Option<&mut MST>) -> Result<DataDiff> {
        mst_diff(curr, prev)
    }

    pub fn node_add(&mut self, node: NodeEntry) -> Result<()> {
        match node {
            NodeEntry::Leaf(node) => self.leaf_add(&node.key, node.value),
            NodeEntry::MST(mut node) => {
                let data = node.serialize()?;
                self.tree_add(data.cid, data.bytes);
            }
        }
        Ok(())
    }

    pub fn node_delete(&mut self, node: NodeEntry) -> Result<()> {
        match node {
            NodeEntry::Leaf(node) => {
                let key = node.key;
                let cid = node.value;
                self.deletes.insert(
                    key.clone(),
                    DataDelete {
                        key: key.clone(),
                        cid,
                    },
                );
                self.removed_cids.add(cid);
            }
            NodeEntry::MST(mut node) => {
                let cid = node.get_pointer()?;
                self.tree_delete(cid)?;
            }
        }
        Ok(())
    }

    pub fn leaf_add(&mut self, key: &String, cid: Cid) -> () {
        self.adds.insert(
            key.clone(),
            DataAdd {
                key: key.clone(),
                cid,
            },
        );
        if self.removed_cids.has(cid) {
            self.removed_cids.delete(cid);
        } else {
            self.new_leaf_cids.add(cid);
        }
        ()
    }

    pub fn leaf_update(&mut self, key: &String, prev: Cid, cid: Cid) -> () {
        if prev.eq(&cid) {
            return ();
        }
        self.updates.insert(
            key.clone(),
            DataUpdate {
                key: key.clone(),
                prev,
                cid,
            },
        );
        self.removed_cids.add(prev);
        self.new_leaf_cids.add(cid);
    }

    pub fn leaf_delete(&mut self, key: &String, cid: Cid) -> () {
        self.deletes.insert(
            key.clone(),
            DataDelete {
                key: key.clone(),
                cid,
            },
        );
        if self.new_leaf_cids.has(cid) {
            self.new_leaf_cids.delete(cid);
        } else {
            self.removed_cids.add(cid);
        }
    }

    pub fn tree_add(&mut self, cid: Cid, bytes: Vec<u8>) -> () {
        if self.removed_cids.has(cid) {
            self.removed_cids.delete(cid);
        } else {
            self.new_mst_blocks.set(cid, bytes);
        }
        ()
    }

    pub fn tree_delete(&mut self, cid: Cid) -> Result<()> {
        if self.new_mst_blocks.has(cid) {
            self.new_mst_blocks.delete(cid)?;
        } else {
            self.removed_cids.add(cid);
        }
        Ok(())
    }
}
