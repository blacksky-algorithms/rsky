use std::cmp::Ordering;
use std::{io, mem};

use cid::multihash::{Code, Hasher, MultihashDigest};
use cid::{Cid, multihash};
use hashbrown::HashMap;
use ipld_core::codec::Codec;
use rs_car_sync::CarReader;
use serde::{Deserialize, Serialize};
use serde_ipld_dagcbor::codec::DagCborCodec;
use sha2::{Digest, Sha256};
use thiserror::Error;

use rsky_common::tid::TID;

use crate::validator::event::{
    Commit, ParseError, SubscribeReposCommit, SubscribeReposCommitOperation, SubscribeReposEvent,
};

// const FUTURE_REV_MAX: Duration = Duration::from_secs(60 * 5);
const MAX_BLOCKS_BYTES: usize = 2_000_000;
const MAX_COMMIT_OPS: usize = 200;
const ATPROTO_REPO_VERSION: u8 = 3;

pub type BlockMap = HashMap<Cid, Vec<u8>>;

#[derive(Debug, Serialize, Deserialize)]
pub struct RepoState {
    pub rev: TID,
    pub data: Cid,
    pub head: Cid,
}

impl SubscribeReposEvent {
    pub fn validate(&self, commit: &Commit, head: &Cid) -> bool {
        let rev = match &self {
            Self::Commit(commit) => {
                if commit.too_big {
                    tracing::debug!("deprecated tooBig commit flag set");
                    return false;
                }
                if commit.rebase {
                    tracing::debug!("deprecated rebase commit flag set");
                    return false;
                }
                if &commit.commit != head {
                    tracing::debug!("mismatched inner commit cid: {}", commit.commit);
                    return false;
                }
                if commit.ops.len() > MAX_COMMIT_OPS {
                    tracing::debug!("too many ops in commit: {}", commit.ops.len());
                    return false;
                }
                if commit.blocks.is_empty() {
                    tracing::debug!("commit messaging missing blocks");
                    return false;
                }
                if commit.blocks.len() > MAX_BLOCKS_BYTES {
                    tracing::debug!(
                        "blocks size ({} bytes) exceeds protocol limit",
                        commit.blocks.len()
                    );
                    return false;
                }
                &commit.rev
            }
            Self::Sync(sync) => {
                if sync.blocks.len() > MAX_BLOCKS_BYTES {
                    tracing::debug!(
                        "blocks size ({} bytes) exceeds protocol limit",
                        sync.blocks.len()
                    );
                    return false;
                }
                &sync.rev
            }
            _ => return true,
        };
        if commit.did != self.did() {
            tracing::debug!("mismatched inner commit did: {}", commit.did);
            return false;
        }
        if &commit.rev != rev {
            tracing::debug!("mismatched inner commit rev: {rev}");
            return false;
        }
        if commit.version != ATPROTO_REPO_VERSION {
            tracing::debug!("unsupported repo version: {}", commit.version);
            return false;
        }
        true
    }

    pub fn commit(&self) -> Result<Option<(Commit, Cid)>, ParseError> {
        let mut blocks = match self {
            Self::Commit(commit) => commit.blocks.as_slice(),
            Self::Sync(sync) => sync.blocks.as_slice(),
            Self::Identity(_) | Self::Account(_) => {
                return Ok(None);
            }
        };
        let reader = CarReader::new(&mut blocks, true)?;
        let root_cid = reader.header.roots[0];
        for next in reader {
            let (cid, block) = next?;
            if cid == root_cid {
                return Ok(Some((serde_ipld_dagcbor::from_slice(&block)?, cid)));
            }
        }
        Err(ParseError::MissingRoot(root_cid))
    }
}

impl SubscribeReposCommit {
    pub fn tree(&self, root: Cid) -> Result<Node, ParseError> {
        let mut blocks = self.blocks.as_slice();
        let mut block_map = HashMap::new();
        for next in CarReader::new(&mut blocks, true)? {
            let (cid, block) = next?;
            block_map.insert(cid, block);
        }

        let Some(mut tree) = Node::load(&block_map, root)? else {
            return Err(ParseError::MissingRoot(root));
        };
        tree.ensure_heights();
        Ok(tree)
    }
}

#[derive(Debug, Error)]
pub enum InvertError {
    #[error("serde error: {0}")]
    Serde(#[from] serde_ipld_dagcbor::EncodeError<io::Error>),
    #[error("multihash error: {0}")]
    Multihash(#[from] multihash::Error),
    #[error("nil tree node")]
    InvalidTree,
    #[error("partial tree")]
    PartialTree,
    #[error("can't determine key range of empty MST node")]
    EmptyTreeNode,
    #[error("malformed tree node")]
    MalformedTreeNode,
    #[error("partial MST, can't determine insertion order")]
    PartialTreeInsertionOrderError,
    #[error("unexpected split when inserting child")]
    UnexpectedSplit,
    #[error("tried to split an empty node")]
    EmptySplit,
    #[error("splitting at one end or the other of entries")]
    SplittingEnds,
    #[error("one of the legs is empty (idx={0}, len={0})")]
    SplitEmptyLegs(usize, usize),
}

#[derive(Debug)]
pub enum NodeEntry {
    Value { key: Vec<u8>, value: Cid, dirty: bool },
    Child { cid: Option<Cid>, child: Option<Node>, dirty: bool },
}

impl NodeEntry {
    const fn is_child(&self) -> bool {
        matches!(self, Self::Child { .. })
    }

    fn child_mut(&mut self) -> Result<&mut Node, InvertError> {
        match self {
            Self::Value { .. } => None,
            Self::Child { child, .. } => child.as_mut(),
        }
        .ok_or(InvertError::PartialTree)
    }

    fn child(self) -> Result<Node, InvertError> {
        match self {
            Self::Value { .. } => None,
            Self::Child { child, .. } => child,
        }
        .ok_or(InvertError::PartialTree)
    }
}

#[derive(Debug, Default)]
pub struct Node {
    pub entries: Vec<NodeEntry>,
    pub height: i8,
    pub dirty: bool,
    pub cid: Option<Cid>,
    pub stub: bool,
}

impl Node {
    pub fn invert(&mut self, op: &SubscribeReposCommitOperation) -> Result<bool, InvertError> {
        match op {
            SubscribeReposCommitOperation::Create { path, cid } => {
                let Some(found) = self.remove(path.as_str(), -1)? else {
                    tracing::debug!("unable to invert create: not found (expected: {cid})");
                    return Ok(false);
                };
                if found == *cid {
                    Ok(true)
                } else {
                    tracing::debug!("unable to invert create: {found} (expected: {cid})");
                    Ok(false)
                }
            }
            SubscribeReposCommitOperation::Update { path, cid, prev_data } => {
                #[expect(clippy::unwrap_used)]
                let Some(found) = self.insert(path.as_str(), prev_data.unwrap(), -1)? else {
                    tracing::debug!("unable to invert update: not found (expected: {cid})");
                    return Ok(false);
                };
                if found == *cid {
                    Ok(true)
                } else {
                    tracing::debug!("unable to invert update: {found} (expected: {cid})");
                    Ok(false)
                }
            }
            SubscribeReposCommitOperation::Delete { path, prev_data } => {
                #[expect(clippy::unwrap_used)]
                let Some(found) = self.insert(path.as_str(), prev_data.unwrap(), -1)? else {
                    return Ok(true);
                };
                tracing::debug!("unable to invert delete: {found}");
                Ok(false)
            }
        }
    }

    // Returns the overall root-node CID for the MST.
    // If possible, lazily returned a known value.
    // If necessary, recursively encodes tree nodes to compute CIDs.
    //
    // NOTE: will mark the tree "clean" (clear any dirty flags).
    pub fn root(&mut self) -> Result<Cid, InvertError> {
        if self.stub && !self.dirty {
            if let Some(cid) = self.cid {
                return Ok(cid);
            }
        }
        self.write_blocks()
    }

    // Recursively encodes sub-tree, optionally writing to blockstore. Returns root CID.
    //
    // This method will not error if tree is partial.
    fn write_blocks(&mut self) -> Result<Cid, InvertError> {
        if self.stub {
            return Err(InvertError::InvalidTree);
        }

        // walk all children first
        for entry in &mut self.entries {
            match entry {
                NodeEntry::Value { dirty, .. } => {
                    // TODO: should we actually clear this here?
                    *dirty = false;
                }
                NodeEntry::Child { cid, child, dirty } => {
                    if let Some(child) = child {
                        if *dirty || child.dirty {
                            let cc = child.write_blocks()?;
                            *cid = Some(cc);
                            *dirty = false;
                        }
                    }
                }
            }
        }

        // compute this block
        let nd = NodeData::from_node(self)?;
        let mut hasher = multihash::Sha2_256::default();
        serde_ipld_dagcbor::to_writer(&mut hasher, &nd)?;
        let mh = Code::Sha2_256.wrap(hasher.finalize())?;
        let cc = Cid::new_v1(<DagCborCodec as Codec<()>>::CODE, mh);
        self.cid = Some(cc);
        self.dirty = false;
        Ok(cc)
    }

    // Adds a key/CID entry to a sub-tree defined by a Node. If a previous value existed, returns it.
    //
    // If the insert is a no-op (the key already existed with exact value), then the operation
    // is a no-op, the tree is not marked dirty, and the val is returned as the 'prev' value.
    fn insert(&mut self, path: &str, val: Cid, mut height: i8) -> Result<Option<Cid>, InvertError> {
        if self.stub {
            return Err(InvertError::PartialTree);
        }
        if height < 0 {
            height = NodeData::height_for_key(path.as_bytes());
        }

        if height > self.height {
            // if the new key is higher in the tree; will need to add a parent node,
            // which may involve splitting this current node
            return self.insert_parent(path, val, height);
        }

        if height < self.height {
            // if key is lower on the tree, we need to descend first
            return self.insert_child(path, val, height);
        }

        // look for existing key
        if let Some(idx) = self.find_value(path) {
            let NodeEntry::Value { value, dirty, .. } = &mut self.entries[idx] else {
                unreachable!()
            };
            if *value == val {
                // same value already exists; no-op
                return Ok(Some(val));
            }
            // update operation
            let prev = *value;
            *value = val;
            *dirty = true;
            self.dirty = true;
            return Ok(Some(prev));
        }

        // insert new entry to this node
        let (idx, split) = self.find_insertion_index(path)?;
        self.dirty = true;
        let new_entry = NodeEntry::Value { key: path.as_bytes().into(), value: val, dirty: true };

        // include "covering" proof for this operation
        match self.prove_mutation(path) {
            Ok(()) | Err(InvertError::PartialTree) => {}
            Err(err) => return Err(err),
        }

        if !split {
            self.entries.insert(idx, new_entry);
            return Ok(None);
        }

        // we need to split
        let NodeEntry::Child { child, .. } = &mut self.entries[idx] else { unreachable!() };
        // remove the existing entry, and replace with three new entries
        let (left, right) = child.take().ok_or(InvertError::PartialTree)?.split(path)?;
        let left_entry = NodeEntry::Child { cid: None, child: Some(left), dirty: true };
        let right_entry = NodeEntry::Child { cid: None, child: Some(right), dirty: true };
        self.entries.splice(idx..=idx, [left_entry, new_entry, right_entry]);
        Ok(None)
    }

    // inserts a node "above" this node in tree, possibly splitting the current node
    fn insert_parent(
        &mut self, path: &str, val: Cid, height: i8,
    ) -> Result<Option<Cid>, InvertError> {
        if self.entries.is_empty() {
            // if current node is empty, just replace directly with current height
            *self = Self { height, dirty: true, ..Default::default() };
        } else {
            // otherwise push a layer and recurse
            let mut this = Self { height: self.height + 1, dirty: true, ..Default::default() };
            mem::swap(self, &mut this);
            self.entries.push(NodeEntry::Child { cid: None, child: Some(this), dirty: true });
        }
        // regular insertion will handle any necessary "split"
        self.insert(path, val, height)
    }

    // inserts a node "below" this node in tree; either creating a new child entry or re-using an existing one
    fn insert_child(
        &mut self, path: &str, val: Cid, height: i8,
    ) -> Result<Option<Cid>, InvertError> {
        // look for an existing child node which encompasses the key, and use that
        if let Some(idx) = self.find_child(path) {
            let NodeEntry::Child { child, .. } = &mut self.entries[idx] else { unreachable!() };
            let Some(child) = child else {
                return Err(InvertError::PartialTree);
            };
            let prev = child.insert(path, val, height)?;
            if let Some(prev) = &prev {
                if *prev == val {
                    // no-op
                    return Ok(Some(val));
                }
            }
            self.dirty = true;
            debug_assert!(child.dirty);
            return Ok(prev);
        }

        // insert a new child node. this might be recursive if the child is not a *direct* child
        let (idx, split) = self.find_insertion_index(path)?;
        if split {
            return Err(InvertError::UnexpectedSplit);
        }
        self.dirty = true;
        let mut new_child = Self { height: self.height - 1, dirty: true, ..Default::default() };
        new_child.insert(path, val, height)?;

        let new_entry = NodeEntry::Child { cid: None, child: Some(new_child), dirty: true };
        self.entries.insert(idx, new_entry);

        Ok(None)
    }

    fn split(mut self, path: &str) -> Result<(Self, Self), InvertError> {
        if self.entries.is_empty() {
            // TODO: this feels defensive and could be removed
            return Err(InvertError::EmptySplit);
        }

        let (idx, split) = self.find_insertion_index(path)?;
        if !split {
            // simple split based on values
            return self.split_entries(idx);
        }

        // need to split recursively
        let mut right_entries = self.entries.split_off(idx);
        let NodeEntry::Child { child, .. } = &mut right_entries[0] else { unreachable!() };
        let (left_node, right_node) = child.take().ok_or(InvertError::PartialTree)?.split(path)?;
        self.entries.push(NodeEntry::Child { cid: None, child: Some(left_node), dirty: true });
        let left =
            Self { entries: self.entries, height: self.height, dirty: true, ..Default::default() };
        right_entries[0] = NodeEntry::Child { cid: None, child: Some(right_node), dirty: true };
        let right =
            Self { entries: right_entries, height: self.height, dirty: true, ..Default::default() };
        Ok((left, right))
    }

    fn split_entries(mut self, idx: usize) -> Result<(Self, Self), InvertError> {
        let len = self.entries.len();
        if idx == 0 || idx >= len {
            return Err(InvertError::SplittingEnds);
        }
        let right_entries = self.entries.split_off(idx);
        let left =
            Self { entries: self.entries, height: self.height, dirty: true, ..Default::default() };
        let right =
            Self { entries: right_entries, height: self.height, dirty: true, ..Default::default() };
        if left.entries.is_empty() || right.entries.is_empty() {
            return Err(InvertError::SplitEmptyLegs(idx, len));
        }
        Ok((left, right))
    }

    // Removes key/value from the sub-tree provided, returning a new tree, and the previous CID value.
    // If key is not found, returns unmodified subtree, and nil for the returned CID.
    fn remove(&mut self, path: &str, mut height: i8) -> Result<Option<Cid>, InvertError> {
        if self.stub {
            return Err(InvertError::PartialTree);
        }
        // TODO: do we need better handling of "is this the top"?
        let mut top = false;
        if height < 0 {
            top = true;
            height = NodeData::height_for_key(path.as_bytes());
        }

        if height > self.height {
            // removing a key from a higher layer; key was not in tree
            return Ok(None);
        }

        if height < self.height {
            // TODO: handle case of this returning an empty node at top of tree, with wrong height
            return self.remove_child(path, height);
        }

        // look at this level
        let Some(idx) = self.find_value(path) else {
            // key not found
            return Ok(None);
        };

        // found it! will remove from list
        self.dirty = true;
        let NodeEntry::Value { value: prev, .. } = self.entries[idx] else { unreachable!() };

        // check if we need to "merge" adjacent nodes
        if idx > 0
            && idx + 1 < self.entries.len()
            && self.entries[idx - 1].is_child()
            && self.entries[idx + 1].is_child()
        {
            #[expect(clippy::unwrap_used)]
            let right = self.entries.drain(idx..idx + 2).nth(1).unwrap();
            self.entries[idx - 1].child_mut()?.merge(right.child()?)?;
        } else {
            // simple removal
            self.entries.remove(idx);
        }

        // marks adjacent child nodes dirty to include as "proof"
        match self.prove_mutation(path) {
            Ok(()) | Err(InvertError::PartialTree) => {}
            Err(err) => return Err(err),
        }

        // check if top of node is now just a pointer
        if top {
            loop {
                if self.entries.len() != 1 || !self.entries[0].is_child() {
                    break;
                }
                let NodeEntry::Child { cid, child, .. } = self.entries.remove(0) else {
                    unreachable!()
                };
                *self = if let Some(child) = child {
                    child
                } else {
                    // this is something of a hack, for MST inversion which requires trimming the tree
                    if let Some(cid) = cid {
                        Self {
                            height: self.height - 1,
                            cid: Some(cid),
                            stub: true,
                            ..Default::default()
                        }
                    } else {
                        return Err(InvertError::PartialTree);
                    }
                };
            }
        }

        Ok(Some(prev))
    }

    // internal helper
    fn remove_child(&mut self, path: &str, height: i8) -> Result<Option<Cid>, InvertError> {
        // look for a child
        let Some(idx) = self.find_child(path) else {
            // no child pointer; key not in tree
            return Ok(None);
        };

        let NodeEntry::Child { child, .. } = &mut self.entries[idx] else { unreachable!() };
        let Some(child) = child else {
            // partial node, can't recurse
            return Err(InvertError::PartialTree);
        };
        let Some(prev) = child.remove(path, height)? else {
            // no-op
            return Ok(None);
        };

        self.dirty = true;
        // if the child node was updated, but still exists, just return
        if !child.entries.is_empty() {
            debug_assert!(child.dirty);
            return Ok(Some(prev));
        }

        // if new child was empty, remove it from entry list
        // note that *this* entry might now be empty
        self.entries.remove(idx);
        Ok(Some(prev))
    }

    fn merge(&mut self, other: Self) -> Result<(), InvertError> {
        let idx = self.entries.len();
        *self = Self {
            entries: mem::take(&mut self.entries),
            height: self.height,
            dirty: true,
            ..Default::default()
        };
        self.entries.extend(other.entries);
        if self.entries[idx - 1].is_child() && self.entries[idx].is_child() {
            // need to merge recursively
            let right = self.entries.remove(idx);
            self.entries[idx - 1].child_mut()?.merge(right.child()?)?;
        }
        Ok(())
    }

    // helper to mark nodes as "dirty" if they are needed to "prove" something about the key.
    // used to generate invertable operation diffs.
    fn prove_mutation(&mut self, path: &str) -> Result<(), InvertError> {
        for idx in 0..self.entries.len() {
            match &self.entries[idx] {
                NodeEntry::Value { key, .. } => {
                    if path.as_bytes() < key.as_slice() {
                        return Ok(());
                    }
                }
                NodeEntry::Child { .. } => {
                    // first, see if there is a next entry as a value which this key would be after
                    // if so we can skip checking this child
                    if idx + 1 < self.entries.len() {
                        if let NodeEntry::Value { key, .. } = &self.entries[idx + 1] {
                            if path.as_bytes() > key.as_slice() {
                                continue;
                            }
                        }
                    }
                    let NodeEntry::Child { child, .. } = &mut self.entries[idx] else {
                        unreachable!()
                    };
                    let Some(child) = child else {
                        return Err(InvertError::PartialTree);
                    };
                    match child.compare_key(path)? {
                        // key comes before this entire child sub-tree
                        Ordering::Less => {
                            return Ok(());
                        }
                        // key falls inside this child sub-tree
                        Ordering::Equal => {
                            return child.prove_mutation(path);
                        }
                        // key comes after this entire child sub-tree
                        Ordering::Greater => {}
                    }
                }
            }
        }
        Ok(())
    }

    // Compares a provided `key` against the overall range of keys represented by a `Node`.
    // This method will set the Dirty flag on this node, and any child nodes which were needed to
    // "prove" the key order.
    fn compare_key(&mut self, path: &str) -> Result<Ordering, InvertError> {
        if self.stub {
            return Err(InvertError::PartialTree);
        }
        if self.entries.is_empty() {
            // TODO: should we actually return 0 in this case?
            return Err(InvertError::EmptyTreeNode);
        }
        self.dirty = true;
        // check if lower than this entire node
        if let NodeEntry::Value { key, .. } = &self.entries[0] {
            if path.as_bytes() < key.as_slice() {
                return Ok(Ordering::Less);
            }
        }
        // check if higher than this entire node
        if let NodeEntry::Value { key, .. } = &self.entries[self.entries.len() - 1] {
            if path.as_bytes() > key.as_slice() {
                return Ok(Ordering::Greater);
            }
        }
        for idx in 0..self.entries.len() {
            match &self.entries[idx] {
                NodeEntry::Value { key, .. } => {
                    if path.as_bytes() < key.as_slice() {
                        // we don't need to recurse/iterate further
                        return Ok(Ordering::Equal);
                    }
                }
                NodeEntry::Child { .. } => {
                    // first, see if there is a next entry as a value which this key would be after
                    // if so we can skip checking this child
                    if idx + 1 < self.entries.len() {
                        if let NodeEntry::Value { key, .. } = &self.entries[idx + 1] {
                            if path.as_bytes() > key.as_slice() {
                                continue;
                            }
                        }
                    }
                    let NodeEntry::Child { child, .. } = &mut self.entries[idx] else {
                        unreachable!()
                    };
                    let Some(child) = child else {
                        return Err(InvertError::PartialTree);
                    };
                    let order = match child.compare_key(path)? {
                        // lower than entire node
                        Ordering::Less if idx == 0 => Ordering::Less,
                        // higher than entire node
                        Ordering::Greater if idx == self.entries.len() - 1 => Ordering::Greater,
                        _ => Ordering::Equal,
                    };
                    return Ok(order);
                }
            }
        }
        Ok(Ordering::Equal)
    }

    // Determines index where a new entry (child or value) would be inserted, relevant to the given key.
    // If the key would "split" an existing child entry, the index of that entry is returned, and a flag set.
    // If the entry would be appended, then the index returned will be one higher that the current largest index.
    fn find_insertion_index(&mut self, path: &str) -> Result<(usize, bool), InvertError> {
        if self.stub {
            return Err(InvertError::PartialTreeInsertionOrderError);
        }
        for idx in 0..self.entries.len() {
            match &self.entries[idx] {
                NodeEntry::Value { key, .. } => {
                    if path.as_bytes() < key.as_slice() {
                        return Ok((idx, false));
                    }
                }
                NodeEntry::Child { .. } => {
                    // first, see if there is a next entry as a value which this key would be after
                    // if so we can skip checking this child
                    if idx + 1 < self.entries.len() {
                        if let NodeEntry::Value { key, .. } = &self.entries[idx + 1] {
                            if path.as_bytes() > key.as_slice() {
                                continue;
                            }
                        }
                    }
                    let NodeEntry::Child { child, .. } = &mut self.entries[idx] else {
                        unreachable!()
                    };
                    let Some(child) = child else {
                        return Err(InvertError::PartialTreeInsertionOrderError);
                    };
                    match child.compare_key(path)? {
                        // key comes before this entire child sub-tree
                        Ordering::Less => {
                            return Ok((idx, false));
                        }
                        // key falls inside this child sub-tree
                        Ordering::Equal => {
                            return Ok((idx, true));
                        }
                        // key comes after this entire child sub-tree
                        Ordering::Greater => {}
                    }
                }
            }
        }

        // would need to be appended after
        Ok((self.entries.len(), false))
    }

    // Looks for a "value" entry in the node with the exact key.
    fn find_value(&self, path: &str) -> Option<usize> {
        for (i, entry) in self.entries.iter().enumerate() {
            match entry {
                // TODO: could skip early if e.Key is lower
                NodeEntry::Value { key, .. } => {
                    if path.as_bytes() == key {
                        return Some(i);
                    }
                }
                NodeEntry::Child { .. } => {}
            }
        }
        None
    }

    // Looks for a "child" entry which the key would live under.
    fn find_child(&self, path: &str) -> Option<usize> {
        let mut idx = None;
        for (i, entry) in self.entries.iter().enumerate() {
            match entry {
                NodeEntry::Value { key, .. } => {
                    if path.as_bytes() <= key.as_slice() {
                        break;
                    }
                    idx = None;
                }
                NodeEntry::Child { .. } => {
                    idx = Some(i);
                }
            }
        }
        idx
    }

    fn load(block_map: &BlockMap, cid: Cid) -> Result<Option<Self>, ParseError> {
        let Some(block) = block_map.get(&cid) else {
            // allow "partial" trees
            return Ok(None);
        };

        let nd: NodeData = serde_ipld_dagcbor::from_slice(block)?;
        let mut n = nd.into_node(cid);
        for entry in &mut n.entries {
            if let NodeEntry::Child { cid: Some(cid), child: node, .. } = entry {
                if let Some(child) = Self::load(block_map, *cid)? {
                    // NOTE: this is kind of a hack
                    if n.height < 0 && child.height >= 0 {
                        n.height = child.height + 1;
                    }
                    *node = Some(child);
                }
            }
        }
        Ok(Some(n))
    }

    // TODO: this feels like a hack, and easy to forget
    fn ensure_heights(&mut self) {
        if self.height <= 0 {
            return;
        }
        for entry in &mut self.entries {
            if let NodeEntry::Child { child: Some(child), .. } = entry {
                if self.height > 0 && child.height < 0 {
                    child.height = self.height - 1;
                }
                child.ensure_heights();
            }
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct EntryData {
    #[serde(rename = "p")]
    prefix_len: usize,
    #[serde(rename = "k", with = "ipld_bytes")]
    key_suffix: Vec<u8>,
    #[serde(rename = "v")]
    value: Cid,
    #[serde(rename = "t")]
    right: Option<Cid>,
}

#[derive(Debug, Serialize, Deserialize)]
struct NodeData {
    #[serde(rename = "l")]
    left: Option<Cid>,
    #[serde(rename = "e")]
    entries: Vec<EntryData>,
}

impl NodeData {
    // Transforms an encoded `NodeData` to `Node` data structure format
    fn into_node(self, cid: Cid) -> Node {
        let mut height = -1;
        let mut entries = Vec::with_capacity(self.entries.len());

        if let Some(cid) = self.left {
            entries.push(NodeEntry::Child { cid: Some(cid), child: None, dirty: false });
        }

        let mut prev: &[u8] = &[];
        for entry in self.entries {
            let mut key = Vec::with_capacity(entry.prefix_len + entry.key_suffix.len());
            key.extend_from_slice(&prev[..entry.prefix_len]);
            key.extend_from_slice(&entry.key_suffix);

            let idx = entries.len();
            entries.push(NodeEntry::Value { key, value: entry.value, dirty: false });
            if let Some(cid) = entry.right {
                entries.push(NodeEntry::Child { cid: Some(cid), child: None, dirty: false });
            }

            let key = {
                let NodeEntry::Value { key, .. } = &entries[idx] else { unreachable!() };
                key.as_slice()
            };
            prev = key;
            if height < 0 {
                height = Self::height_for_key(key);
            }
        }

        // TODO: height doesn't get set properly if this is an intermediate node
        // we rely on `ensure_heights` getting called to fix that
        Node { entries, height, dirty: false, cid: Some(cid), ..Default::default() }
    }

    fn from_node(node: &Node) -> Result<Self, InvertError> {
        let mut d = Self { left: None, entries: Vec::with_capacity(node.entries.len()) };

        let mut prev: &[u8] = &[];
        for (idx, entry) in node.entries.iter().enumerate() {
            match entry {
                NodeEntry::Value { key, value, .. } => {
                    let mut prefix_len = 0;
                    for (byte_a, byte_b) in key.iter().zip(prev.iter()) {
                        if byte_a != byte_b {
                            break;
                        }
                        prefix_len += 1;
                    }
                    d.entries.push(EntryData {
                        prefix_len,
                        key_suffix: key[prefix_len..].to_vec(),
                        value: *value,
                        right: None,
                    });
                    prev = key.as_slice();
                }
                NodeEntry::Child { cid, .. } => {
                    if idx == 0 {
                        d.left = *cid;
                        continue;
                    }
                    if d.entries.is_empty() {
                        return Err(InvertError::MalformedTreeNode);
                    }
                    let idx = d.entries.len() - 1;
                    d.entries[idx].right = *cid;
                }
            }
        }

        Ok(d)
    }

    // Computes the MST "height" for a key (bytestring).
    // Layers are counted from the "bottom" of the tree, starting with zero.
    //
    // For atproto repository v3, uses SHA-256 as the hashing function and
    // counts two bits at a time, for an MST "fanout" value of 16.
    fn height_for_key(key: &[u8]) -> i8 {
        let mut height = 0;
        let hash = Sha256::digest(key);

        for byte in hash {
            if byte & 0xc0 != 0 {
                // Common case. No leading pair of zero bits.
                break;
            }
            if byte == 0x00 {
                height += 4;
                continue;
            }
            if byte & 0xfc == 0x00 {
                height += 3;
            } else if byte & 0xf0 == 0x00 {
                height += 2;
            } else {
                height += 1;
            }
            break;
        }

        height
    }
}

mod ipld_bytes {
    use ipld_core::ipld::Ipld;
    use serde::de::Error;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[expect(clippy::ptr_arg)]
    pub fn serialize<S: Serializer>(t: &Vec<u8>, s: S) -> Result<S::Ok, S::Error> {
        Ipld::Bytes(t.clone()).serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let ipld = Ipld::deserialize(d)?;
        let Ipld::Bytes(key_suffix) = ipld else {
            return Err(D::Error::custom(format!("expected ipld bytes, got: {:?}", ipld.kind())));
        };
        Ok(key_suffix)
    }
}
