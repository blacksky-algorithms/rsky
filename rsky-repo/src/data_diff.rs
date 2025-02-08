use crate::block_map::BlockMap;
use crate::cid_set::CidSet;
use crate::mst::diff::mst_diff;
use crate::mst::{NodeEntry, MST};
use anyhow::Result;
use lexicon_cid::Cid;
use std::collections::HashMap;
use std::fmt::Debug;

#[derive(Debug, Clone, PartialEq)]
pub struct DataAdd {
    pub(crate) key: String,
    pub(crate) cid: Cid,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DataUpdate {
    pub(crate) key: String,
    pub(crate) prev: Cid,
    pub(crate) cid: Cid,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DataDelete {
    pub(crate) key: String,
    pub(crate) cid: Cid,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DataDiff {
    pub adds: HashMap<String, DataAdd>,
    pub updates: HashMap<String, DataUpdate>,
    pub deletes: HashMap<String, DataDelete>,

    pub new_mst_blocks: BlockMap,
    pub new_leaf_cids: CidSet,
    pub removed_cids: CidSet,
}

impl DataDiff {
    pub fn new() -> Self {
        DataDiff {
            adds: HashMap::new(),
            updates: HashMap::new(),
            deletes: HashMap::new(),
            new_mst_blocks: BlockMap::new(),
            new_leaf_cids: CidSet::new(None),
            removed_cids: CidSet::new(None),
        }
    }

    pub async fn of(curr: &mut MST, prev: Option<&mut MST>) -> Result<DataDiff> {
        mst_diff(curr, prev).await
    }

    pub async fn node_add(&mut self, node: NodeEntry) -> Result<()> {
        match node {
            NodeEntry::Leaf(node) => self.leaf_add(&node.key, node.value),
            NodeEntry::MST(node) => {
                let data = node.serialize().await?;
                self.tree_add(data.cid, data.bytes);
            }
        }
        Ok(())
    }

    pub async fn node_delete(&mut self, node: NodeEntry) -> Result<()> {
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
            NodeEntry::MST(node) => {
                let cid = node.get_pointer().await?;
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

    pub fn add_list(&self) -> Vec<DataAdd> {
        self.adds.values().cloned().collect()
    }

    pub fn update_list(&self) -> Vec<DataUpdate> {
        self.updates.values().cloned().collect()
    }

    pub fn delete_list(&self) -> Vec<DataDelete> {
        self.deletes.values().cloned().collect()
    }
}
