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
use crate::common::tid::Ticker;
use crate::repo::block_map::BlockMap;
use crate::repo::cid_set::CidSet;
use crate::repo::error::DataStoreError;
use crate::repo::parse;
use crate::repo::types::CidAndBytes;
use crate::storage::types::RepoStorage;
use crate::storage::ObjAndBytes;
use crate::vendored::iroh_car::CarWriter;
use anyhow::{anyhow, Result};
use async_recursion::async_recursion;
use futures::{Stream, StreamExt};
use lexicon_cid::Cid;
use rocket::async_stream::stream;
use rocket::async_trait;
use serde_cbor::Value as CborValue;
use std::fmt::{Debug, Display, Formatter};
use std::sync::Arc;
use std::{fmt, mem};
use tokio::io::DuplexStream;
use tokio::sync::RwLock;

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
impl NodeIter {
    pub fn into_stream(self) -> impl Stream<Item = NodeEntry> {
        stream! {
            let mut current = self;
            loop {
                let next_item = current.next().await;
                match next_item {
                    Some(item) => yield item,
                    None => break,
                }
            }
        }
    }

    #[async_recursion(Sync)]
    async fn next(&mut self) -> Option<NodeEntry> {
        match self.entries.first() {
            None => {
                if let Some(this) = self.this.take() {
                    // If we have a stored MST node, return it
                    Some(this)
                } else {
                    // Proceed to parent
                    if let Some(parent) = self.parent.take() {
                        *self = *parent;
                        self.next().await
                    } else {
                        None
                    }
                }
            }
            Some(NodeEntry::Leaf(_)) => {
                // Remove and return the leaf
                let leaf = self.entries.remove(0);
                Some(leaf)
            }
            Some(NodeEntry::MST(_)) => {
                // Handle MST node
                let mut mst_entry = self.entries.remove(0);
                let entries = match &mut mst_entry {
                    NodeEntry::MST(subtree) => {
                        // Asynchronously fetch child entries
                        subtree.get_entries().await.unwrap_or_default()
                    }
                    _ => vec![],
                };

                // Create new child iterator with these entries
                let parent = mem::replace(
                    self,
                    NodeIter {
                        entries,
                        parent: Some(Box::new(NodeIter {
                            entries: vec![],
                            parent: None,
                            this: None,
                        })),
                        this: Some(mst_entry),
                    },
                );

                // Link the new iterator's parent to the current state
                self.parent = Some(Box::new(parent));

                // Continue processing with the new child iterator
                self.next().await
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

impl NodeIterReachable {
    pub fn into_stream(self) -> impl Stream<Item = Result<NodeEntry>> {
        stream! {
            let mut current = self;
            loop {
                let next_item = current.next().await;
                match next_item {
                    Some(item) => yield item,
                    None => break,
                }
            }
        }
    }

    #[async_recursion(Sync)]
    async fn next(&mut self) -> Option<Result<NodeEntry>> {
        match self.entries.first() {
            None => {
                if let Some(this) = self.this.take() {
                    // If we have a stored MST node, return it
                    Some(Ok(this))
                } else {
                    // Proceed to parent
                    if let Some(parent) = self.parent.take() {
                        *self = *parent;
                        self.next().await
                    } else {
                        None
                    }
                }
            }
            Some(NodeEntry::Leaf(_)) => {
                // Remove and return the leaf
                let leaf = self.entries.remove(0);
                Some(Ok(leaf))
            }
            Some(NodeEntry::MST(_)) => {
                // Handle MST node
                let mut mst_entry = self.entries.remove(0);
                let entries = match &mut mst_entry {
                    NodeEntry::MST(subtree) => {
                        // Asynchronously fetch child entries
                        subtree.get_entries().await
                    }
                    _ => Err(anyhow::Error::new(DataStoreError::MissingBlock(
                        "Missing Blocks".to_string(),
                    ))),
                };

                match entries {
                    Err(e) => {
                        match e.downcast_ref() {
                            Some(DataStoreError::MissingBlock(_)) => self.next().await, // Don't iterate
                            _ => return Some(Err(e)),
                        }
                    }
                    _ => {
                        *self = NodeIterReachable {
                            entries: entries.unwrap().to_vec(),
                            parent: Some(Box::new(mem::take(self))),
                            this: Some(mst_entry),
                        };
                        self.next().await
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
#[derive(PartialEq, Clone, Deserialize, Serialize)]
pub struct TreeEntry {
    pub p: u8, // count of characters shared with previous path/key in tree
    #[serde(with = "serde_bytes")]
    pub k: Vec<u8>, // remaining part of path/key (appended to "previous key")
    pub v: Cid, // CID pointer at this path/key
    pub t: Option<Cid>, // [optional] pointer to lower-level subtree to the "right" of this path/key entry
}

impl Debug for TreeEntry {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let t_string = match &self.t {
            None => None,
            Some(cid) => Some(cid.to_string()),
        };
        f.debug_struct("TreeEntry")
            .field("p", &self.p)
            .field("k", &self.k)
            .field("v", &self.v.to_string())
            .field("t", &t_string)
            .finish()
    }
}

/// MST tree node as gets serialized to CBOR. Note that the CBOR fields are all
/// single-character.
#[derive(PartialEq, Clone, Deserialize, Serialize)]
pub struct NodeData {
    pub l: Option<Cid>, // [optional] pointer to lower-level subtree to the "left" of this path/key
    pub e: Vec<TreeEntry>, // ordered list of entries at this node
}

impl Debug for NodeData {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let cid = match &self.l {
            None => None,
            Some(cid) => Some(cid.to_string()),
        };
        f.debug_struct("NodeData")
            .field("l", &cid)
            .field("e", &self.e)
            .finish()
    }
}

#[derive(Deserialize, Serialize, Clone)]
pub struct Leaf {
    pub key: String, // record key
    pub value: Cid,
}

impl Debug for Leaf {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("TreeEntry")
            .field("key", &self.key)
            .field("value", &self.value.to_string())
            .finish()
    }
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
                entries: vec![self],
                parent: None,
                this: None,
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
                this: None,
            },
            NodeEntry::Leaf(_) => NodeIterReachable {
                entries: vec![self.clone()],
                parent: None,
                this: None,
            },
        }
    }
}

#[async_trait]
pub trait AsyncPartialEq<Rhs: ?Sized = Self> {
    async fn async_eq(&self, other: &Rhs) -> bool;
}

#[async_trait]
impl AsyncPartialEq for NodeEntry {
    async fn async_eq(&self, other: &Self) -> bool {
        match (self, other) {
            (NodeEntry::Leaf(this), NodeEntry::Leaf(other)) => {
                this.key == other.key && this.value == other.value
            }
            (NodeEntry::Leaf(_), NodeEntry::MST(_)) => false,
            (NodeEntry::MST(_), NodeEntry::Leaf(_)) => false,
            (NodeEntry::MST(this), NodeEntry::MST(other)) => {
                let this_pointer = this
                    .get_pointer()
                    .await
                    .expect("Failed to get pointer for `this`");
                let other_pointer = other
                    .get_pointer()
                    .await
                    .expect("Failed to get pointer for `other`");
                this_pointer == other_pointer
            }
        }
    }
}

#[async_trait]
impl AsyncPartialEq<MST> for NodeEntry {
    async fn async_eq(&self, other: &MST) -> bool {
        match self {
            NodeEntry::Leaf(_) => false,
            NodeEntry::MST(this) => {
                let this_pointer = this
                    .get_pointer()
                    .await
                    .expect("Failed to get pointer for `this`");
                let other_pointer = other
                    .get_pointer()
                    .await
                    .expect("Failed to get pointer for `other`");
                this_pointer == other_pointer
            }
        }
    }
}

#[async_trait]
impl AsyncPartialEq<Leaf> for NodeEntry {
    async fn async_eq(&self, other: &Leaf) -> bool {
        match self {
            NodeEntry::Leaf(this) => this.key == other.key && this.value == other.value,
            NodeEntry::MST(_) => false,
        }
    }
}

#[async_trait]
impl<T> AsyncPartialEq for Vec<T>
where
    T: AsyncPartialEq + Sync, // Sync is needed because async_trait requires it.
{
    async fn async_eq(&self, other: &Self) -> bool {
        if self.len() != other.len() {
            return false;
        }
        // Compare elements one-by-one.
        for (a, b) in self.iter().zip(other.iter()) {
            if !a.async_eq(b).await {
                return false;
            }
        }
        true
    }
}

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
#[derive(Clone)]
pub struct MST {
    pub entries: Arc<RwLock<Option<Vec<NodeEntry>>>>,
    pub layer: Option<u32>,
    pub pointer: Arc<RwLock<Cid>>,
    pub outdated_pointer: Arc<RwLock<bool>>,
    pub storage: Arc<RwLock<dyn RepoStorage>>,
}

impl Debug for MST {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("MST")
            .field("entries", &self.entries.try_read().unwrap())
            .field("layer", &self.layer)
            .field("pointer", &self.pointer.try_read().unwrap().to_string())
            .field(
                "outdated_pointer",
                &self.outdated_pointer.try_read().unwrap(),
            )
            .finish()
    }
}

impl Display for MST {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        fn pointer_str(mst: &MST) -> String {
            let cid_guard = mst.pointer.try_read().unwrap();
            format!("*({})", util::short_cid(&*cid_guard))
        }

        fn fmt_mst(mst: &MST, f: &mut Formatter<'_>, prefix: &str, is_last: bool) -> fmt::Result {
            // Print MST pointer using our helper
            writeln!(
                f,
                "{}{}── {}",
                prefix,
                if is_last { "└" } else { "├" },
                pointer_str(mst),
            )?;

            // Prepare the child prefix
            let child_prefix = format!("{}{}", prefix, if is_last { "   " } else { "│  " });

            let entries_guard = mst.entries.try_read().unwrap();
            let entries = match &*entries_guard {
                Some(e) => e,
                None => {
                    writeln!(f, "{}(virtual node)", child_prefix)?;
                    return Ok(());
                }
            };

            for (i, entry) in entries.iter().enumerate() {
                let last_child = i == entries.len() - 1;
                match entry {
                    NodeEntry::Leaf(leaf) => {
                        // Print leaf key and (short) leaf value
                        writeln!(
                            f,
                            "{}{}── {} -> {}",
                            child_prefix,
                            if last_child { "└" } else { "├" },
                            leaf.key,
                            util::short_cid(&leaf.value)
                        )?;
                    }
                    NodeEntry::MST(child_mst) => {
                        // Recurse
                        fmt_mst(child_mst, f, &child_prefix, last_child)?;
                    }
                }
            }
            Ok(())
        }

        // Start with empty prefix for the root
        fmt_mst(self, f, "", true)
    }
}

impl MST {
    pub fn new(
        storage: Arc<RwLock<dyn RepoStorage>>,
        pointer: Cid,
        entries: Option<Vec<NodeEntry>>,
        layer: Option<u32>,
    ) -> Self {
        Self {
            storage,
            entries: Arc::new(RwLock::new(entries)),
            layer,
            pointer: Arc::new(RwLock::new(pointer)),
            outdated_pointer: Arc::new(RwLock::new(false)),
        }
    }

    pub async fn create(
        storage: Arc<RwLock<dyn RepoStorage>>,
        entries: Option<Vec<NodeEntry>>,
        layer: Option<u32>,
    ) -> Result<Self> {
        let entries = entries.unwrap_or(Vec::new());
        let pointer = util::cid_for_entries(entries.as_slice()).await?;
        Ok(MST::new(storage, pointer, Some(entries), layer))
    }

    pub fn from_data(
        storage: Arc<RwLock<dyn RepoStorage>>,
        data: &NodeData,
        layer: Option<u32>,
    ) -> Result<Self> {
        let entries = util::deserialize_node_data(storage.clone(), data, layer)?;
        let pointer = ipld::cid_for_cbor(&data)?;
        Ok(MST::new(storage, pointer, Some(entries), layer))
    }

    /// This is poorly named in both implementations, because it is lazy
    /// this is really a *lazy* load, doesn't actually touch storage
    pub fn load(
        storage: Arc<RwLock<dyn RepoStorage>>,
        cid: Cid,
        layer: Option<u32>,
    ) -> Result<Self> {
        Ok(MST::new(storage, cid, None, layer))
    }

    // Immutability
    // -------------------

    /// We never mutate an MST, we just return a new MST with updated values
    pub async fn new_tree(&mut self, entries: Vec<NodeEntry>) -> Result<Self> {
        let mut mst = MST::new(
            self.storage.clone(),
            self.pointer.read().await.clone(),
            Some(entries),
            self.layer,
        );
        mst.outdated_pointer = Arc::new(RwLock::new(true));
        Ok(mst)
    }

    // === "Getters (lazy load)" ===

    /// "We don't want to load entries of every subtree, just the ones we need"
    pub async fn get_entries(&self) -> Result<Vec<NodeEntry>> {
        // If `self.entries` is not populated, hydrate it first\
        {
            let mut entries = self.entries.write().await;
            if entries.is_none() {
                // Read from storage (block store) to get the node data
                let storage_guard = self.storage.read().await;
                let pointer = self.pointer.read().await;
                let data: CborValue = storage_guard
                    .read_obj(
                        &*pointer,
                        Box::new(|obj: CborValue| {
                            match serde_cbor::value::from_value::<NodeData>(obj.clone()) {
                                Ok(_) => true,
                                Err(_) => false,
                            }
                        }),
                    )
                    .await?;
                let data: NodeData = serde_cbor::value::from_value(data)?;

                // Compute the layer
                let first_leaf = data.e.get(0);
                let layer = match first_leaf {
                    Some(first_leaf) => Some(util::leading_zeros_on_hash(&first_leaf.k)?),
                    None => None,
                };

                // Deserialize into self.entries
                *entries = Some(util::deserialize_node_data(
                    self.storage.clone(),
                    &data,
                    layer,
                )?);
            }
        }

        let guard = self.entries.read().await;

        guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No entries present"))
            .map(|v| v.clone())
    }

    // We don't hash the node on every mutation for performance reasons
    // Instead we keep track of whether the pointer is outdated and only (recursively) calculate when needed
    #[async_recursion(Sync)]
    pub async fn get_pointer(&self) -> Result<Cid> {
        let mut outdated = self.outdated_pointer.write().await;
        if !*outdated {
            return Ok(*self.pointer.read().await);
        }

        let serialized = self.serialize().await?;
        *self.pointer.write().await = serialized.cid;
        *outdated = false;

        Ok(serialized.cid)
    }

    pub async fn serialize(&self) -> Result<CidAndBytes> {
        let mut entries = self.get_entries().await?;
        let mut outdated: Vec<Self> = Vec::new();
        for entry in &entries {
            if let NodeEntry::MST(ref mst) = entry {
                let is_outdated = *mst.outdated_pointer.read().await;
                if is_outdated {
                    outdated.push(mst.clone());
                }
            }
        }

        if outdated.len() > 0 {
            for outdated_entry in &outdated {
                let _ = outdated_entry.get_pointer().await?;
            }
            entries = self.get_entries().await?
        }
        let data = util::serialize_node_data(entries.as_slice()).await?;
        Ok(CidAndBytes {
            cid: ipld::cid_for_cbor(&data)?,
            bytes: common::struct_to_cbor(data)?,
        })
    }

    /// In most cases, we get the layer of a node from a hint on creation
    /// In the case of the topmost node in the tree, we look for a key in the node & determine the layer
    /// In the case where we don't find one, we recurse down until we do.
    /// If we still can't find one, then we have an empty tree and the node is layer 0
    pub async fn get_layer(&mut self) -> Result<u32> {
        self.layer = self.attempt_get_layer().await?;
        if self.layer.is_none() {
            self.layer = Some(0);
        }
        Ok(self.layer.unwrap_or(0))
    }

    #[async_recursion(Sync)]
    pub async fn attempt_get_layer(&mut self) -> Result<Option<u32>> {
        if self.layer.is_some() {
            return Ok(self.layer);
        };
        let mut entries = self.get_entries().await?;
        let mut layer = util::layer_for_entries(entries.as_slice())?;
        if layer.is_none() {
            for entry in entries.iter_mut() {
                if let NodeEntry::MST(ref mut tree) = entry {
                    let child_layer = tree.attempt_get_layer().await?;
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
    #[async_recursion(Sync)]
    pub async fn get_unstored_blocks(&self) -> Result<UnstoredBlocks> {
        let mut blocks = BlockMap::new();
        let pointer = self.get_pointer().await?;
        let already_has = {
            let storage_guard = self.storage.read().await;
            storage_guard.has(pointer).await?
        };
        if already_has {
            return Ok(UnstoredBlocks {
                root: pointer,
                blocks,
            });
        }
        let entries = self.get_entries().await?;
        let data: NodeData = util::serialize_node_data(entries.as_slice()).await?;
        let _ = blocks.add(data)?;
        for entry in entries.iter() {
            if let NodeEntry::MST(e) = entry {
                let subtree = e.get_unstored_blocks().await?;
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
    #[async_recursion(Sync)]
    pub async fn add(&mut self, key: &str, value: Cid, known_zeros: Option<u32>) -> Result<Self> {
        util::ensure_valid_mst_key(&key)?;
        let key_zeros: u32;
        if let Some(z) = known_zeros {
            key_zeros = z;
        } else {
            key_zeros = util::leading_zeros_on_hash(key.as_bytes())?;
        }
        let layer = self.get_layer().await?;

        let new_leaf = Leaf {
            key: key.to_string(),
            value,
        };

        return if key_zeros == layer {
            // it belongs in this layer
            let index = self.find_gt_or_equal_leaf_index(&key).await?;

            let found = self.at_index(index).await?;
            if let Some(NodeEntry::Leaf(l)) = found {
                if l.key == *key {
                    return Err(anyhow!("There is already a value at key: {}", key));
                }
            }
            let prev_node = self.at_index(index - 1).await?;
            if let Some(p) = prev_node {
                match p {
                    // if entry before is a leaf we can just splice in
                    NodeEntry::Leaf(_) => self.splice_in(NodeEntry::Leaf(new_leaf), index).await,
                    // else we try to split the subtree around the key
                    NodeEntry::MST(mut m) => {
                        let split_sub_tree = m.split_around(key).await?;
                        self.replace_with_split(
                            index - 1,
                            split_sub_tree.0,
                            new_leaf,
                            split_sub_tree.1,
                        )
                        .await
                    }
                }
            } else {
                // If we're on far left we can just splice in
                self.splice_in(NodeEntry::Leaf(new_leaf), index).await
            }
        } else if key_zeros < layer {
            // it belongs on a lower layer
            let index = self.find_gt_or_equal_leaf_index(key).await?;
            let prev_node = self.at_index(index - 1).await?;
            if let Some(NodeEntry::MST(mut p)) = prev_node {
                // if entry before is a tree, we add it to that tree
                let new_subtree = p.add(key, value, Some(key_zeros)).await?;
                self.update_entry(index - 1, NodeEntry::MST(new_subtree))
                    .await
            } else {
                let mut sub_tree = self.create_child().await?;
                let new_subtree = sub_tree.add(key, value, Some(key_zeros)).await?;
                self.splice_in(NodeEntry::MST(new_subtree), index).await
            }
        } else {
            let layer = self.get_layer().await?;
            let extra_layers_to_add = key_zeros - layer;

            // it belongs on a higher layer & we must push the rest of the tree down
            let split = self.split_around(key).await?;
            // if the newly added key has >=2 more leading zeros than the current highest layer
            // then we need to add in structural nodes in between as well
            let mut left: Option<Self> = split.0;
            let mut right: Option<Self> = split.1;
            // intentionally starting at 1, since first layer is taken care of by split
            for _ in 1..extra_layers_to_add {
                if let Some(l) = left.clone() {
                    left = Some(l.create_parent().await?);
                }
                if let Some(r) = right.clone() {
                    right = Some(r.create_parent().await?);
                }
            }
            let mut updated: Vec<NodeEntry> = Vec::new();
            if let Some(l) = left {
                updated.push(NodeEntry::MST(l));
            }
            updated.push(NodeEntry::Leaf(Leaf {
                key: key.to_string(),
                value,
            }));
            if let Some(r) = right {
                updated.push(NodeEntry::MST(r));
            }
            let mut new_root =
                MST::create(self.storage.clone(), Some(updated), Some(key_zeros)).await?;
            new_root.outdated_pointer = Arc::new(RwLock::new(true));
            Ok(new_root)
        };
    }

    /// Gets the value at the given key
    #[async_recursion(Sync)]
    pub async fn get(&mut self, key: &String) -> Result<Option<Cid>> {
        let index = self.find_gt_or_equal_leaf_index(key).await?;
        let found = self.at_index(index).await?;
        if let Some(NodeEntry::Leaf(f)) = found {
            if f.key == *key {
                return Ok(Some(f.value));
            }
        }
        let prev = self.at_index(index - 1).await?;
        if let Some(NodeEntry::MST(mut p)) = prev {
            return Ok(p.get(key).await?);
        }
        return Ok(None);
    }

    /// Edits the value at the given key
    /// Throws if the given key does not exist
    #[async_recursion(Sync)]
    pub async fn update(&mut self, key: &str, value: Cid) -> Result<Self> {
        util::ensure_valid_mst_key(key)?;
        let index = self.find_gt_or_equal_leaf_index(key).await?;
        let found = self.at_index(index).await?;
        if let Some(NodeEntry::Leaf(f)) = found {
            if f.key == *key {
                return self
                    .update_entry(
                        index,
                        NodeEntry::Leaf(Leaf {
                            key: key.to_string(),
                            value,
                        }),
                    )
                    .await;
            }
        }
        let prev = self.at_index(index - 1).await?;
        if let Some(NodeEntry::MST(mut p)) = prev {
            let updated_tree = p.update(key, value).await?;
            return self
                .update_entry(index - 1, NodeEntry::MST(updated_tree))
                .await;
        }
        Err(anyhow!("Could not find a record with key: {}", key))
    }

    /// Deletes the value at the given key
    pub async fn delete(&mut self, key: &String) -> Result<Self> {
        let altered = self.delete_recurse(key).await?;
        Ok(altered.trim_top().await?)
    }

    #[async_recursion(Sync)]
    pub async fn delete_recurse(&mut self, key: &String) -> Result<Self> {
        let index = self.find_gt_or_equal_leaf_index(key).await?;
        let found = self.at_index(index).await?;
        // if found, remove it on this level
        if let Some(NodeEntry::Leaf(f)) = found {
            if f.key == *key {
                let prev = self.at_index(index - 1).await?;
                let next = self.at_index(index + 1).await?;
                return match (prev, next) {
                    (Some(NodeEntry::MST(mut p)), Some(NodeEntry::MST(n))) => {
                        let merged = p.append_merge(n).await?;
                        let mut new_tree_entries: Vec<NodeEntry> = Vec::new();
                        new_tree_entries
                            .append(&mut self.slice(Some(0), Some(index - 1)).await?.to_vec());
                        new_tree_entries.push(NodeEntry::MST(merged));
                        new_tree_entries
                            .append(&mut self.slice(Some(index + 2), None).await?.to_vec());
                        self.new_tree(new_tree_entries).await
                    }
                    (_, _) => self.remove_entry(index).await,
                };
            }
        }
        // else recurse down to find it
        let prev = self.at_index(index - 1).await?;
        return if let Some(NodeEntry::MST(mut p)) = prev {
            let subtree = &mut p.delete_recurse(key).await?;
            let subtree_entries = subtree.get_entries().await?;
            if subtree_entries.len() == 0 {
                self.remove_entry(index - 1).await
            } else {
                self.update_entry(index - 1, NodeEntry::MST(subtree.clone()))
                    .await
            }
        } else {
            Err(anyhow!("Could not find a record with key: {}", key))
        };
    }

    // Simple Operations
    // -------------------

    /// update entry in place
    pub async fn update_entry(&mut self, index: isize, entry: NodeEntry) -> Result<Self> {
        let mut update = Vec::new();
        for e in self.slice(Some(0), Some(index)).await?.to_vec() {
            update.push(e);
        }
        update.push(entry);
        for e in self.slice(Some(index + 1), None).await?.to_vec() {
            update.push(e);
        }
        self.new_tree(update).await
    }

    /// remove entry at index
    pub async fn remove_entry(&mut self, index: isize) -> Result<Self> {
        let mut updated = Vec::new();
        updated.append(&mut self.slice(Some(0), Some(index)).await?.to_vec());
        updated.append(&mut self.slice(Some(index + 1), None).await?.to_vec());

        self.new_tree(updated).await
    }

    /// append entry to end of the node / Vec is allowed here.
    pub async fn append(&mut self, entry: NodeEntry) -> Result<Self> {
        let mut entries = self.get_entries().await?.to_vec();
        entries.push(entry);
        self.new_tree(entries).await
    }

    /// prepend entry to end of the node
    pub async fn prepend(&mut self, entry: NodeEntry) -> Result<Self> {
        let mut entries = self.get_entries().await?.to_vec();
        entries.splice(0..0, vec![entry]);
        self.new_tree(entries).await
    }

    /// returns entry at index
    pub async fn at_index(&mut self, index: isize) -> Result<Option<NodeEntry>> {
        let entries = self.get_entries().await?;
        if index >= 0 {
            Ok(entries
                .into_iter()
                .nth(index as usize)
                .map(|entry| entry.clone()))
        } else {
            Ok(None)
        }
    }

    /// returns a slice of the node
    pub async fn slice(&self, start: Option<isize>, end: Option<isize>) -> Result<Vec<NodeEntry>> {
        let entries = self.get_entries().await?;
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
    pub async fn splice_in(&mut self, entry: NodeEntry, index: isize) -> Result<Self> {
        let mut update = Vec::new();
        for e in self.slice(Some(0), Some(index)).await?.to_vec() {
            update.push(e);
        }
        update.push(entry);
        for e in self.slice(Some(index), None).await?.to_vec() {
            update.push(e);
        }
        self.new_tree(update).await
    }

    /// replaces an entry with [ Some(tree), Leaf, Some(tree) ]
    pub async fn replace_with_split(
        &mut self,
        index: isize,
        left: Option<Self>,
        leaf: Leaf,
        right: Option<Self>,
    ) -> Result<Self> {
        let update = self.slice(Some(0), Some(index)).await?;
        let mut update = update.to_vec();
        if let Some(l) = left {
            update.push(NodeEntry::MST(l));
        }
        update.push(NodeEntry::Leaf(leaf));
        if let Some(r) = right {
            update.push(NodeEntry::MST(r));
        }
        let remainder = self.slice(Some(index + 1), None).await?;
        let remainder = &mut remainder.to_vec();
        update.append(remainder);
        self.new_tree(update).await
    }

    /// if the topmost node in the tree only points to another tree, trim the top and return the subtree
    #[async_recursion(Sync)]
    pub async fn trim_top(self) -> Result<Self> {
        let entries = self.get_entries().await?;
        return if entries.len() == 1 {
            match entries.into_iter().nth(0) {
                Some(NodeEntry::MST(n)) => Ok(n.clone().trim_top().await?),
                _ => Ok(self),
            }
        } else {
            Ok(self)
        };
    }

    // Subtree & Splits
    // -------------------

    /// Recursively splits a subtree around a given key
    #[async_recursion(Sync)]
    pub async fn split_around(&mut self, key: &str) -> Result<(Option<Self>, Option<Self>)> {
        let index = self.find_gt_or_equal_leaf_index(key).await?;
        // split tree around key
        let left_data = {
            let tmp = self.slice(Some(0), Some(index)).await?;
            tmp.to_vec()
        };
        let right_data = {
            let tmp = self.slice(Some(index), None).await?;
            tmp.to_vec()
        };
        let mut left = self.new_tree(left_data.clone()).await?;
        let mut right = self.new_tree(right_data).await?;

        // if the far right of the left side is a subtree,
        // we need to split it on the key as well
        let left_len = left_data.len();
        let last_in_left: Option<NodeEntry> = if let [.., last] = left_data.as_slice() {
            Some(last.clone())
        } else {
            None
        };
        if let Some(NodeEntry::MST(mut last)) = last_in_left {
            left = left.remove_entry(left_len as isize - 1).await?;
            let split = last.split_around(key).await?;
            if let Some(s0) = split.0 {
                left = left.append(NodeEntry::MST(s0)).await?;
            }
            if let Some(s1) = split.1 {
                right = right.prepend(NodeEntry::MST(s1)).await?;
            }
        }

        let left_output: Option<Self>;
        match left.get_entries().await?.len() {
            0 => left_output = None,
            _ => left_output = Some(left),
        };
        let right_output: Option<Self>;
        match right.get_entries().await?.len() {
            0 => right_output = None,
            _ => right_output = Some(right),
        };
        Ok((left_output, right_output))
    }

    /// The simple merge case where every key in the right tree is greater than every key in the left tree
    /// (used primarily for deletes)
    #[async_recursion(Sync)]
    pub async fn append_merge(&mut self, mut to_merge: Self) -> Result<Self> {
        if self.get_layer().await? != to_merge.get_layer().await? {
            return Err(anyhow!(
                "Trying to merge two nodes from different layers of the MST"
            ));
        }
        let mut self_entries = self.get_entries().await?.to_vec();
        let mut to_merge_entries = to_merge.get_entries().await?.to_vec();
        let last_in_left = self_entries.last();
        let first_in_right = to_merge_entries.first();
        let mut new_tree_entries: Vec<NodeEntry> = Vec::new();
        match (last_in_left, first_in_right) {
            (Some(NodeEntry::MST(l)), Some(NodeEntry::MST(r))) => {
                let mut new_l = l.clone();
                let merged = new_l.append_merge(r.clone()).await?;
                self_entries.pop();
                new_tree_entries.append(&mut self_entries);
                new_tree_entries.push(NodeEntry::MST(merged));
                to_merge_entries.remove(0);
                new_tree_entries.append(&mut to_merge_entries);
            }
            (_, _) => {
                new_tree_entries.append(&mut self_entries);
                new_tree_entries.append(&mut to_merge_entries);
            }
        };
        self.new_tree(new_tree_entries).await
    }

    // Create relatives
    // -------------------

    pub async fn create_child(&mut self) -> Result<Self> {
        let layer = self.get_layer().await?;
        MST::create(self.storage.clone(), Some(Vec::new()), Some(layer - 1)).await
    }

    pub async fn create_parent(mut self) -> Result<Self> {
        let layer = self.get_layer().await?;
        let mut parent = MST::create(
            self.storage.clone(),
            Some(vec![NodeEntry::MST(self)]),
            Some(layer + 1),
        )
        .await?;
        parent.outdated_pointer = Arc::new(RwLock::new(true));
        Ok(parent)
    }

    // Finding insertion points
    // -------------------

    /// finds index of first leaf node that is greater than or equal to the value
    pub async fn find_gt_or_equal_leaf_index(&mut self, key: &str) -> Result<isize> {
        let entries = self.get_entries().await?;
        let maybe_index = entries.iter().position(|entry| match entry {
            NodeEntry::MST(_) => false,
            NodeEntry::Leaf(entry) => entry.key >= key.to_string(),
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
    #[async_recursion(Sync)]
    pub async fn walk_leaves_from(&mut self, key: &String) -> impl Iterator<Item = Leaf> {
        let mut iter: Vec<Leaf> = Vec::new();
        let index = self.find_gt_or_equal_leaf_index(key).await.unwrap() as usize;
        let entries = self.get_entries().await.unwrap();
        if let Some(prev_index) = index.checked_sub(1) {
            let prev = entries.get(prev_index);
            if let Some(NodeEntry::MST(p)) = prev {
                let mut p = p.clone();
                for leaf in p.walk_leaves_from(key).await {
                    iter.push(leaf);
                }
            }
        }
        for i in index..entries.len() {
            let entry = entries[i].clone();
            match entry {
                NodeEntry::Leaf(e) => iter.push(e),
                NodeEntry::MST(mut e) => {
                    for leaf in e.walk_leaves_from(key).await {
                        iter.push(leaf);
                    }
                }
            }
        }
        iter.into_iter()
    }

    pub async fn list(
        &mut self,
        count: Option<usize>,
        after: Option<String>,
        before: Option<String>,
    ) -> Result<Vec<Leaf>> {
        let mut vals: Vec<Leaf> = Vec::new();
        let after = after.unwrap_or("".to_owned());
        for leaf in self.walk_leaves_from(&after).await {
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

    pub async fn list_with_prefix(&mut self, prefix: &String, count: usize) -> Result<Vec<Leaf>> {
        let mut vals: Vec<Leaf> = Vec::new();
        for leaf in self.walk_leaves_from(prefix).await {
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
    pub fn walk(self) -> impl Stream<Item = NodeEntry> {
        NodeEntry::MST(self).iter().into_stream()
    }

    /// Walk full tree & emit nodes, consumer can bail at any point by returning None
    #[async_recursion(Sync)]
    pub async fn paths(self) -> Result<Vec<Vec<NodeEntry>>> {
        let mut paths: Vec<Vec<NodeEntry>> = Vec::new();
        let mut stream = Box::pin(self.walk());
        while let Some(entry) = stream.next().await {
            match entry {
                NodeEntry::Leaf(_) => paths.push(vec![entry]),
                NodeEntry::MST(ref m) => {
                    let sub_paths = m.clone().paths().await?;
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
    pub async fn all_nodes(self) -> Result<Vec<NodeEntry>> {
        let mut nodes: Vec<NodeEntry> = Vec::new();
        let mut stream = Box::pin(self.walk());
        while let Some(entry) = stream.next().await {
            match entry {
                NodeEntry::Leaf(_) => nodes.push(entry),
                NodeEntry::MST(m) => nodes.push(NodeEntry::MST(m)),
            }
        }
        Ok(nodes)
    }

    /// Walks tree & returns all cids
    #[async_recursion(Sync)]
    pub async fn all_cids(self) -> Result<CidSet> {
        let mut cids = CidSet::new(None);
        let mut stream = Box::pin(self.clone().walk());
        while let Some(entry) = stream.next().await {
            match entry {
                NodeEntry::Leaf(leaf) => cids.add(leaf.value),
                NodeEntry::MST(m) => {
                    let subtree_cids = m.all_cids().await?;
                    let _ = &cids.add_set(subtree_cids);
                }
            }
        }
        cids.add(self.clone().get_pointer().await?);
        Ok(cids)
    }

    /// Walks tree & returns all leaves
    pub async fn leaves(self) -> Result<Vec<Leaf>> {
        let mut leaves: Vec<Leaf> = Vec::new();
        let mut stream = Box::pin(self.walk());
        while let Some(entry) = stream.next().await {
            if let NodeEntry::Leaf(leaf) = entry {
                leaves.push(leaf);
            }
        }
        Ok(leaves)
    }

    /// Returns total leaf count
    pub async fn leaf_count(self) -> Result<usize> {
        let leaves = self.leaves().await?;
        Ok(leaves.len())
    }

    // Reachable tree traversal
    // -------------------

    /// Walk reachable branches of tree & emit nodes, consumer can bail at any point
    /// by returning false
    pub fn walk_reachable(self) -> impl Stream<Item = Result<NodeEntry>> {
        NodeEntry::MST(self).iter_reachable().into_stream()
    }

    pub async fn reachable_leaves(self) -> Result<Vec<Leaf>> {
        let mut leaves: Vec<Leaf> = Vec::new();
        let mut stream = Box::pin(self.walk_reachable());
        while let Some(entry) = stream.next().await {
            if let Ok(NodeEntry::Leaf(leaf)) = entry {
                leaves.push(leaf);
            }
        }
        Ok(leaves)
    }

    /// Sync Protocol
    pub async fn write_to_car_stream(
        &mut self,
        mut car: CarWriter<DuplexStream>,
    ) -> Result<CarWriter<DuplexStream>> {
        let mut leaves = CidSet::new(None);
        let mut to_fetch = CidSet::new(None);
        to_fetch.add(self.get_pointer().await?);
        while to_fetch.size() > 0 {
            let mut next_layer = CidSet::new(None);
            let fetched = {
                let storage_guard = self.storage.read().await;
                storage_guard.get_blocks(to_fetch.to_list()).await?
            };
            if fetched.missing.len() > 0 {
                return Err(anyhow::Error::new(DataStoreError::MissingBlocks(
                    "mst node".to_owned(),
                    fetched.missing,
                )));
            }
            for cid in to_fetch.to_list() {
                let found: ObjAndBytes =
                    parse::get_and_parse_by_kind(&fetched.blocks, cid, |obj: CborValue| {
                        match serde_cbor::value::from_value::<NodeData>(obj.clone()) {
                            Ok(_) => true,
                            Err(_) => false,
                        }
                    })?;
                car.write(cid, found.bytes).await?;
                let node_data: NodeData = serde_cbor::value::from_value(found.obj)?;
                let entries = util::deserialize_node_data(self.storage.clone(), &node_data, None)?;

                for entry in entries {
                    match entry {
                        NodeEntry::Leaf(l) => leaves.add(l.value),
                        NodeEntry::MST(m) => next_layer.add(m.get_pointer().await?),
                    }
                }
            }
            to_fetch = next_layer;
        }
        let leaf_data = {
            let storage_guard = self.storage.read().await;
            storage_guard.get_blocks(leaves.to_list()).await?
        };
        if leaf_data.missing.len() > 0 {
            return Err(anyhow::Error::new(DataStoreError::MissingBlocks(
                "mst leaf".to_owned(),
                leaf_data.missing,
            )));
        }
        for leaf in leaf_data.blocks.entries()? {
            car.write(leaf.cid, leaf.bytes).await?;
        }
        Ok(car)
    }

    #[async_recursion(Sync)]
    pub async fn cids_for_path(&mut self, key: String) -> Result<Vec<Cid>> {
        let mut cids: Vec<Cid> = vec![self.get_pointer().await?];
        let index = self.find_gt_or_equal_leaf_index(&key).await?;
        let found = self.at_index(index).await?;
        if let Some(NodeEntry::Leaf(l)) = found {
            if l.key == *key {
                cids.push(l.value);
                return Ok(cids);
            }
        }
        let prev = self.at_index(index - 1).await?;
        if let Some(NodeEntry::MST(mut p)) = prev {
            cids.append(&mut p.cids_for_path(key).await?);
            return Ok(cids);
        }
        Ok(cids)
    }

    #[async_recursion(Sync)]
    pub async fn add_blocks_for_path(&mut self, key: String, blocks: &mut BlockMap) -> Result<()> {
        let serialized = self.serialize().await?;
        blocks.set(serialized.cid, serialized.bytes);
        let index = self.find_gt_or_equal_leaf_index(&key).await?;
        let found = self.at_index(index).await?;
        if let Some(NodeEntry::Leaf(found)) = found {
            if found.key == key {
                return Ok(());
            }
        }
        match self.at_index(index - 1).await? {
            Some(NodeEntry::MST(mut prev)) => prev.add_blocks_for_path(key, blocks).await,
            _ => Ok(()),
        }
    }

    pub async fn save_mst(&self) -> Result<Cid> {
        let diff = self.get_unstored_blocks().await?;
        let storage = self.storage.read().await;
        storage
            .put_many(diff.blocks, Ticker::new().next(None).to_string())
            .await?;
        Ok(diff.root)
    }
}

#[async_trait]
impl AsyncPartialEq for MST {
    async fn async_eq(&self, other: &Self) -> bool {
        let this_pointer = self
            .get_pointer()
            .await
            .expect("Failed to get pointer for `this`");
        let other_pointer = other
            .get_pointer()
            .await
            .expect("Failed to get pointer for `other`");
        this_pointer == other_pointer
    }
}

#[async_trait]
impl AsyncPartialEq<Leaf> for MST {
    async fn async_eq(&self, _other: &Leaf) -> bool {
        false
    }
}

#[async_trait]
impl AsyncPartialEq<NodeEntry> for MST {
    async fn async_eq(&self, other: &NodeEntry) -> bool {
        match other {
            NodeEntry::Leaf(_) => false,
            NodeEntry::MST(other) => {
                let this_pointer = self
                    .get_pointer()
                    .await
                    .expect("Failed to get pointer for `this`");
                let other_pointer = other
                    .get_pointer()
                    .await
                    .expect("Failed to get pointer for `other`");
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
    use crate::repo::data_diff::{DataAdd, DataDelete, DataDiff, DataUpdate};
    use crate::storage::memory_blockstore::MemoryBlockstore;
    use anyhow::Result;
    use rand::seq::SliceRandom;
    use rand::thread_rng;
    use std::collections::HashMap;

    fn string_to_vec_u8(input: &str) -> Vec<u8> {
        input.as_bytes().to_vec()
    }

    #[tokio::test]
    async fn adds_records() -> Result<()> {
        let mut storage = MemoryBlockstore::default();
        let mapping = generate_bulk_data_keys(254, Some(&mut storage)).await?;
        let mut mst = MST::create(Arc::new(RwLock::new(storage)), None, None).await?;
        let mut rng = thread_rng();

        let mut entries = mapping
            .iter()
            .map(|e| (e.0.clone(), e.1.clone()))
            .collect::<Vec<(String, Cid)>>();
        entries.shuffle(&mut rng);

        for entry in &entries {
            //let start = std::time::Instant::now();
            mst = mst.add(&entry.0, entry.1, None).await?;
            //let duration = start.elapsed();
            //println!("Time:{:?}, Key:{}, Cid:{:?}", duration, &entry.0, entry.1);
        }
        for entry in entries {
            let got = mst.get(&entry.0).await?;
            assert_eq!(Some(entry.1), got);
        }
        let total_size = mst.leaf_count().await?;
        assert_eq!(total_size, 254);

        Ok(())
    }

    #[tokio::test]
    async fn edits_records() -> Result<()> {
        let mut storage = MemoryBlockstore::default();
        let mapping = generate_bulk_data_keys(100, Some(&mut storage)).await?;
        let mut mst = MST::create(Arc::new(RwLock::new(storage)), None, None).await?;
        let mut rng = thread_rng();

        let mut entries = mapping
            .iter()
            .map(|e| (e.0.clone(), e.1.clone()))
            .collect::<Vec<(String, Cid)>>();
        entries.shuffle(&mut rng);

        for entry in &entries {
            mst = mst.add(&entry.0, entry.1, None).await?;
        }

        let mut edited: Vec<(String, Cid)> = Vec::new();
        for entry in &entries {
            let mut no_storage: Option<&mut dyn RepoStorage> = None;
            let new_cid = random_cid(&mut no_storage, None).await?;
            mst = mst.update(&entry.0, new_cid).await?;
            edited.push((entry.0.clone(), new_cid));
        }
        for entry in edited {
            let got = mst.get(&entry.0).await?;
            assert_eq!(Some(entry.1), got);
        }
        let total_size = mst.leaf_count().await?;
        assert_eq!(total_size, 100);

        Ok(())
    }

    #[tokio::test]
    async fn deletes_records() -> Result<()> {
        let mut storage = MemoryBlockstore::default();
        let mapping = generate_bulk_data_keys(254, Some(&mut storage)).await?;
        let mut mst = MST::create(Arc::new(RwLock::new(storage)), None, None).await?;
        let mut rng = thread_rng();

        let mut entries = mapping
            .iter()
            .map(|e| (e.0.clone(), e.1.clone()))
            .collect::<Vec<(String, Cid)>>();
        entries.shuffle(&mut rng);

        for entry in &entries {
            mst = mst.add(&entry.0, entry.1, None).await?;
        }

        let to_delete = &entries[0..100];
        let the_rest = &entries[100..entries.len()];

        for entry in to_delete {
            mst = mst.delete(&entry.0).await?;
        }

        let total_size = mst.clone().leaf_count().await?;
        assert_eq!(total_size, 154);

        for entry in to_delete {
            let got = mst.get(&entry.0).await?;
            assert_eq!(None, got);
        }

        for entry in the_rest {
            let got = mst.get(&entry.0).await?;
            assert_eq!(Some(entry.1), got);
        }

        Ok(())
    }

    #[tokio::test]
    async fn is_order_independent() -> Result<()> {
        let mut storage = MemoryBlockstore::default();
        let mapping = generate_bulk_data_keys(254, Some(&mut storage)).await?;
        let mut mst = MST::create(Arc::new(RwLock::new(storage)), None, None).await?;
        let mut rng = thread_rng();

        let mut entries = mapping
            .iter()
            .map(|e| (e.0.clone(), e.1.clone()))
            .collect::<Vec<(String, Cid)>>();
        entries.shuffle(&mut rng);

        for entry in &entries {
            mst = mst.add(&entry.0, entry.1, None).await?;
        }

        let mut recreated = MST::create(mst.storage.clone(), None, None).await?;
        let all_nodes = mst.all_nodes().await?;

        let mut reshuffled = mapping
            .iter()
            .map(|e| (e.0.clone(), e.1.clone()))
            .collect::<Vec<(String, Cid)>>();
        reshuffled.shuffle(&mut rng);

        for entry in &reshuffled {
            recreated = recreated.add(&entry.0, entry.1, None).await?;
        }
        let all_reshuffled = recreated.all_nodes().await?;
        assert_eq!(all_nodes.len(), all_reshuffled.len());
        assert!(all_nodes.async_eq(&all_reshuffled).await);

        Ok(())
    }

    #[tokio::test]
    async fn saves_and_loads_from_blockstore() -> Result<()> {
        let mut storage = MemoryBlockstore::default();
        let _mapping = generate_bulk_data_keys(50, Some(&mut storage)).await?;
        let mst = MST::create(Arc::new(RwLock::new(storage)), None, None).await?;

        let root = mst.save_mst().await?;
        let loaded = MST::load(mst.storage.clone(), root, None)?;
        let original_nodes = mst.all_nodes().await?;
        let loaded_nodes = loaded.all_nodes().await?;

        assert_eq!(original_nodes.len(), loaded_nodes.len());
        assert!(original_nodes.async_eq(&loaded_nodes).await);

        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn diffs() -> Result<()> {
        let mut storage = MemoryBlockstore::default();
        let mapping = generate_bulk_data_keys(100, Some(&mut storage)).await?;
        let mut mst = MST::create(Arc::new(RwLock::new(storage)), None, None).await?;
        let entries = mapping
            .iter()
            .map(|e| (e.0.clone(), e.1.clone()))
            .collect::<Vec<(String, Cid)>>();

        for entry in &entries {
            mst = mst.add(&entry.0, entry.1, None).await?;
        }

        let _ = mst.save_mst().await?;
        let mut to_diff = mst.clone();

        let to_add = {
            let mut mst_storage = mst.storage.write().await;
            generate_bulk_data_keys(50, Some(&mut *mst_storage))
                .await?
                .into_iter()
                .map(|e| (e.0, e.1))
                .collect::<Vec<(String, Cid)>>()
        };

        let to_edit = entries[10..20].to_vec();
        let to_del = entries[20..30].to_vec();

        let mut expected_adds: HashMap<String, DataAdd> = HashMap::new();
        let mut expected_updates: HashMap<String, DataUpdate> = HashMap::new();
        let mut expected_dels: HashMap<String, DataDelete> = HashMap::new();

        for entry in &to_add {
            to_diff = to_diff.add(&entry.0, entry.1, None).await?;
            expected_adds.insert(
                entry.0.clone(),
                DataAdd {
                    key: entry.0.clone(),
                    cid: entry.1.clone(),
                },
            );
        }

        for entry in &to_edit {
            let mut no_storage: Option<&mut dyn RepoStorage> = None;
            let updated = random_cid(&mut no_storage, None).await?;
            to_diff = to_diff.update(&entry.0, updated).await?;
            expected_updates.insert(
                entry.0.clone(),
                DataUpdate {
                    key: entry.0.clone(),
                    prev: entry.1.clone(),
                    cid: updated,
                },
            );
        }

        for entry in &to_del {
            to_diff = to_diff.delete(&entry.0).await?;
            expected_dels.insert(
                entry.0.clone(),
                DataDelete {
                    key: entry.0.clone(),
                    cid: entry.1.clone(),
                },
            );
        }
        println!("@TESTS: mst - \n{}", mst);
        let diff = DataDiff::of(&mut to_diff, Some(&mut mst)).await?;
        assert_eq!(diff.add_list().len(), 50);
        assert_eq!(diff.update_list().len(), 10);
        assert_eq!(diff.delete_list().len(), 10);

        assert_eq!(diff.adds, expected_adds);
        assert_eq!(diff.updates, expected_updates);
        assert_eq!(diff.deletes, expected_dels);
        println!("@TESTS: to_diff - \n{}", to_diff);

        // ensure we correctly report all added CIDs
        let mut storage_guard = mst.storage.write().await;
        let mut stream = Box::pin(to_diff.walk());
        while let Some(mut entry) = stream.next().await {
            let cid: Cid = match entry {
                NodeEntry::MST(ref mut entry) => entry.get_pointer().await?,
                NodeEntry::Leaf(ref entry) => entry.value.clone(),
            };
            let found = (&mut *storage_guard).has(cid).await?
                || diff.new_mst_blocks.has(cid)
                || diff.new_leaf_cids.has(cid);
            assert!(found, "Missing block {cid}")
        }

        Ok(())
    }

    /// computes "simple" tree root CID
    #[tokio::test]
    async fn simple_tree_diffs() -> Result<()> {
        let cid1 = Cid::try_from("bafyreie5cvv4h45feadgeuwhbcutmh6t2ceseocckahdoe6uat64zmz454")?;
        let storage = MemoryBlockstore::default();
        let mut mst = MST::create(Arc::new(RwLock::new(storage)), None, None).await?;

        let mut mst = mst
            .add(&"com.example.record/3jqfcqzm3fp2j".to_string(), cid1, None)
            .await?; // level 0
        let mut mst = mst
            .add(&"com.example.record/3jqfcqzm3fr2j".to_string(), cid1, None)
            .await?; // level 0
        let mut mst = mst
            .add(&"com.example.record/3jqfcqzm3fs2j".to_string(), cid1, None)
            .await?; // level 1
        let mut to_diff = mst.clone();
        let mut expected_adds: HashMap<String, DataAdd> = HashMap::new();
        let mut expected_updates: HashMap<String, DataUpdate> = HashMap::new();
        let mut expected_dels: HashMap<String, DataDelete> = HashMap::new();

        to_diff = to_diff
            .add(&"com.example.record/3jqfcqzm3ft2j".to_string(), cid1, None)
            .await?; // level 0
        expected_adds.insert(
            "com.example.record/3jqfcqzm3ft2j".to_string(),
            DataAdd {
                key: "com.example.record/3jqfcqzm3ft2j".to_string(),
                cid: cid1,
            },
        );
        to_diff = to_diff
            .add(&"com.example.record/3jqfcqzm4fc2j".to_string(), cid1, None)
            .await?; // level 0
        expected_adds.insert(
            "com.example.record/3jqfcqzm4fc2j".to_string(),
            DataAdd {
                key: "com.example.record/3jqfcqzm4fc2j".to_string(),
                cid: cid1,
            },
        );
        let mut no_storage: Option<&mut dyn RepoStorage> = None;
        let updated = random_cid(&mut no_storage, None).await?;
        to_diff = to_diff
            .update(&"com.example.record/3jqfcqzm3fs2j", updated)
            .await?;
        expected_updates.insert(
            "com.example.record/3jqfcqzm3fs2j".to_string(),
            DataUpdate {
                key: "com.example.record/3jqfcqzm3fs2j".to_string(),
                prev: cid1,
                cid: updated,
            },
        );

        to_diff = to_diff
            .delete(&"com.example.record/3jqfcqzm3fr2j".to_string())
            .await?;
        expected_dels.insert(
            "com.example.record/3jqfcqzm3fr2j".to_string(),
            DataDelete {
                key: "com.example.record/3jqfcqzm3fr2j".to_string(),
                cid: cid1,
            },
        );

        let diff = DataDiff::of(&mut to_diff, Some(&mut mst)).await?;
        assert_eq!(diff.add_list().len(), 2);
        assert_eq!(diff.update_list().len(), 1);
        assert_eq!(diff.delete_list().len(), 1);

        assert_eq!(diff.adds, expected_adds);
        assert_eq!(diff.updates, expected_updates);
        assert_eq!(diff.deletes, expected_dels);

        // ensure we correctly report all added CIDs
        let mut blockstore_guard = mst.storage.write().await;
        let mut stream = Box::pin(to_diff.walk());
        while let Some(mut entry) = stream.next().await {
            let cid: Cid = match entry {
                NodeEntry::MST(ref mut entry) => entry.get_pointer().await?,
                NodeEntry::Leaf(ref entry) => entry.value.clone(),
            };
            let found = (&mut *blockstore_guard).has(cid).await?
                || diff.new_mst_blocks.has(cid)
                || diff.new_leaf_cids.has(cid);
            assert!(found)
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_leading_zeros() -> Result<()> {
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

    #[tokio::test]
    async fn test_prefix_len() -> Result<()> {
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

    #[tokio::test]
    async fn test_prefix_len_wide() -> Result<()> {
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

    #[tokio::test]
    async fn test_allowed_keys() -> Result<()> {
        let cid1str = "bafyreie5cvv4h45feadgeuwhbcutmh6t2ceseocckahdoe6uat64zmz454";
        let cid1 = Cid::try_from(cid1str)?;

        let storage = MemoryBlockstore::default();
        let mut mst = MST::create(Arc::new(RwLock::new(storage)), None, None).await?;
        // Rejects empty key
        let result = mst.add(&"".to_string(), cid1, None).await;
        assert!(result.is_err());

        // Rejects a key with no collection
        let result = mst.add(&"asdf".to_string(), cid1, None).await;
        assert!(result.is_err());

        // Rejects a key with a nested collection
        let result = mst
            .add(&"nested/collection/asdf".to_string(), cid1, None)
            .await;
        assert!(result.is_err());

        // Rejects on empty coll or rkey
        let result = mst.add(&"coll/".to_string(), cid1, None).await;
        assert!(result.is_err());
        let result = mst.add(&"/rkey".to_string(), cid1, None).await;
        assert!(result.is_err());

        // Rejects non-ascii chars
        let result = mst.add(&"coll/jalapeñoA".to_string(), cid1, None).await;
        assert!(result.is_err());
        let result = mst.add(&"coll/coöperative".to_string(), cid1, None).await;
        assert!(result.is_err());
        let result = mst.add(&"coll/abc💩".to_string(), cid1, None).await;
        assert!(result.is_err());

        // Rejects ascii that we don't support
        let invalid_chars = vec!["$", "%", "(", ")", "+", "="];
        for ch in invalid_chars {
            let key = format!("coll/key{}", ch);
            let result = mst.add(&key, cid1, None).await;
            assert!(result.is_err(), "Key '{}' should be invalid", key);
        }

        // Rejects keys over 256 chars
        let long_key: String = "a".repeat(253);
        let key = format!("coll/{}", long_key);
        let result = mst.add(&key, cid1, None).await;
        assert!(result.is_err());

        // Allows long key under 256 chars
        let long_key: String = "a".repeat(250);
        let key = format!("coll/{}", long_key);
        let result = mst.add(&key, cid1, None).await;
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
            let result = mst.add(&key.to_string(), cid1, None).await;
            assert!(result.is_ok(), "Key '{}' should be valid", key);
        }

        Ok(())
    }

    // MST Interop Known Maps

    /// computes "empty" tree root CID
    #[tokio::test]
    async fn empty_tree_root() -> Result<()> {
        let storage = MemoryBlockstore::default();
        let mst = MST::create(Arc::new(RwLock::new(storage)), None, None).await?;
        assert_eq!(mst.clone().leaf_count().await?, 0);
        assert_eq!(
            mst.get_pointer().await?.to_string(),
            "bafyreie5737gdxlw5i64vzichcalba3z2v5n6icifvx5xytvske7mr3hpm"
        );
        Ok(())
    }

    /// computes "trivial" tree root CID
    #[tokio::test]
    async fn trivial_tree() -> Result<()> {
        let cid1 = Cid::try_from("bafyreie5cvv4h45feadgeuwhbcutmh6t2ceseocckahdoe6uat64zmz454")?; //dag-pb
        let storage = MemoryBlockstore::default();
        let mut mst = MST::create(Arc::new(RwLock::new(storage)), None, None).await?;

        mst = mst
            .add(&"com.example.record/3jqfcqzm3fo2j".to_string(), cid1, None)
            .await?;
        assert_eq!(mst.clone().leaf_count().await?, 1);
        assert_eq!(
            mst.get_pointer().await?.to_string(),
            "bafyreibj4lsc3aqnrvphp5xmrnfoorvru4wynt6lwidqbm2623a6tatzdu"
        );

        Ok(())
    }

    /// computes "singlelayer2" tree root CID
    #[tokio::test]
    async fn singlelayer2_tree() -> Result<()> {
        let cid1 = Cid::try_from("bafyreie5cvv4h45feadgeuwhbcutmh6t2ceseocckahdoe6uat64zmz454")?; //dag-pb
        let storage = MemoryBlockstore::default();
        let mut mst = MST::create(Arc::new(RwLock::new(storage)), None, None).await?;

        mst = mst
            .add(&"com.example.record/3jqfcqzm3fx2j".to_string(), cid1, None)
            .await?;
        assert_eq!(mst.clone().leaf_count().await?, 1);
        assert_eq!(mst.clone().layer, Some(2));
        assert_eq!(
            mst.get_pointer().await?.to_string(),
            "bafyreih7wfei65pxzhauoibu3ls7jgmkju4bspy4t2ha2qdjnzqvoy33ai"
        );

        Ok(())
    }

    /// computes "simple" tree root CID
    #[tokio::test]
    async fn simple_tree() -> Result<()> {
        let cid1 = Cid::try_from("bafyreie5cvv4h45feadgeuwhbcutmh6t2ceseocckahdoe6uat64zmz454")?;
        let storage = MemoryBlockstore::default();
        let mut mst = MST::create(Arc::new(RwLock::new(storage)), None, None).await?;

        let mut mst = mst
            .add(&"com.example.record/3jqfcqzm3fp2j".to_string(), cid1, None)
            .await?; // level 0
        let mut mst = mst
            .add(&"com.example.record/3jqfcqzm3fr2j".to_string(), cid1, None)
            .await?; // level 0
        let mut mst = mst
            .add(&"com.example.record/3jqfcqzm3fs2j".to_string(), cid1, None)
            .await?; // level 1
        let mut mst = mst
            .add(&"com.example.record/3jqfcqzm3ft2j".to_string(), cid1, None)
            .await?; // level 0
        let mst = mst
            .add(&"com.example.record/3jqfcqzm4fc2j".to_string(), cid1, None)
            .await?; // level 0
        assert_eq!(mst.clone().leaf_count().await?, 5);
        assert_eq!(
            mst.get_pointer().await?.to_string(),
            "bafyreicmahysq4n6wfuxo522m6dpiy7z7qzym3dzs756t5n7nfdgccwq7m"
        );

        Ok(())
    }

    // MST Interop Edge Cases

    /// trims top of tree on delete
    #[tokio::test]
    async fn trim_on_delete() -> Result<()> {
        let cid1 = Cid::try_from("bafyreie5cvv4h45feadgeuwhbcutmh6t2ceseocckahdoe6uat64zmz454")?;
        let storage = MemoryBlockstore::default();
        let mut mst = MST::create(Arc::new(RwLock::new(storage)), None, None).await?;

        let l1root = "bafyreifnqrwbk6ffmyaz5qtujqrzf5qmxf7cbxvgzktl4e3gabuxbtatv4";
        let l0root = "bafyreie4kjuxbwkhzg2i5dljaswcroeih4dgiqq6pazcmunwt2byd725vi";

        let mut mst = mst
            .add(&"com.example.record/3jqfcqzm3fn2j".to_string(), cid1, None)
            .await?; // level 0
        let mut mst = mst
            .add(&"com.example.record/3jqfcqzm3fo2j".to_string(), cid1, None)
            .await?; // level 0
        let mut mst = mst
            .add(&"com.example.record/3jqfcqzm3fp2j".to_string(), cid1, None)
            .await?; // level 0
        let mut mst = mst
            .add(&"com.example.record/3jqfcqzm3fs2j".to_string(), cid1, None)
            .await?; // level 1
        let mut mst = mst
            .add(&"com.example.record/3jqfcqzm3ft2j".to_string(), cid1, None)
            .await?; // level 0
        let mut mst = mst
            .add(&"com.example.record/3jqfcqzm3fu2j".to_string(), cid1, None)
            .await?; // level 0
        assert_eq!(mst.clone().leaf_count().await?, 6);
        assert_eq!(mst.get_layer().await?, 1);
        assert_eq!(mst.get_pointer().await?.to_string(), l1root);

        let mut mst = mst
            .delete(&"com.example.record/3jqfcqzm3fs2j".to_string())
            .await?; // level 1
        assert_eq!(mst.clone().leaf_count().await?, 5);
        assert_eq!(mst.get_layer().await?, 0);
        assert_eq!(mst.get_pointer().await?.to_string(), l0root);

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
    #[tokio::test]
    async fn handle_insertion_that_splits_two_layers_down() -> Result<()> {
        let cid1 = Cid::try_from("bafyreie5cvv4h45feadgeuwhbcutmh6t2ceseocckahdoe6uat64zmz454")?;
        let storage = MemoryBlockstore::default();
        let mut mst = MST::create(Arc::new(RwLock::new(storage)), None, None).await?;

        let l1root = "bafyreiettyludka6fpgp33stwxfuwhkzlur6chs4d2v4nkmq2j3ogpdjem";
        let l2root = "bafyreid2x5eqs4w4qxvc5jiwda4cien3gw2q6cshofxwnvv7iucrmfohpm";

        let mut mst = mst
            .add(&"com.example.record/3jqfcqzm3fo2j".to_string(), cid1, None)
            .await?; // A; level 0
        let mut mst = mst
            .add(&"com.example.record/3jqfcqzm3fp2j".to_string(), cid1, None)
            .await?; // B; level 0
        let mut mst = mst
            .add(&"com.example.record/3jqfcqzm3fr2j".to_string(), cid1, None)
            .await?; // C; level 0
        let mut mst = mst
            .add(&"com.example.record/3jqfcqzm3fs2j".to_string(), cid1, None)
            .await?; // D; level 1
        let mut mst = mst
            .add(&"com.example.record/3jqfcqzm3ft2j".to_string(), cid1, None)
            .await?; // E; level 0
                     // GAP for F
        let mut mst = mst
            .add(&"com.example.record/3jqfcqzm3fz2j".to_string(), cid1, None)
            .await?; // G; level 0
        let mut mst = mst
            .add(&"com.example.record/3jqfcqzm4fc2j".to_string(), cid1, None)
            .await?; // H; level 0
        let mut mst = mst
            .add(&"com.example.record/3jqfcqzm4fd2j".to_string(), cid1, None)
            .await?; // I; level 1
        let mut mst = mst
            .add(&"com.example.record/3jqfcqzm4ff2j".to_string(), cid1, None)
            .await?; // J; level 0
        let mut mst = mst
            .add(&"com.example.record/3jqfcqzm4fg2j".to_string(), cid1, None)
            .await?; // K; level 0
        let mut mst = mst
            .add(&"com.example.record/3jqfcqzm4fh2j".to_string(), cid1, None)
            .await?; // L; level 0

        assert_eq!(mst.clone().leaf_count().await?, 11);
        assert_eq!(mst.get_layer().await?, 1);
        assert_eq!(mst.get_pointer().await?.to_string(), l1root);

        // insert F, which will push E out of the node with G+H to a new node under D
        let mut mst = mst
            .add(&"com.example.record/3jqfcqzm3fx2j".to_string(), cid1, None)
            .await?; // F; level 2
        assert_eq!(mst.clone().leaf_count().await?, 12);
        assert_eq!(mst.get_layer().await?, 2);
        assert_eq!(mst.get_pointer().await?.to_string(), l2root); // @TODO this is failing

        // remove F, which should push E back over with G+H
        let mut mst = mst
            .delete(&"com.example.record/3jqfcqzm3fx2j".to_string())
            .await?; // F; level 2
        assert_eq!(mst.clone().leaf_count().await?, 11);
        assert_eq!(mst.get_layer().await?, 1);
        assert_eq!(mst.get_pointer().await?.to_string(), l1root);

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
    #[tokio::test]
    async fn handle_new_layers_that_are_two_higher_than_existing() -> Result<()> {
        let cid1 = Cid::try_from("bafyreie5cvv4h45feadgeuwhbcutmh6t2ceseocckahdoe6uat64zmz454")?;
        let storage = MemoryBlockstore::default();
        let mut mst = MST::create(Arc::new(RwLock::new(storage)), None, None).await?;

        let l0root = "bafyreidfcktqnfmykz2ps3dbul35pepleq7kvv526g47xahuz3rqtptmky";
        let l2root = "bafyreiavxaxdz7o7rbvr3zg2liox2yww46t7g6hkehx4i4h3lwudly7dhy";
        let l2root2 = "bafyreig4jv3vuajbsybhyvb7gggvpwh2zszwfyttjrj6qwvcsp24h6popu";

        let mut mst = mst
            .add(&"com.example.record/3jqfcqzm3ft2j".to_string(), cid1, None)
            .await?; // A; level 0
        let mut mst = mst
            .add(&"com.example.record/3jqfcqzm3fz2j".to_string(), cid1, None)
            .await?; // C; level 0
        assert_eq!(mst.clone().leaf_count().await?, 2);
        assert_eq!(mst.get_layer().await?, 0);
        assert_eq!(mst.get_pointer().await?.to_string(), l0root);

        // insert B, which is two levels above
        let mut mst = mst
            .add(&"com.example.record/3jqfcqzm3fx2j".to_string(), cid1, None)
            .await?; // B; level 2
        assert_eq!(mst.clone().leaf_count().await?, 3);
        assert_eq!(mst.get_layer().await?, 2);
        assert_eq!(mst.get_pointer().await?.to_string(), l2root);

        // remove B
        let mut mst = mst
            .delete(&"com.example.record/3jqfcqzm3fx2j".to_string())
            .await?; // B; level 2
        assert_eq!(mst.clone().leaf_count().await?, 2);
        assert_eq!(mst.get_layer().await?, 0);
        assert_eq!(mst.get_pointer().await?.to_string(), l0root);

        // insert B (level=2) and D (level=1)
        let mut mst = mst
            .add(&"com.example.record/3jqfcqzm3fx2j".to_string(), cid1, None)
            .await?; // B; level 2
        let mut mst = mst
            .add(&"com.example.record/3jqfcqzm4fd2j".to_string(), cid1, None)
            .await?; // D; level 1
        assert_eq!(mst.clone().leaf_count().await?, 4);
        assert_eq!(mst.get_layer().await?, 2);
        assert_eq!(mst.get_pointer().await?.to_string(), l2root2);

        // remove D
        let mut mst = mst
            .delete(&"com.example.record/3jqfcqzm4fd2j".to_string())
            .await?; // D; level 1
        assert_eq!(mst.clone().leaf_count().await?, 3);
        assert_eq!(mst.get_layer().await?, 2);
        assert_eq!(mst.get_pointer().await?.to_string(), l2root);

        Ok(())
    }
}
