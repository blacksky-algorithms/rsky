use cid::Cid;
use hashbrown::HashMap;
use ipld_core::ipld::Ipld;
use rs_car_sync::CarReader;
use serde::{Deserialize, Serialize};

use rsky_common::tid::TID;

use crate::validator::event::{ParseError, SubscribeReposCommit, SubscribeReposCommitOperation};

pub type BlockMap = HashMap<Cid, Vec<u8>>;

#[derive(Debug, Serialize, Deserialize)]
pub struct RepoState {
    pub rev: TID,
    pub head: Cid,
    pub data: Cid,
}

impl SubscribeReposCommit {
    pub fn tree(&self, root: Cid) -> Result<Node, ParseError> {
        let mut blocks = self.blocks.as_slice();
        let mut block_map = HashMap::new();
        for next in CarReader::new(&mut blocks, true)? {
            let (cid, block) = next?;
            block_map.insert(cid, block);
        }

        let Some(tree) = Node::load(&block_map, root)? else {
            return Err(ParseError::MissingRoot(root));
        };
        Ok(tree)
    }
}

#[derive(Debug)]
pub enum NodeEntry {
    #[expect(dead_code)]
    Direct {
        key: Vec<u8>,
        value: Cid,
    },
    Indirect {
        cid: Cid,
        child: Option<Node>,
    },
}

#[derive(Debug)]
pub struct Node {
    #[expect(dead_code)]
    pub cid: Cid,
    pub entries: Vec<NodeEntry>,
}

impl Node {
    pub fn invert(&mut self, op: &SubscribeReposCommitOperation) -> bool {
        match op {
            SubscribeReposCommitOperation::Create { path, cid } => {
                let Some(found) = self.remove(path.as_str(), Some(*cid)) else {
                    tracing::debug!("unable to invert create: not found (expected: {cid})");
                    return false;
                };
                if found == *cid {
                    true
                } else {
                    tracing::debug!("unable to invert create: {found} (expected: {cid})");
                    false
                }
            }
            SubscribeReposCommitOperation::Update { path, cid, prev_data } => {
                #[expect(clippy::unwrap_used)]
                let Some(found) = self.insert(path.as_str(), prev_data.unwrap(), Some(*cid)) else {
                    tracing::debug!("unable to invert update: not found (expected: {cid})");
                    return false;
                };
                if found == *cid {
                    true
                } else {
                    tracing::debug!("unable to invert update: {found} (expected: {cid})");
                    false
                }
            }
            SubscribeReposCommitOperation::Delete { path, prev_data } => {
                #[expect(clippy::unwrap_used)]
                let Some(found) = self.insert(path.as_str(), prev_data.unwrap(), None) else {
                    return true;
                };
                tracing::debug!("unable to invert delete: {found}");
                false
            }
        }
    }

    #[expect(clippy::needless_pass_by_ref_mut, clippy::unused_self)]
    fn insert(&mut self, _path: &str, _data: Cid, ret: Option<Cid>) -> Option<Cid> {
        // TODO: implement this & remove ret
        ret
    }

    #[expect(clippy::needless_pass_by_ref_mut, clippy::unused_self)]
    fn remove(&mut self, _path: &str, ret: Option<Cid>) -> Option<Cid> {
        // TODO: implement this & remove ret
        ret
    }

    fn load(block_map: &BlockMap, cid: Cid) -> Result<Option<Self>, ParseError> {
        let Some(block) = block_map.get(&cid) else {
            return Ok(None);
        };

        let data: NodeData = serde_ipld_dagcbor::from_slice(block)?;
        let mut node = data.into_node(cid);
        for entry in &mut node.entries {
            if let NodeEntry::Indirect { cid, child } = entry {
                *child = Self::load(block_map, *cid)?;
            }
        }
        Ok(Some(node))
    }
}

#[derive(Debug, Deserialize)]
struct NodeData {
    #[serde(rename = "l")]
    left: Option<Cid>,
    #[serde(rename = "e")]
    entries: Vec<EntryData>,
}

impl NodeData {
    fn into_node(self, cid: Cid) -> Node {
        let mut entries = Vec::new();
        if let Some(cid) = self.left {
            entries.push(NodeEntry::Indirect { cid, child: None });
        }
        let mut prev = Vec::new();
        for entry in self.entries {
            let mut key = Vec::new();
            key.extend_from_slice(&prev[..entry.prefix_len]);
            let Ipld::Bytes(key_suffix) = &entry.key_suffix else {
                unreachable!();
            };
            key.extend_from_slice(key_suffix);
            prev = key.clone();
            entries.push(NodeEntry::Direct { key, value: entry.value });
            if let Some(cid) = entry.right {
                entries.push(NodeEntry::Indirect { cid, child: None });
            }
        }
        Node { cid, entries }
    }
}

#[derive(Debug, Deserialize)]
struct EntryData {
    #[serde(rename = "p")]
    prefix_len: usize,
    #[serde(rename = "k")]
    key_suffix: Ipld,
    #[serde(rename = "v")]
    value: Cid,
    #[serde(rename = "t")]
    right: Option<Cid>,
}
