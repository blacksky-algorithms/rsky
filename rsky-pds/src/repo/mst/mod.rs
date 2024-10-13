/**
 * This is an implementation of a Merkle Search Tree (MST)
 * The data structure is described here: https://hal.inria.fr/hal-02303490/document
 * The MST is an ordered, insert-order-independent, deterministic tree.
 * Keys are laid out in alphabetic order.
 * The key insight of an MST is that each key is hashed and starting 0s are counted
 * to determine which layer it falls on (5 zeros for ~32 fanout).
 * This is a merkle tree, so each subtree is referred to by its hash (CID).
 * When a leaf is changed, every tree on the path to that leaf is changed as well,
 * thereby updating the root hash.
 *
 * For atproto, we use SHA-256 as the key hashing algorithm, and ~4 fanout
 * (2-bits of zero per layer).
 */
use crate::common;
use crate::common::ipld;
use crate::repo::block_map::BlockMap;
use crate::repo::cid_set::CidSet;
use crate::repo::error::DataStoreError;
use crate::repo::parse;
use crate::repo::types::{BlockWriter, CidAndBytes};
use crate::storage::{ObjAndBytes, SqlRepoReader};
use anyhow::{anyhow, bail, Result};
use lexicon_cid::Cid;
use serde_cbor::Value as CborValue;
use std::mem;

#[derive(Debug)]
pub struct NodeIter {
    entries: Vec<NodeEntry>, // Contains the remaining children of a node,
    // The iterator of the parent node, if present
    // It must be wrapped in a Box because a struct in Rust can’t contain itself
    // without indirection
    parent: Option<Box<NodeIter>>,
    this: Option<NodeEntry>,
}

impl Default for NodeIter {
    fn default() -> Self {
        NodeIter {
            entries: vec![],
            parent: None,
            this: None,
        }
    }
}

/// We want to traverse (i.e. iterate over) this kind of tree depth-first. This means that
/// when a node has multiple children, we first traverse the first child and all its descendants
/// before moving on to the second child.
impl Iterator for NodeIter {
    type Item = NodeEntry;

    fn next(&mut self) -> Option<Self::Item> {
        match self.entries.get(0) {
            // We first check if children is empty. If that’s the case, we try to continue
            // iterating the parent node. If there is no parent node, we return None.
            None => {
                match self.this {
                    Some(NodeEntry::MST(_)) => {
                        let this = self.this.clone().unwrap();
                        self.this = None;
                        Some(this)
                    }
                    _ => {
                        match self.parent.take() {
                            Some(parent) => {
                                // continue with the parent node
                                *self = *parent;
                                self.next()
                            }
                            None => None,
                        }
                    }
                }
            }
            // If children is not empty, we remove the first child and check its variant.
            // If it is a NodeEntry::Leaf, we return its content.
            Some(NodeEntry::Leaf(_)) => {
                let leaf = self.entries.get(0).unwrap().clone();
                self.entries = self.entries[1..].to_vec();
                Some(leaf)
            }
            // If it is a NodeEntry::MST, we create a new iterator for the child entries.
            // The parent field is set to self, and self is replaced with the newly created iterator
            Some(NodeEntry::MST(ref mst)) => {
                let mut subtree = mst.clone();
                let this = self.entries.get(0).unwrap().clone();
                self.entries = self.entries[1..].to_vec();

                // start iterating the child trees
                *self = NodeIter {
                    entries: subtree.get_entries().unwrap_or(vec![]),
                    parent: Some(Box::new(mem::take(self))),
                    this: Some(this),
                };
                self.next()
            }
        }
    }
}

/// Alternative implementation of iterator
#[derive(Debug)]
pub struct NodeIterReachable {
    entries: Vec<NodeEntry>,
    parent: Option<Box<NodeIterReachable>>,
    this: Option<NodeEntry>,
}

impl Default for NodeIterReachable {
    fn default() -> Self {
        NodeIterReachable {
            entries: vec![],
            parent: None,
            this: None,
        }
    }
}

impl Iterator for NodeIterReachable {
    type Item = Result<NodeEntry>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.entries.get(0) {
            None => match self.this {
                Some(NodeEntry::MST(ref t)) => {
                    let this = NodeEntry::MST(t.clone());
                    self.this = None;
                    Some(Ok(this))
                }
                _ => match self.parent.take() {
                    Some(parent) => {
                        *self = *parent;
                        self.next()
                    }
                    None => None,
                },
            },
            Some(NodeEntry::Leaf(_)) => {
                let leaf = self.entries.get(0).unwrap().clone();
                self.entries = self.entries[1..].to_vec();
                Some(Ok(leaf))
            }
            Some(NodeEntry::MST(_)) => {
                let this = self.entries.get(0).unwrap().clone();
                self.entries = self.entries[1..].to_vec();
                let entries = if let NodeEntry::MST(mut r) = this.clone() {
                    r.get_entries()
                } else {
                    Err(anyhow::Error::new(DataStoreError::MissingBlock(
                        "Missing Blocks".to_string(),
                    )))
                };
                match entries {
                    Err(e) => {
                        match e.downcast_ref() {
                            Some(DataStoreError::MissingBlock(_)) => self.next(), // Don't iterate
                            _ => return Some(Err(e)),
                        }
                    }
                    _ => {
                        *self = NodeIterReachable {
                            entries: entries.unwrap(),
                            parent: Some(Box::new(mem::take(self))),
                            this: Some(this.clone()),
                        };
                        self.next()
                    }
                }
            }
        }
    }
}

/**
 * A couple notes on CBOR encoding:
 *
 * There are never two neighboring subtrees.
 * Therefore, we can represent a node as an array of
 * leaves & pointers to their right neighbor (possibly null),
 * along with a pointer to the left-most subtree (also possibly null).
 *
 * Most keys in a subtree will have overlap.
 * We do compression on prefixes by describing keys as:
 * - the length of the prefix that it shares in common with the preceding key
 * - the rest of the string
 *
 * For example:
 * If the first leaf in a tree is `bsky/posts/abcdefg` and the second is `bsky/posts/abcdehi`
 * Then the first will be described as `prefix: 0, key: 'bsky/posts/abcdefg'`,
 * and the second will be described as `prefix: 16, key: 'hi'.`
 */
/// treeEntry are elements of nodeData's Entries.
#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
pub struct TreeEntry {
    pub p: u8, // count of characters shared with previous path/key in tree
    #[serde(with = "serde_bytes")]
    pub k: Vec<u8>, // remaining part of path/key (appended to "previous key")
    pub v: Cid, // CID pointer at this path/key
    pub t: Option<Cid>, // [optional] pointer to lower-level subtree to the "right" of this path/key entry
}

/// MST tree node as gets serialized to CBOR. Note that the CBOR fields are all
/// single-character.
#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
pub struct NodeData {
    pub l: Option<Cid>, // [optional] pointer to lower-level subtree to the "left" of this path/key
    pub e: Vec<TreeEntry>, // ordered list of entries at this node
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Leaf {
    pub key: String, // record key
    pub value: Cid,
}

impl PartialEq for Leaf {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key && self.value == other.value
    }
}

impl PartialEq<MST> for Leaf {
    fn eq(&self, _other: &MST) -> bool {
        false
    }
}

impl PartialEq<NodeEntry> for Leaf {
    fn eq(&self, other: &NodeEntry) -> bool {
        match other {
            NodeEntry::Leaf(other) => self.key == other.key && self.value == other.value,
            NodeEntry::MST(_) => false,
        }
    }
}

/// nodeEntry is a node in the MST.
///
/// Following the Typescript implementation, this is basically a flexible
/// "TreeEntry" (aka "leaf") which might also be the "Left" pointer on a
/// NodeData (aka "tree").
#[derive(Clone, Debug)]
pub enum NodeEntry {
    MST(MST),
    Leaf(Leaf),
}

impl NodeEntry {
    pub fn is_tree(&self) -> bool {
        match self {
            NodeEntry::MST(_) => true,
            _ => false,
        }
    }

    pub fn is_leaf(&self) -> bool {
        match self {
            NodeEntry::Leaf(_) => true,
            _ => false,
        }
    }

    fn iter(self) -> NodeIter {
        match self {
            NodeEntry::MST(_) => NodeIter {
                entries: vec![self.clone()],
                parent: None,
                this: Some(self),
            },
            NodeEntry::Leaf(_) => NodeIter {
                entries: vec![self],
                parent: None,
                this: None,
            },
        }
    }

    fn iter_reachable(&self) -> NodeIterReachable {
        match self {
            NodeEntry::MST(_) => NodeIterReachable {
                entries: vec![self.clone()],
                parent: None,
                this: Some(self.clone()),
            },
            NodeEntry::Leaf(_) => NodeIterReachable {
                entries: vec![self.clone()],
                parent: None,
                this: None,
            },
        }
    }
}

impl PartialEq for NodeEntry {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (NodeEntry::Leaf(this), NodeEntry::Leaf(other)) => {
                this.key == other.key && this.value == other.value
            }
            (NodeEntry::Leaf(_), NodeEntry::MST(_)) => false,
            (NodeEntry::MST(_), NodeEntry::Leaf(_)) => false,
            (NodeEntry::MST(this), NodeEntry::MST(other)) => {
                let this_pointer = this.clone().get_pointer().unwrap();
                let other_pointer = other.clone().get_pointer().unwrap();
                this_pointer == other_pointer
            }
        }
    }
}

impl PartialEq<MST> for NodeEntry {
    fn eq(&self, other: &MST) -> bool {
        match self {
            NodeEntry::Leaf(_) => false,
            NodeEntry::MST(this) => {
                let this_pointer = this.clone().get_pointer().unwrap();
                let other_pointer = other.clone().get_pointer().unwrap();
                this_pointer == other_pointer
            }
        }
    }
}

impl PartialEq<Leaf> for NodeEntry {
    fn eq(&self, other: &Leaf) -> bool {
        match self {
            NodeEntry::Leaf(this) => this.key == other.key && this.value == other.value,
            NodeEntry::MST(_) => false,
        }
    }
}

/*impl IntoIterator for NodeEntry {
    type Item = NodeEntry;

    type IntoIter = NodeIter;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}*/

#[derive(Debug)]
pub struct UnstoredBlocks {
    root: Cid,
    blocks: BlockMap,
}

/// MST represents a MerkleSearchTree tree node (NodeData type). It can be in
/// several levels of hydration: fully hydrated (entries and "pointer" (CID)
/// computed); dirty (entries correct, but pointer (CID) not valid); virtual
/// (pointer is defined, no entries have been pulled from block store)
///
/// MerkleSearchTree values are immutable. Methods return copies with changes.
#[derive(Clone, Debug)]
pub struct MST {
    pub entries: Option<Vec<NodeEntry>>,
    pub layer: Option<u32>,
    pub pointer: Cid,
    pub outdated_pointer: bool,
    pub storage: SqlRepoReader,
}

impl MST {
    pub fn new(
        storage: SqlRepoReader,
        pointer: Cid,
        entries: Option<Vec<NodeEntry>>,
        layer: Option<u32>,
    ) -> Self {
        MST {
            storage,
            entries,
            layer,
            pointer,
            outdated_pointer: false,
        }
    }

    pub fn create(
        storage: SqlRepoReader,
        entries: Option<Vec<NodeEntry>>,
        layer: Option<u32>,
    ) -> Result<MST> {
        let entries = entries.unwrap_or(Vec::new());
        let pointer = util::cid_for_entries(entries.clone())?;
        Ok(MST::new(storage, pointer, Some(entries), layer))
    }

    pub fn from_data(storage: SqlRepoReader, data: NodeData, layer: Option<u32>) -> Result<MST> {
        let entries = util::deserialize_node_data(&storage, data.clone(), layer)?;
        let pointer = ipld::cid_for_cbor(&data)?;
        Ok(MST::new(storage, pointer, Some(entries), layer))
    }

    /// This is poorly named in both implementations, because it is lazy
    /// this is really a *lazy* load, doesn't actually touch storage
    pub fn load(storage: SqlRepoReader, cid: Cid, layer: Option<u32>) -> Result<MST> {
        Ok(MST::new(storage, cid, None, layer))
    }

    // Immutability
    // -------------------

    /// We never mutate an MST, we just return a new MST with updated values
    pub fn new_tree(&mut self, entries: Vec<NodeEntry>) -> Result<MST> {
        let mut mst = MST::new(
            self.storage.clone(),
            self.get_pointer()?,
            Some(entries),
            self.layer,
        );
        mst.outdated_pointer = true;
        Ok(mst)
    }

    // === "Getters (lazy load)" ===

    /// "We don't want to load entries of every subtree, just the ones we need"
    pub fn get_entries(&mut self) -> Result<Vec<NodeEntry>> {
        // if we are "hydrated", entries are available
        if let Some(entries) = self.entries.clone() {
            return Ok(entries);
        };
        // otherwise this is a virtual/pointer struct, and we need to hydrate from
        // block store before returning entries
        let data: CborValue = self.storage.read_obj(&self.pointer, |obj: &CborValue| {
            match serde_cbor::value::from_value::<NodeData>(obj.clone()) {
                Ok(_) => true,
                Err(_) => false,
            }
        })?;
        let data: NodeData = serde_cbor::value::from_value(data)?;

        // can compute the layer on the first KeySuffix, because
        // for the first entry that field is a complete key
        let first_leaf = data.e.get(0);
        let layer: Option<u32> = match first_leaf {
            Some(first_leaf) => Some(util::leading_zeros_on_hash(&first_leaf.k)?),
            None => None,
        };

        self.entries = Some(util::deserialize_node_data(
            &self.storage,
            data.clone(),
            layer,
        )?);

        if let Some(entries) = self.entries.clone() {
            Ok(entries)
        } else {
            bail!("No entries")
        }
    }

    pub fn get_pointer(&mut self) -> Result<Cid> {
        if !self.outdated_pointer {
            return Ok(self.pointer);
        }
        let CidAndBytes { cid, .. } = self.serialize()?;
        self.pointer = cid;
        self.outdated_pointer = false;
        Ok(self.pointer)
    }

    pub fn serialize(&mut self) -> Result<CidAndBytes> {
        let mut entries = self.get_entries()?;
        let mut outdated: Vec<&mut MST> = entries
            .iter_mut()
            .filter_map(|e| match e {
                NodeEntry::MST(e) if e.outdated_pointer => Some(e),
                _ => None,
            })
            .collect::<Vec<_>>();

        if outdated.len() > 0 {
            let _ = outdated
                .iter_mut()
                .map(|e| e.get_pointer())
                .collect::<Result<Vec<Cid>>>()?;
            entries = self.get_entries()?;
        }
        let data = util::serialize_node_data(entries)?;
        Ok(CidAndBytes {
            cid: ipld::cid_for_cbor(&data)?,
            bytes: common::struct_to_cbor(data)?,
        })
    }

    /// In most cases, we get the layer of a node from a hint on creation
    /// In the case of the topmost node in the tree, we look for a key in the node & determine the layer
    /// In the case where we don't find one, we recurse down until we do.
    /// If we still can't find one, then we have an empty tree and the node is layer 0
    pub fn get_layer(&mut self) -> Result<u32> {
        self.layer = self.attempt_get_layer()?;
        if self.layer.is_none() {
            self.layer = Some(0);
        }
        Ok(self.layer.unwrap_or(0))
    }

    pub fn attempt_get_layer(&mut self) -> Result<Option<u32>> {
        if self.layer.is_some() {
            return Ok(self.layer);
        };
        let entries = self.get_entries()?;
        let mut layer = util::layer_for_entries(entries.clone())?;
        if layer.is_none() {
            for entry in entries {
                if let NodeEntry::MST(mut tree) = entry {
                    let child_layer = tree.attempt_get_layer()?;
                    if let Some(c) = child_layer {
                        layer = Some(c + 1);
                        break;
                    }
                }
            }
        } else {
            self.layer = layer;
        }
        Ok(layer)
    }

    // Core functionality
    // -------------------

    /// Return the necessary blocks to persist the MST to repo storage
    pub fn get_unstored_blocks(&mut self) -> Result<UnstoredBlocks> {
        let mut blocks = BlockMap::new();
        let pointer = self.get_pointer()?;
        let already_has = self.storage.has(pointer)?;
        if already_has {
            return Ok(UnstoredBlocks {
                root: pointer,
                blocks,
            });
        }
        let entries = self.get_entries()?;
        let data = util::serialize_node_data(entries.clone())?;
        let _ = blocks.add(data)?;
        for entry in entries {
            if let NodeEntry::MST(mut e) = entry {
                let subtree = e.get_unstored_blocks()?;
                blocks.add_map(subtree.blocks)?;
            }
        }
        Ok(UnstoredBlocks {
            root: pointer,
            blocks,
        })
    }

    /// Adds a new leaf for the given key/value pair
    /// Throws if a leaf with that key already exists
    pub fn add(&mut self, key: &String, value: Cid, known_zeros: Option<u32>) -> Result<MST> {
        util::ensure_valid_mst_key(&key)?;
        let key_zeros: u32;
        if let Some(z) = known_zeros {
            key_zeros = z;
        } else {
            key_zeros = util::leading_zeros_on_hash(&key.clone().into_bytes())?;
        }
        let layer = self.get_layer()?;

        let new_leaf = Leaf {
            key: key.clone(),
            value,
        };

        return if key_zeros == layer {
            // it belongs in this layer
            let index = self.find_gt_or_equal_leaf_index(&key)?;

            let found = self.at_index(index)?;
            if let Some(NodeEntry::Leaf(l)) = found {
                if l.key == *key {
                    return Err(anyhow!("There is already a value at key: {}", key));
                }
            }
            let prev_node = self.at_index(index - 1)?;
            if let Some(p) = prev_node {
                match p {
                    // if entry before is a leaf we can just splice in
                    NodeEntry::Leaf(_) => self.splice_in(NodeEntry::Leaf(new_leaf), index),
                    // else we try to split the subtree around the key
                    NodeEntry::MST(mut m) => {
                        let split_sub_tree = m.split_around(key)?;
                        self.replace_with_split(
                            index - 1,
                            split_sub_tree.0,
                            new_leaf,
                            split_sub_tree.1,
                        )
                    }
                }
            } else {
                // If we're on far left we can just splice in
                self.splice_in(NodeEntry::Leaf(new_leaf), index)
            }
        } else if key_zeros < layer {
            // it belongs on a lower layer
            let index = self.find_gt_or_equal_leaf_index(key)?;
            let prev_node = self.at_index(index - 1)?;
            if let Some(NodeEntry::MST(mut p)) = prev_node {
                // if entry before is a tree, we add it to that tree
                let new_subtree = p.add(key, value, Some(key_zeros))?;
                self.update_entry(index - 1, NodeEntry::MST(new_subtree))
            } else {
                let mut sub_tree = self.create_child()?;
                let new_subtree = sub_tree.add(key, value, Some(key_zeros))?;
                self.splice_in(NodeEntry::MST(new_subtree), index)
            }
        } else {
            let layer = self.get_layer()?;
            let extra_layers_to_add = key_zeros - layer;

            // it belongs on a higher layer & we must push the rest of the tree down
            let split = self.split_around(key)?;
            // if the newly added key has >=2 more leading zeros than the current highest layer
            // then we need to add in structural nodes in between as well
            let mut left: Option<MST> = split.0;
            let mut right: Option<MST> = split.1;
            // intentionally starting at 1, since first layer is taken care of by split
            for _ in 1..extra_layers_to_add {
                if let Some(l) = left.clone() {
                    left = Some(l.create_parent()?);
                }
                if let Some(r) = right.clone() {
                    right = Some(r.create_parent()?);
                }
            }
            let mut updated: Vec<NodeEntry> = Vec::new();
            if let Some(l) = left {
                updated.push(NodeEntry::MST(l.clone()));
            }
            updated.push(NodeEntry::Leaf(Leaf {
                key: key.clone(),
                value,
            }));
            if let Some(r) = right {
                updated.push(NodeEntry::MST(r.clone()));
            }
            let mut new_root = MST::create(self.storage.clone(), Some(updated), Some(key_zeros))?;
            new_root.outdated_pointer = true;
            Ok(new_root)
        };
    }

    /// Gets the value at the given key
    pub fn get(&mut self, key: &String) -> Result<Option<Cid>> {
        let index = self.find_gt_or_equal_leaf_index(key)?;
        let found = self.at_index(index)?;
        if let Some(NodeEntry::Leaf(f)) = found {
            if f.key == *key {
                return Ok(Some(f.value));
            }
        }
        let prev = self.at_index(index - 1)?;
        if let Some(NodeEntry::MST(mut p)) = prev {
            return Ok(p.get(key)?);
        }
        return Ok(None);
    }

    /// Edits the value at the given key
    /// Throws if the given key does not exist
    pub fn update(&mut self, key: &String, value: Cid) -> Result<MST> {
        util::ensure_valid_mst_key(key)?;
        let index = self.find_gt_or_equal_leaf_index(key)?;
        let found = self.at_index(index)?;
        if let Some(NodeEntry::Leaf(f)) = found {
            if f.key == *key {
                return self.update_entry(
                    index,
                    NodeEntry::Leaf(Leaf {
                        key: key.clone(),
                        value,
                    }),
                );
            }
        }
        let prev = self.at_index(index - 1)?;
        if let Some(NodeEntry::MST(mut p)) = prev {
            let updated_tree = p.update(key, value)?;
            return self.update_entry(index - 1, NodeEntry::MST(updated_tree.clone()));
        }
        Err(anyhow!("Could not find a record with key: {}", key))
    }

    /// Deletes the value at the given key
    pub fn delete(&mut self, key: &String) -> Result<MST> {
        let altered = self.delete_recurse(key)?;
        Ok(altered.clone().trim_top()?)
    }

    pub fn delete_recurse(&mut self, key: &String) -> Result<MST> {
        let index = self.find_gt_or_equal_leaf_index(key)?;
        let found = self.at_index(index)?;
        // if found, remove it on this level
        if let Some(NodeEntry::Leaf(f)) = found {
            if f.key == *key {
                let prev = self.at_index(index - 1)?;
                let next = self.at_index(index + 1)?;
                return match (prev, next) {
                    (Some(NodeEntry::MST(mut p)), Some(NodeEntry::MST(n))) => {
                        let merged = p.append_merge(n)?;
                        let mut new_tree_entries: Vec<NodeEntry> = Vec::new();
                        new_tree_entries.append(&mut self.slice(Some(0), Some(index - 1))?);
                        new_tree_entries.push(NodeEntry::MST(merged));
                        new_tree_entries.append(&mut self.slice(Some(index + 2), None)?);
                        self.new_tree(new_tree_entries)
                    }
                    (_, _) => self.remove_entry(index),
                };
            }
        }
        // else recurse down to find it
        let prev = self.at_index(index - 1)?;
        return if let Some(NodeEntry::MST(mut p)) = prev {
            let subtree = &mut p.delete_recurse(key)?;
            let subtree_entries = subtree.get_entries()?;
            if subtree_entries.len() == 0 {
                self.remove_entry(index - 1)
            } else {
                self.update_entry(index - 1, NodeEntry::MST(subtree.clone()))
            }
        } else {
            Err(anyhow!("Could not find a record with key: {}", key))
        };
    }

    // Simple Operations
    // -------------------

    /// update entry in place
    pub fn update_entry(&mut self, index: isize, entry: NodeEntry) -> Result<MST> {
        let mut update = Vec::new();
        for e in self.slice(Some(0), Some(index))? {
            update.push(e);
        }
        update.push(entry);
        for e in self.slice(Some(index + 1), None)? {
            update.push(e.clone());
        }
        Ok(self.new_tree(update)?)
    }

    /// remove entry at index
    pub fn remove_entry(&mut self, index: isize) -> Result<MST> {
        let mut updated = Vec::new();
        updated.append(&mut self.slice(Some(0), Some(index))?);
        updated.append(&mut self.slice(Some(index + 1), None)?);

        Ok(self.new_tree(updated)?)
    }

    /// append entry to end of the node
    pub fn append(&mut self, entry: NodeEntry) -> Result<MST> {
        let mut entries = self.get_entries()?;
        entries.push(entry);
        Ok(self.new_tree(entries)?)
    }

    /// prepend entry to end of the node
    pub fn prepend(&mut self, entry: NodeEntry) -> Result<MST> {
        let mut entries = self.get_entries()?;
        entries.splice(0..0, vec![entry]);
        Ok(self.new_tree(entries)?)
    }

    /// returns entry at index
    pub fn at_index(&mut self, index: isize) -> Result<Option<NodeEntry>> {
        let entries = self.get_entries()?;
        if index >= 0 {
            Ok(entries.into_iter().nth(index as usize))
        } else {
            Ok(None)
        }
    }

    /// returns a slice of the node
    pub fn slice(&mut self, start: Option<isize>, end: Option<isize>) -> Result<Vec<NodeEntry>> {
        let entries = self.get_entries()?;
        let entry_len = entries.len() as isize;
        match (start, end) {
            (Some(start), Some(end)) => {
                // Adapted from Javascript Array.prototype.slice()
                // https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Array/slice
                let start: usize = if start < 0 && start >= (-1 * entry_len) {
                    (start + entry_len) as usize
                } else if start < (-1 * entry_len) {
                    0
                } else if start >= entry_len {
                    return Ok(vec![]);
                } else {
                    start as usize
                };

                let end: usize = if end < 0 && end >= (-1 * entry_len) {
                    (end + entry_len) as usize
                } else if end < (-1 * entry_len) {
                    0
                } else if end >= entry_len {
                    entries.len()
                } else if end <= start as isize {
                    return Ok(vec![]);
                } else {
                    end as usize
                };

                Ok(entries[start..end].to_vec())
            }
            (Some(start), None) => {
                let start: usize = if start < 0 && start >= (-1 * entry_len) {
                    (start + entry_len) as usize
                } else if start < (-1 * entry_len) {
                    0
                } else if start >= entry_len {
                    return Ok(vec![]);
                } else {
                    start as usize
                };
                Ok(entries[start..].to_vec())
            }
            (None, Some(end)) => {
                let end: usize = if end < 0 && end >= (-1 * entry_len) {
                    (end + entry_len) as usize
                } else if end < (-1 * entry_len) {
                    0
                } else if end >= entry_len {
                    entries.len()
                } else if end <= 0 {
                    return Ok(vec![]);
                } else {
                    end as usize
                };
                Ok(entries[..end].to_vec())
            }
            (None, None) => Ok(entries),
        }
    }

    /// inserts entry at index
    pub fn splice_in(&mut self, entry: NodeEntry, index: isize) -> Result<MST> {
        let mut update = Vec::new();
        for e in self.slice(Some(0), Some(index))? {
            update.push(e);
        }
        update.push(entry);
        for e in self.slice(Some(index), None)? {
            update.push(e);
        }
        self.new_tree(update)
    }

    /// replaces an entry with [ Some(tree), Leaf, Some(tree) ]
    pub fn replace_with_split(
        &mut self,
        index: isize,
        left: Option<MST>,
        leaf: Leaf,
        right: Option<MST>,
    ) -> Result<MST> {
        let update = self.slice(Some(0), Some(index))?;
        let mut update = update.to_vec();
        if let Some(l) = left {
            update.push(NodeEntry::MST(l.clone()));
        }
        update.push(NodeEntry::Leaf(leaf));
        if let Some(r) = right {
            update.push(NodeEntry::MST(r.clone()));
        }
        let remainder = self.slice(Some(index + 1), None)?;
        let remainder = &mut remainder.to_vec();
        update.append(remainder);
        self.new_tree(update)
    }

    /// if the topmost node in the tree only points to another tree, trim the top and return the subtree
    pub fn trim_top(mut self) -> Result<MST> {
        let entries = self.get_entries()?;
        return if entries.len() == 1 {
            match entries.into_iter().nth(0) {
                Some(NodeEntry::MST(n)) => Ok(n.trim_top()?),
                _ => Ok(self.clone()),
            }
        } else {
            Ok(self.clone())
        };
    }

    // Subtree & Splits
    // -------------------

    /// Recursively splits a subtree around a given key
    pub fn split_around(&mut self, key: &String) -> Result<(Option<MST>, Option<MST>)> {
        let index = self.find_gt_or_equal_leaf_index(key)?;
        // split tree around key
        let left_data = self.slice(Some(0), Some(index))?;
        let right_data = self.slice(Some(index), None)?;
        let mut left = self.new_tree(left_data.clone())?;
        let mut right = self.new_tree(right_data)?;

        // if the far right of the left side is a subtree,
        // we need to split it on the key as well
        let left_len = left_data.len();
        let last_in_left: Option<NodeEntry> = if let [.., last] = left_data.as_slice() {
            Some(last.clone())
        } else {
            None
        };
        if let Some(NodeEntry::MST(mut last)) = last_in_left {
            left = left.remove_entry(left_len as isize - 1)?;
            let split = last.split_around(key)?;
            if let Some(s0) = split.0 {
                left = left.append(NodeEntry::MST(s0.clone()))?;
            }
            if let Some(s1) = split.1 {
                right = right.prepend(NodeEntry::MST(s1.clone()))?;
            }
        }

        let left_output: Option<MST>;
        match left.get_entries()?.len() {
            0 => left_output = None,
            _ => left_output = Some(left),
        };
        let right_output: Option<MST>;
        match right.get_entries()?.len() {
            0 => right_output = None,
            _ => right_output = Some(right),
        };
        Ok((left_output, right_output))
    }

    /// The simple merge case where every key in the right tree is greater than every key in the left tree
    /// (used primarily for deletes)
    pub fn append_merge(&mut self, mut to_merge: MST) -> Result<MST> {
        if self.get_layer()? != to_merge.get_layer()? {
            return Err(anyhow!(
                "Trying to merge two nodes from different layers of the MST"
            ));
        }
        let mut self_entries = self.get_entries()?;
        let mut to_merge_entries = to_merge.get_entries()?;
        let last_in_left = self_entries.last();
        let first_in_right = to_merge_entries.first();
        let mut new_tree_entries: Vec<NodeEntry> = Vec::new();
        return match (last_in_left, first_in_right) {
            (Some(NodeEntry::MST(l)), Some(NodeEntry::MST(r))) => {
                let mut new_l = l.clone();
                let merged = new_l.append_merge(r.clone())?;
                new_tree_entries.append(&mut self_entries[0..self_entries.len() - 1].to_vec());
                new_tree_entries.push(NodeEntry::MST(merged));
                new_tree_entries.append(&mut to_merge_entries[1..].to_vec());
                self.new_tree(new_tree_entries)
            }
            (_, _) => {
                new_tree_entries.append(&mut self_entries);
                new_tree_entries.append(&mut to_merge_entries);
                self.new_tree(new_tree_entries)
            }
        };
    }

    // Create relatives
    // -------------------

    pub fn create_child(&mut self) -> Result<MST> {
        let layer = self.get_layer()?;
        MST::create(self.storage.clone(), Some(Vec::new()), Some(layer - 1))
    }

    pub fn create_parent(mut self) -> Result<Self> {
        let layer = self.get_layer()?;
        let mut parent = MST::create(
            self.storage.clone(),
            Some(vec![NodeEntry::MST(self.clone())]),
            Some(layer + 1),
        )?;
        parent.outdated_pointer = true;
        Ok(parent)
    }

    // Finding insertion points
    // -------------------

    /// finds index of first leaf node that is greater than or equal to the value
    pub fn find_gt_or_equal_leaf_index(&mut self, key: &String) -> Result<isize> {
        let entries = self.get_entries()?;
        let maybe_index = entries.iter().position(|entry| match entry {
            NodeEntry::MST(_) => false,
            NodeEntry::Leaf(entry) => entry.key >= *key,
        });
        // if we can't find, we're on the end
        if let Some(i) = maybe_index {
            Ok(i as isize)
        } else {
            Ok(entries.len() as isize)
        }
    }

    // List operations (partial tree traversal)
    // -------------------

    /// Walk tree starting at key
    /// @Rudy Note: This may be suboptimal since we always traverse the tree even though external
    /// controls might stop earlier.
    pub fn walk_leaves_from(&mut self, key: &String) -> impl Iterator<Item = Leaf> {
        let mut iter: Vec<Leaf> = Vec::new();
        let index = self.find_gt_or_equal_leaf_index(key).unwrap() as usize;
        let entries = self.get_entries().unwrap();
        let prev = entries.get(index - 1).unwrap().clone();
        if let NodeEntry::MST(mut p) = prev {
            for leaf in p.walk_leaves_from(key) {
                iter.push(leaf);
            }
        }
        for i in index..entries.len() {
            let entry = entries[i].clone();
            match entry {
                NodeEntry::Leaf(e) => iter.push(e),
                NodeEntry::MST(mut e) => {
                    for leaf in e.walk_leaves_from(key) {
                        iter.push(leaf);
                    }
                }
            }
        }
        iter.into_iter()
    }

    pub fn list(
        &mut self,
        count: Option<usize>,
        after: Option<String>,
        before: Option<String>,
    ) -> Result<Vec<Leaf>> {
        let mut vals: Vec<Leaf> = Vec::new();
        let after = after.unwrap_or("".to_owned());
        for leaf in self.walk_leaves_from(&after) {
            if leaf.key == after {
                continue;
            }
            if vals.len() >= count.unwrap_or(usize::MAX) {
                break;
            }
            if let Some(b) = &before {
                if leaf.key >= *b {
                    break;
                }
            }
            vals.push(leaf);
        }
        Ok(vals)
    }

    pub fn list_with_prefix(&mut self, prefix: &String, count: usize) -> Result<Vec<Leaf>> {
        let mut vals: Vec<Leaf> = Vec::new();
        for leaf in self.walk_leaves_from(prefix) {
            if vals.len() >= count || !leaf.key.starts_with(prefix) {
                break;
            }
            vals.push(leaf);
        }
        Ok(vals)
    }

    // Full tree traversal
    // -------------------

    /// Walk full tree & emit nodes, consumer can bail at any point by returning None
    pub fn walk(self) -> NodeIter {
        NodeEntry::MST(self).iter()
    }

    /// Walk full tree & emit nodes, consumer can bail at any point by returning None
    pub fn paths(self) -> Result<Vec<Vec<NodeEntry>>> {
        let mut paths: Vec<Vec<NodeEntry>> = Vec::new();
        for entry in self.walk() {
            match entry {
                NodeEntry::Leaf(_) => paths.push(vec![entry]),
                NodeEntry::MST(ref m) => {
                    let sub_paths = m.clone().paths()?;
                    sub_paths
                        .clone()
                        .into_iter()
                        .map(|mut p| {
                            let mut path: Vec<NodeEntry> = vec![entry.clone()];
                            path.append(&mut p);
                            paths.push(path)
                        })
                        .for_each(drop);
                }
            }
        }
        Ok(paths)
    }

    /// Walks tree & returns all nodes
    pub fn all_nodes(self) -> Result<Vec<NodeEntry>> {
        let mut nodes: Vec<NodeEntry> = Vec::new();
        for entry in self.walk() {
            match entry {
                NodeEntry::Leaf(_) => nodes.push(entry),
                NodeEntry::MST(mut m) => {
                    if m.outdated_pointer {
                        m.pointer = m.get_pointer()?;
                    }
                    nodes.push(NodeEntry::MST(m))
                }
            }
        }
        Ok(nodes)
    }

    /// Walks tree & returns all cids
    pub fn all_cids(self) -> Result<CidSet> {
        let mut cids = CidSet::new(None);
        for entry in self.clone().walk() {
            match entry {
                NodeEntry::Leaf(leaf) => cids.add(leaf.value),
                NodeEntry::MST(m) => {
                    let subtree_cids = m.all_cids()?;
                    let _ = &cids.add_set(subtree_cids);
                }
            }
        }
        cids.add(self.clone().get_pointer()?);
        Ok(cids)
    }

    /// Walks tree & returns all leaves
    pub fn leaves(self) -> Result<Vec<Leaf>> {
        let mut leaves: Vec<Leaf> = Vec::new();
        for entry in self.walk() {
            if let NodeEntry::Leaf(leaf) = entry {
                leaves.push(leaf);
            }
        }
        Ok(leaves)
    }

    /// Returns total leaf count
    pub fn leaf_count(self) -> Result<usize> {
        let leaves = self.leaves()?;
        Ok(leaves.len())
    }

    // Reachable tree traversal
    // -------------------

    /// Walk reachable branches of tree & emit nodes, consumer can bail at any point
    /// by returning false
    pub fn walk_reachable(self) -> NodeIterReachable {
        NodeEntry::MST(self).iter_reachable()
    }

    pub fn reachable_leaves(self) -> Result<Vec<Leaf>> {
        let mut leaves: Vec<Leaf> = Vec::new();
        for entry in self.walk_reachable() {
            if let Ok(NodeEntry::Leaf(leaf)) = entry {
                leaves.push(leaf);
            }
        }
        Ok(leaves)
    }

    /// Sync Protocol
    /// @TODO: This needs to implement an actual CarWriter
    pub async fn write_to_car_stream(&mut self, car: &mut BlockWriter) -> Result<()> {
        let mut leaves = CidSet::new(None);
        let mut to_fetch = CidSet::new(None);
        to_fetch.add(self.get_pointer()?);
        while to_fetch.size() > 0 {
            let mut next_layer = CidSet::new(None);
            let fetched = self.storage.get_blocks(to_fetch.to_list()).await?;
            if fetched.missing.len() > 0 {
                return Err(anyhow::Error::new(DataStoreError::MissingBlocks(
                    "mst node".to_owned(),
                    fetched.missing,
                )));
            }
            for cid in to_fetch.to_list() {
                let found: ObjAndBytes =
                    parse::get_and_parse_by_kind(&fetched.blocks, cid, |obj: &CborValue| {
                        match serde_cbor::value::from_value::<NodeData>(obj.clone()) {
                            Ok(_) => true,
                            Err(_) => false,
                        }
                    })?;
                car.push(CidAndBytes {
                    cid,
                    bytes: found.bytes,
                });
                let node_data: NodeData = serde_cbor::value::from_value(found.obj)?;
                let entries = util::deserialize_node_data(&self.storage, node_data.clone(), None)?;

                for entry in entries {
                    match entry {
                        NodeEntry::Leaf(l) => leaves.add(l.value),
                        NodeEntry::MST(mut m) => next_layer.add(m.get_pointer()?),
                    }
                }
            }
            to_fetch = next_layer;
        }
        let leaf_data = self.storage.get_blocks(leaves.to_list()).await?;
        if leaf_data.missing.len() > 0 {
            return Err(anyhow::Error::new(DataStoreError::MissingBlocks(
                "mst leaf".to_owned(),
                leaf_data.missing,
            )));
        }
        for leaf in leaf_data.blocks.entries()? {
            car.push(leaf);
        }
        Ok(())
    }

    pub fn cids_for_path(&mut self, key: String) -> Result<Vec<Cid>> {
        let mut cids: Vec<Cid> = vec![self.get_pointer()?];
        let index = self.find_gt_or_equal_leaf_index(&key)?;
        let found = self.at_index(index)?;
        if let Some(NodeEntry::Leaf(l)) = found {
            if l.key == *key {
                cids.push(l.value);
                return Ok(cids);
            }
        }
        let prev = self.at_index(index - 1)?;
        if let Some(NodeEntry::MST(mut p)) = prev {
            cids.append(&mut p.cids_for_path(key)?);
            return Ok(cids);
        }
        Ok(cids)
    }
}

impl PartialEq for MST {
    fn eq(&self, other: &Self) -> bool {
        let this_pointer = self.clone().get_pointer().unwrap();
        let other_pointer = other.clone().get_pointer().unwrap();
        this_pointer == other_pointer
    }
}

impl PartialEq<Leaf> for MST {
    fn eq(&self, _other: &Leaf) -> bool {
        false
    }
}

impl PartialEq<NodeEntry> for MST {
    fn eq(&self, other: &NodeEntry) -> bool {
        match other {
            NodeEntry::Leaf(_) => false,
            NodeEntry::MST(other) => {
                let this_pointer = self.clone().get_pointer().unwrap();
                let other_pointer = other.clone().get_pointer().unwrap();
                this_pointer == other_pointer
            }
        }
    }
}

pub mod diff;
pub mod util;
pub mod walker;

#[cfg(test)]
mod tests {
    use super::util::*;
    use super::*;
    use anyhow::Result;
    use rand::seq::SliceRandom;
    use rand::thread_rng;

    fn string_to_vec_u8(input: &str) -> Vec<u8> {
        input.as_bytes().to_vec()
    }

    #[test]
    fn adds_records() -> Result<()> {
        let mut storage =
            SqlRepoReader::new(None, "did:example:123456789abcdefghi".to_string(), None);
        let mapping = generate_bulk_data_keys(254, Some(&mut storage))?;
        let mut mst = MST::create(storage, None, None)?;
        let mut rng = thread_rng();

        let mut entries = mapping
            .iter()
            .map(|e| (e.0.clone(), e.1.clone()))
            .collect::<Vec<(String, Cid)>>();
        entries.shuffle(&mut rng);

        for entry in &entries {
            //let start = std::time::Instant::now();
            mst = mst.add(&entry.0, entry.1, None)?;
            //let duration = start.elapsed();
            //println!("Time:{:?}, Key:{}, Cid:{:?}", duration, &entry.0, entry.1);
        }
        for entry in entries {
            let got = mst.get(&entry.0)?;
            assert_eq!(Some(entry.1), got);
        }
        let total_size = mst.leaf_count()?;
        assert_eq!(total_size, 254);

        Ok(())
    }

    #[test]
    fn edits_records() -> Result<()> {
        let mut storage =
            SqlRepoReader::new(None, "did:example:123456789abcdefghi".to_string(), None);
        let mapping = generate_bulk_data_keys(100, Some(&mut storage))?;
        let mut mst = MST::create(storage, None, None)?;
        let mut rng = thread_rng();

        let mut entries = mapping
            .iter()
            .map(|e| (e.0.clone(), e.1.clone()))
            .collect::<Vec<(String, Cid)>>();
        entries.shuffle(&mut rng);

        for entry in &entries {
            mst = mst.add(&entry.0, entry.1, None)?;
        }

        let mut edited: Vec<(String, Cid)> = Vec::new();
        for entry in &entries {
            let new_cid = random_cid(&mut None)?;
            mst = mst.update(&entry.0, new_cid)?;
            edited.push((entry.0.clone(), new_cid));
        }
        for entry in edited {
            let got = mst.get(&entry.0)?;
            assert_eq!(Some(entry.1), got);
        }
        let total_size = mst.leaf_count()?;
        assert_eq!(total_size, 100);

        Ok(())
    }

    #[test]
    fn deletes_records() -> Result<()> {
        let mut storage =
            SqlRepoReader::new(None, "did:example:123456789abcdefghi".to_string(), None);
        let mapping = generate_bulk_data_keys(254, Some(&mut storage))?;
        let mut mst = MST::create(storage, None, None)?;
        let mut rng = thread_rng();

        let mut entries = mapping
            .iter()
            .map(|e| (e.0.clone(), e.1.clone()))
            .collect::<Vec<(String, Cid)>>();
        entries.shuffle(&mut rng);

        for entry in &entries {
            mst = mst.add(&entry.0, entry.1, None)?;
        }

        let to_delete = &entries[0..100];
        let the_rest = &entries[100..entries.len()];

        for entry in to_delete {
            mst = mst.delete(&entry.0)?;
        }

        let total_size = mst.clone().leaf_count()?;
        assert_eq!(total_size, 154);

        for entry in to_delete {
            let got = mst.get(&entry.0)?;
            assert_eq!(None, got);
        }

        for entry in the_rest {
            let got = mst.get(&entry.0)?;
            assert_eq!(Some(entry.1), got);
        }

        Ok(())
    }

    #[test]
    fn is_order_independent() -> Result<()> {
        let mut storage =
            SqlRepoReader::new(None, "did:example:123456789abcdefghi".to_string(), None);
        let mapping = generate_bulk_data_keys(254, Some(&mut storage))?;
        let mut mst = MST::create(storage, None, None)?;
        let mut rng = thread_rng();

        let mut entries = mapping
            .iter()
            .map(|e| (e.0.clone(), e.1.clone()))
            .collect::<Vec<(String, Cid)>>();
        entries.shuffle(&mut rng);

        for entry in &entries {
            mst = mst.add(&entry.0, entry.1, None)?;
        }

        let mut recreated = MST::create(mst.storage.clone(), None, None)?;
        let all_nodes = mst.all_nodes()?;

        let mut reshuffled = mapping
            .iter()
            .map(|e| (e.0.clone(), e.1.clone()))
            .collect::<Vec<(String, Cid)>>();
        reshuffled.shuffle(&mut rng);

        for entry in &reshuffled {
            recreated = recreated.add(&entry.0, entry.1, None)?;
        }
        let all_reshuffled = recreated.all_nodes()?;
        assert_eq!(all_nodes.len(), all_reshuffled.len());
        assert_eq!(all_nodes, all_reshuffled);

        Ok(())
    }

    #[test]
    fn saves_and_loads_from_blockstore() -> Result<()> {
        let mut storage =
            SqlRepoReader::new(None, "did:example:123456789abcdefghi".to_string(), None);
        let mapping = generate_bulk_data_keys(50, Some(&mut storage))?;
        let mut mst = MST::create(storage, None, None)?;

        let mst_storage = mst.storage.clone();
        let root = futures::executor::block_on(save_mst(&mst_storage, &mut mst))?;
        let loaded = MST::load(mst_storage, root, None)?;
        let original_nodes = mst.all_nodes()?;
        let loaded_nodes = loaded.all_nodes()?;

        assert_eq!(original_nodes.len(), loaded_nodes.len());
        assert_eq!(original_nodes, loaded_nodes);

        Ok(())
    }

    #[test]
    fn test_leading_zeros() -> Result<()> {
        let msg = "MST 'depth' computation (SHA-256 leading zeros)";

        // Helper macro to handle the Result in the test assertions
        macro_rules! assert_leading_zeros {
            ($input:expr, $expected:expr) => {
                assert_eq!(
                    leading_zeros_on_hash(&string_to_vec_u8($input))?,
                    $expected,
                    "{}",
                    msg
                );
            };
        }

        // Test cases
        assert_leading_zeros!("", 0);
        assert_leading_zeros!("asdf", 0);
        assert_leading_zeros!("blue", 1);
        assert_leading_zeros!("2653ae71", 0);
        assert_leading_zeros!("88bfafc7", 2);
        assert_leading_zeros!("2a92d355", 4);
        assert_leading_zeros!("884976f5", 6);
        assert_leading_zeros!("app.bsky.feed.post/454397e440ec", 4);
        assert_leading_zeros!("app.bsky.feed.post/9adeb165882c", 8);

        Ok(())
    }

    #[test]
    fn test_prefix_len() -> Result<()> {
        let msg = "length of common prefix between strings";

        // Helper macro to handle assertions
        macro_rules! assert_prefix_len {
            ($a:expr, $b:expr, $expected:expr) => {
                assert_eq!(
                    count_prefix_len($a.to_string(), $b.to_string())?,
                    $expected,
                    "{}",
                    msg
                );
            };
        }

        // Test cases
        assert_prefix_len!("abc", "abc", 3);
        assert_prefix_len!("", "abc", 0);
        assert_prefix_len!("abc", "", 0);
        assert_prefix_len!("ab", "abc", 2);
        assert_prefix_len!("abc", "ab", 2);
        assert_prefix_len!("abcde", "abc", 3);
        assert_prefix_len!("abc", "abcde", 3);
        assert_prefix_len!("abcde", "abc1", 3);
        assert_prefix_len!("abcde", "abb", 2);
        assert_prefix_len!("abcde", "qbb", 0);
        assert_prefix_len!("abc", "abc\x00", 3);
        assert_prefix_len!("abc\x00", "abc", 3);

        Ok(())
    }

    #[test]
    fn test_prefix_len_wide() -> Result<()> {
        let msg = "length of common prefix between strings (wide chars)";

        // Testing string lengths (Note: length in bytes, not characters)
        assert_eq!("jalapeño".len(), 9, "{}", msg); // 9 bytes in Rust, same as Go
        assert_eq!("💩".len(), 4, "{}", msg); // 4 bytes in Rust, same as Go
        assert_eq!("👩‍👧‍👧".len(), 18, "{}", msg); // 18 bytes in Rust, same as Go

        // Helper macro to handle assertions for count_prefix_len
        macro_rules! assert_prefix_len {
            ($a:expr, $b:expr, $expected:expr) => {
                assert_eq!(
                    count_prefix_len($a.to_string(), $b.to_string())?,
                    $expected,
                    "{}",
                    msg
                );
            };
        }

        // many of the below are different in Go because we count chars not bytes
        assert_prefix_len!("jalapeño", "jalapeno", 6);
        assert_prefix_len!("jalapeñoA", "jalapeñoB", 8);
        assert_prefix_len!("coöperative", "coüperative", 2);
        assert_prefix_len!("abc💩abc", "abcabc", 3);
        assert_prefix_len!("💩abc", "💩ab", 3);
        assert_prefix_len!("abc👩‍👦‍👦de", "abc👩‍👧‍👧de", 5);

        Ok(())
    }

    #[test]
    fn test_allowed_keys() -> Result<()> {
        let cid1str = "bafyreie5cvv4h45feadgeuwhbcutmh6t2ceseocckahdoe6uat64zmz454";
        let cid1 = Cid::try_from(cid1str)?;

        let storage = SqlRepoReader::new(None, "did:example:123456789abcdefghi".to_string(), None);
        let mut mst = MST::create(storage, None, None)?;
        // Rejects empty key
        let result = mst.add(&"".to_string(), cid1, None);
        assert!(result.is_err());

        // Rejects a key with no collection
        let result = mst.add(&"asdf".to_string(), cid1, None);
        assert!(result.is_err());

        // Rejects a key with a nested collection
        let result = mst.add(&"nested/collection/asdf".to_string(), cid1, None);
        assert!(result.is_err());

        // Rejects on empty coll or rkey
        let result = mst.add(&"coll/".to_string(), cid1, None);
        assert!(result.is_err());
        let result = mst.add(&"/rkey".to_string(), cid1, None);
        assert!(result.is_err());

        // Rejects non-ascii chars
        let result = mst.add(&"coll/jalapeñoA".to_string(), cid1, None);
        assert!(result.is_err());
        let result = mst.add(&"coll/coöperative".to_string(), cid1, None);
        assert!(result.is_err());
        let result = mst.add(&"coll/abc💩".to_string(), cid1, None);
        assert!(result.is_err());

        // Rejects ascii that we don't support
        let invalid_chars = vec!["$", "%", "(", ")", "+", "="];
        for ch in invalid_chars {
            let key = format!("coll/key{}", ch);
            let result = mst.add(&key, cid1, None);
            assert!(result.is_err(), "Key '{}' should be invalid", key);
        }

        // Rejects keys over 256 chars
        let long_key: String = "a".repeat(253);
        let key = format!("coll/{}", long_key);
        let result = mst.add(&key, cid1, None);
        assert!(result.is_err());

        // Allows long key under 256 chars
        let long_key: String = "a".repeat(250);
        let key = format!("coll/{}", long_key);
        let result = mst.add(&key, cid1, None);
        assert!(result.is_ok());

        // Allows URL-safe chars
        let valid_keys = vec![
            "coll/key0",
            "coll/key_",
            "coll/key:",
            "coll/key.",
            "coll/key-",
        ];
        for key in valid_keys {
            let result = mst.add(&key.to_string(), cid1, None);
            assert!(result.is_ok(), "Key '{}' should be valid", key);
        }

        Ok(())
    }

    // MST Interop Known Maps

    /// computes "empty" tree root CID
    #[test]
    fn empty_tree_root() -> Result<()> {
        let storage = SqlRepoReader::new(None, "did:example:123456789abcdefghi".to_string(), None);
        let mut mst = MST::create(storage, None, None)?;

        assert_eq!(mst.clone().leaf_count()?, 0);
        assert_eq!(
            mst.get_pointer()?.to_string(),
            "bafyreie5737gdxlw5i64vzichcalba3z2v5n6icifvx5xytvske7mr3hpm"
        );

        Ok(())
    }

    /// computes "trivial" tree root CID
    #[test]
    fn trivial_tree() -> Result<()> {
        let cid1 = Cid::try_from("bafyreie5cvv4h45feadgeuwhbcutmh6t2ceseocckahdoe6uat64zmz454")?; //dag-pb
        let storage = SqlRepoReader::new(None, "did:example:123456789abcdefghi".to_string(), None);
        let mut mst = MST::create(storage, None, None)?;

        mst = mst.add(&"com.example.record/3jqfcqzm3fo2j".to_string(), cid1, None)?;
        assert_eq!(mst.clone().leaf_count()?, 1);
        assert_eq!(
            mst.get_pointer()?.to_string(),
            "bafyreibj4lsc3aqnrvphp5xmrnfoorvru4wynt6lwidqbm2623a6tatzdu"
        );

        Ok(())
    }

    /// computes "singlelayer2" tree root CID
    #[test]
    fn singlelayer2_tree() -> Result<()> {
        let cid1 = Cid::try_from("bafyreie5cvv4h45feadgeuwhbcutmh6t2ceseocckahdoe6uat64zmz454")?; //dag-pb
        let storage = SqlRepoReader::new(None, "did:example:123456789abcdefghi".to_string(), None);
        let mut mst = MST::create(storage, None, None)?;

        mst = mst.add(&"com.example.record/3jqfcqzm3fx2j".to_string(), cid1, None)?;
        assert_eq!(mst.clone().leaf_count()?, 1);
        assert_eq!(mst.clone().layer, Some(2));
        assert_eq!(
            mst.get_pointer()?.to_string(),
            "bafyreih7wfei65pxzhauoibu3ls7jgmkju4bspy4t2ha2qdjnzqvoy33ai"
        );

        Ok(())
    }

    /// computes "simple" tree root CID
    #[test]
    fn simple_tree() -> Result<()> {
        let cid1 = Cid::try_from("bafyreie5cvv4h45feadgeuwhbcutmh6t2ceseocckahdoe6uat64zmz454")?;
        let storage = SqlRepoReader::new(None, "did:example:123456789abcdefghi".to_string(), None);
        let mut mst = MST::create(storage, None, None)?;

        let mut mst = mst.add(&"com.example.record/3jqfcqzm3fp2j".to_string(), cid1, None)?; // level 0
        let mut mst = mst.add(&"com.example.record/3jqfcqzm3fr2j".to_string(), cid1, None)?; // level 0
        let mut mst = mst.add(&"com.example.record/3jqfcqzm3fs2j".to_string(), cid1, None)?; // level 1
        let mut mst = mst.add(&"com.example.record/3jqfcqzm3ft2j".to_string(), cid1, None)?; // level 0
        let mut mst = mst.add(&"com.example.record/3jqfcqzm4fc2j".to_string(), cid1, None)?; // level 0
        assert_eq!(mst.clone().leaf_count()?, 5);
        assert_eq!(
            mst.get_pointer()?.to_string(),
            "bafyreicmahysq4n6wfuxo522m6dpiy7z7qzym3dzs756t5n7nfdgccwq7m"
        );

        Ok(())
    }

    // MST Interop Edge Cases

    /// trims top of tree on delete
    #[test]
    fn trim_on_delete() -> Result<()> {
        let cid1 = Cid::try_from("bafyreie5cvv4h45feadgeuwhbcutmh6t2ceseocckahdoe6uat64zmz454")?;
        let storage = SqlRepoReader::new(None, "did:example:123456789abcdefghi".to_string(), None);
        let mut mst = MST::create(storage, None, None)?;

        let l1root = "bafyreifnqrwbk6ffmyaz5qtujqrzf5qmxf7cbxvgzktl4e3gabuxbtatv4";
        let l0root = "bafyreie4kjuxbwkhzg2i5dljaswcroeih4dgiqq6pazcmunwt2byd725vi";

        let mut mst = mst.add(&"com.example.record/3jqfcqzm3fn2j".to_string(), cid1, None)?; // level 0
        let mut mst = mst.add(&"com.example.record/3jqfcqzm3fo2j".to_string(), cid1, None)?; // level 0
        let mut mst = mst.add(&"com.example.record/3jqfcqzm3fp2j".to_string(), cid1, None)?; // level 0
        let mut mst = mst.add(&"com.example.record/3jqfcqzm3fs2j".to_string(), cid1, None)?; // level 1
        let mut mst = mst.add(&"com.example.record/3jqfcqzm3ft2j".to_string(), cid1, None)?; // level 0
        let mut mst = mst.add(&"com.example.record/3jqfcqzm3fu2j".to_string(), cid1, None)?; // level 0
        assert_eq!(mst.clone().leaf_count()?, 6);
        assert_eq!(mst.get_layer()?, 1);
        assert_eq!(mst.get_pointer()?.to_string(), l1root);

        let mut mst = mst.delete(&"com.example.record/3jqfcqzm3fs2j".to_string())?; // level 1
        assert_eq!(mst.clone().leaf_count()?, 5);
        assert_eq!(mst.get_layer()?, 0);
        assert_eq!(mst.get_pointer()?.to_string(), l0root);

        Ok(())
    }

    /**
     *
     *                *                                  *
     *       _________|________                      ____|_____
     *       |   |    |    |   |                    |    |     |
     *       *   d    *    i   *       ->           *    f     *
     *     __|__    __|__    __|__                __|__      __|___
     *    |  |  |  |  |  |  |  |  |              |  |  |    |  |   |
     *    a  b  c  e  g  h  j  k  l              *  d  *    *  i   *
     *                                         __|__   |   _|_   __|__
     *                                        |  |  |  |  |   | |  |  |
     *                                        a  b  c  e  g   h j  k  l
     *
     */
    #[test]
    fn handle_insertion_that_splits_two_layers_down() -> Result<()> {
        let cid1 = Cid::try_from("bafyreie5cvv4h45feadgeuwhbcutmh6t2ceseocckahdoe6uat64zmz454")?;
        let storage = SqlRepoReader::new(None, "did:example:123456789abcdefghi".to_string(), None);
        let mut mst = MST::create(storage, None, None)?;

        let l1root = "bafyreiettyludka6fpgp33stwxfuwhkzlur6chs4d2v4nkmq2j3ogpdjem";
        let l2root = "bafyreid2x5eqs4w4qxvc5jiwda4cien3gw2q6cshofxwnvv7iucrmfohpm";

        let mut mst = mst.add(&"com.example.record/3jqfcqzm3fo2j".to_string(), cid1, None)?; // A; level 0
        let mut mst = mst.add(&"com.example.record/3jqfcqzm3fp2j".to_string(), cid1, None)?; // B; level 0
        let mut mst = mst.add(&"com.example.record/3jqfcqzm3fr2j".to_string(), cid1, None)?; // C; level 0
        let mut mst = mst.add(&"com.example.record/3jqfcqzm3fs2j".to_string(), cid1, None)?; // D; level 1
        let mut mst = mst.add(&"com.example.record/3jqfcqzm3ft2j".to_string(), cid1, None)?; // E; level 0
                                                                                             // GAP for F
        let mut mst = mst.add(&"com.example.record/3jqfcqzm3fz2j".to_string(), cid1, None)?; // G; level 0
        let mut mst = mst.add(&"com.example.record/3jqfcqzm4fc2j".to_string(), cid1, None)?; // H; level 0
        let mut mst = mst.add(&"com.example.record/3jqfcqzm4fd2j".to_string(), cid1, None)?; // I; level 1
        let mut mst = mst.add(&"com.example.record/3jqfcqzm4ff2j".to_string(), cid1, None)?; // J; level 0
        let mut mst = mst.add(&"com.example.record/3jqfcqzm4fg2j".to_string(), cid1, None)?; // K; level 0
        let mut mst = mst.add(&"com.example.record/3jqfcqzm4fh2j".to_string(), cid1, None)?; // L; level 0

        assert_eq!(mst.clone().leaf_count()?, 11);
        assert_eq!(mst.get_layer()?, 1);
        assert_eq!(mst.get_pointer()?.to_string(), l1root);

        // insert F, which will push E out of the node with G+H to a new node under D
        let mut mst = mst.add(&"com.example.record/3jqfcqzm3fx2j".to_string(), cid1, None)?; // F; level 2
        assert_eq!(mst.clone().leaf_count()?, 12);
        assert_eq!(mst.get_layer()?, 2);
        assert_eq!(mst.get_pointer()?.to_string(), l2root); // @TODO this is failing

        // remove F, which should push E back over with G+H
        let mut mst = mst.delete(&"com.example.record/3jqfcqzm3fx2j".to_string())?; // F; level 2
        assert_eq!(mst.clone().leaf_count()?, 11);
        assert_eq!(mst.get_layer()?, 1);
        assert_eq!(mst.get_pointer()?.to_string(), l1root);

        Ok(())
    }

    /**
     *
     *          *        ->            *
     *        __|__                  __|__
     *       |     |                |  |  |
     *       a     c                *  b  *
     *                              |     |
     *                              *     *
     *                              |     |
     *                              a     c
     *
     */
    #[test]
    fn handle_new_layers_that_are_two_higher_than_existing() -> Result<()> {
        let cid1 = Cid::try_from("bafyreie5cvv4h45feadgeuwhbcutmh6t2ceseocckahdoe6uat64zmz454")?;
        let storage = SqlRepoReader::new(None, "did:example:123456789abcdefghi".to_string(), None);
        let mut mst = MST::create(storage, None, None)?;

        let l0root = "bafyreidfcktqnfmykz2ps3dbul35pepleq7kvv526g47xahuz3rqtptmky";
        let l2root = "bafyreiavxaxdz7o7rbvr3zg2liox2yww46t7g6hkehx4i4h3lwudly7dhy";
        let l2root2 = "bafyreig4jv3vuajbsybhyvb7gggvpwh2zszwfyttjrj6qwvcsp24h6popu";

        let mut mst = mst.add(&"com.example.record/3jqfcqzm3ft2j".to_string(), cid1, None)?; // A; level 0
        let mut mst = mst.add(&"com.example.record/3jqfcqzm3fz2j".to_string(), cid1, None)?; // C; level 0
        assert_eq!(mst.clone().leaf_count()?, 2);
        assert_eq!(mst.get_layer()?, 0);
        assert_eq!(mst.get_pointer()?.to_string(), l0root);

        // insert B, which is two levels above
        let mut mst = mst.add(&"com.example.record/3jqfcqzm3fx2j".to_string(), cid1, None)?; // B; level 2
        assert_eq!(mst.clone().leaf_count()?, 3);
        assert_eq!(mst.get_layer()?, 2);
        assert_eq!(mst.get_pointer()?.to_string(), l2root);

        // remove B
        let mut mst = mst.delete(&"com.example.record/3jqfcqzm3fx2j".to_string())?; // B; level 2
        assert_eq!(mst.clone().leaf_count()?, 2);
        assert_eq!(mst.get_layer()?, 0);
        assert_eq!(mst.get_pointer()?.to_string(), l0root);

        // insert B (level=2) and D (level=1)
        let mut mst = mst.add(&"com.example.record/3jqfcqzm3fx2j".to_string(), cid1, None)?; // B; level 2
        let mut mst = mst.add(&"com.example.record/3jqfcqzm4fd2j".to_string(), cid1, None)?; // D; level 1
        assert_eq!(mst.clone().leaf_count()?, 4);
        assert_eq!(mst.get_layer()?, 2);
        assert_eq!(mst.get_pointer()?.to_string(), l2root2);

        // remove D
        let mut mst = mst.delete(&"com.example.record/3jqfcqzm4fd2j".to_string())?; // D; level 1
        assert_eq!(mst.clone().leaf_count()?, 3);
        assert_eq!(mst.get_layer()?, 2);
        assert_eq!(mst.get_pointer()?.to_string(), l2root);

        Ok(())
    }
}
