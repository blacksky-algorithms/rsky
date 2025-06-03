//! This module contains functionality for parsing, validating, and managing repository events
//! and commits within a decentralized version control system. It defines types and implementations
//! that adhere to specific protocol requirements and version constraints.
//!
//! # Constants
//! - `MAX_BLOCKS_BYTES`: Defines the maximum permitted size (in bytes) for blocks in a commit.
//! - `MAX_COMMIT_OPS`: Sets the limit for the number of operations allowed in a commit.
//! - `ATPROTO_REPO_VERSION`: Specifies the currently supported version of the repository protocol.
//!
//! # Types
//!
//! ## `RepoState`
//! A structure that represents the state of a repository, including its revision, data CID, 
//! and head CID. This is useful for tracking the evolution of a repository over time.
//!
//! ## `BlockMap`
//! A type alias for `HashMap<Cid, Vec<u8>>`, which maps content identifiers (CIDs) to their 
//! corresponding serialized block data.
//!
//! ## `InvertError`
//! An enumeration of errors that may occur during operations involving Merkle Search Trees (MST), 
//! serialization, multihash generation, or malformed data. This type provides detailed error messages
//! for debugging and issue resolution.
//!
//! ## `NodeEntry`
//! Represents an entry in a node of a Merkle Search Tree (MST), with variants for either a value
//! (key-value pair) or a child reference. It includes utility functions for manipulation and validation.
//!
//! # Key Structures and Methods
//!
//! ## `SubscribeReposEvent::validate`
//! Validates a given `Commit` against specific protocol rules, including size, structure, and
//! consistency constraints. This method works on various types of repository events, such as
//! commits or synchronization events, and provides detailed debug logging for failed validations.
//!
//! ### Arguments
//! - `commit`: A reference to the `Commit` object to be validated.
//! - `head`: A reference to the repository head (a `Cid`) for validation against the commit.
//!
//! ### Returns
//! A boolean indicating whether the given commit passes all validation checks or not.
//!
//! ### Validation Criteria
//! - Checks for deprecated flags (`too_big`, `rebase`) in the commit.
//! - Validates CID alignment to ensure structural consistency.
//! - Ensures blocks are non-empty and do not exceed specified size limits.
//! - Verifies the revision, protocol version, and DID comply with the repository's expected state.
//!
//! ## `SubscribeReposEvent::commit`
//! Parses the current repository event and attempts to extract the commit and its associated root CID.
//! It leverages `CarReader` for interpreting serialized block data and handles potential parse errors
//! gracefully.
//!
//! ### Returns
//! - An `Ok` result containing an optional `(Commit, Cid)` tuple if parsing is successful.
//! - An error of type `ParseError` if parsing fails, including cases where the root CID is missing.
//!
//! ## `SubscribeReposCommit::tree`
//! Constructs a Merkle Search Tree (MST) node based on the provided root CID and the event's blocks.
//! This method ensures the proper structure and integrity of the tree, handling any missing or malformed
//! data with custom errors (`ParseError`).
//!
//! ### Arguments
//! - `root`: The root `Cid` of the MST.
//!
//! ### Returns
//! - An `Ok` result containing the `Node` representing the tree structure.
//! - An error of type `ParseError` if the tree is incomplete or the root is missing.
//!
//! ## `NodeEntry::is_child`
//! A utility function to verify whether the `NodeEntry` represents a child node.
//!
//! ### Returns
//! - `true` if the entry is of variant `Child`.
//! - `false` otherwise.
//!
//! ## `NodeEntry::child_mut`
//! Attempts to retrieve a mutable reference to the `Node` child associated with the `NodeEntry`.
//!
//! ### Returns
//! - A mutable reference to the child `Node` on success.
//! - An error of type `InvertError` if the operation is invalid (e.g., the entry is not a child).
//!
//! # Logging
//! All methods that involve validation, parsing, or error handling use `tracing::debug` for detailed
//! logging. Logged information includes mismatched values, exceeded limits, and unsupported protocol versions.
//!
//! # Errors
//! Errors encountered during parsing, validation, or tree manipulation are encapsulated in custom
//! error types (`ParseError` and `InvertError`). These errors provide descriptive messages for easier
//! debugging and error resolution.
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
    /// Validates a commit against several criteria to ensure the integrity of the data and adherence
    /// to specific protocol rules. The function evaluates the provided `Commit` and compares it with
    /// the provided `head` CID. Depending on the variant of `self`, it may perform additional checks.
    ///
    /// # Arguments
    ///
    /// * `commit` - A reference to the `Commit` structure that is being validated. This contains
    ///   information about operations, blocks, revision, and other commit metadata.
    /// * `head` - A reference to the `Cid` (Content Identifier) representing the head of the repository
    ///   used for comparison against the `commit`.
    ///
    /// # Returns
    ///
    /// * `true` - If the commit satisfies all validation criteria.
    /// * `false` - If any of the criteria fail, logging a debug message for debugging purposes.
    ///
    /// # Validation Criteria
    ///
    /// 1. **Commit-Specific Validations:**
    ///    - Deprecated flags: Fails validation if the `too_big` or `rebase` flags are set.
    ///    - CID mismatch: Fails validation if the commit's inner CID does not match the given head.
    ///    - Operations limit: Ensures the number of operations does not exceed `MAX_COMMIT_OPS`.
    ///    - Blocks:
    ///        - Must not be empty.
    ///        - Must not exceed the maximum allowed size (`MAX_BLOCKS_BYTES`).
    ///
    /// 2. **Sync-Specific Validations:**
    ///    - Blocks must not exceed the maximum size limit (`MAX_BLOCKS_BYTES`).
    ///
    /// 3. **General Criteria:**
    ///    - DID (Decentralized Identifier): Ensures the commit's DID matches the expected value
    ///      derived from `self.did()`.
    ///    - Revision: Ensures the commit's revision matches the expected revision derived from the implementation.
    ///    - Protocol version: Validates that the commit uses the supported repository version (`ATPROTO_REPO_VERSION`).
    ///
    /// # Logging
    ///
    /// For each failed validation criterion, a debug message is logged using `tracing::debug!`. These
    /// messages include relevant information (e.g., mismatched values, exceeded limits) to help with
    /// debugging during development or issue diagnosis.
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
                    tracing::debug!(inner = %commit.commit, "mismatched inner commit cid");
                    return false;
                }
                if commit.ops.len() > MAX_COMMIT_OPS {
                    tracing::debug!(len = %commit.ops.len(), "too many ops in commit");
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
            tracing::debug!(inner = %commit.did, "mismatched inner commit did");
            return false;
        }
        if &commit.rev != rev {
            tracing::debug!(inner = %rev, "mismatched inner commit rev");
            return false;
        }
        if commit.version != ATPROTO_REPO_VERSION {
            tracing::debug!(version = %commit.version, "unsupported repo version");
            return false;
        }
        true
    }

    /// Attempts to extract and deserialize a commit from the object.
    ///
    /// # Returns
    /// - `Ok(Some((Commit, Cid)))` if a valid commit and its root CID are successfully retrieved and parsed.
    /// - `Ok(None)` if the object does not represent a commit (e.g., it is an Identity or Account).
    /// - `Err(ParseError)` if an error occurs during parsing or processing of the CAR file, including
    ///   cases where the root CID is missing in the CAR blocks.
    ///
    /// # Errors
    /// - Returns `ParseError::MissingRoot` if the root CID is not found in the CAR blocks.
    /// - Propagates errors from other functions, such as block parsing or deserialization.
    ///
    /// # Notes
    /// - This function differentiates between four variants of the object (`Commit`, `Sync`, `Identity`, 
    ///   and `Account`). It only processes `Commit` and `Sync` variants.
    /// - The function processes CAR (Content Addressable aRchive) format blocks and reads their contents 
    ///   to identify and deserialize the root commit block.
    ///
    /// # Example
    /// ```
    /// let result = object.commit();
    /// match result {
    ///     Ok(Some((commit, cid))) => {
    ///         println!("Successfully retrieved commit with CID: {:?}", cid);
    ///     }
    ///     Ok(None) => {
    ///         println!("No commit found for the given object.");
    ///     }
    ///     Err(e) => {
    ///         println!("Failed to parse commit: {:?}", e);
    ///     }
    /// }
    /// ```
    ///
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
    /// Constructs a tree structure from a given root CID using the provided block data.
    ///
    /// This function processes CAR (Content Addressable aRchive) encoded blocks to build a tree starting
    /// from the specified `root` CID. The blocks are organized into a `HashMap` for lookup during the tree
    /// construction process.
    ///
    /// # Arguments
    /// - `self`: A reference to the current object that contains the block data.
    /// - `root`: The root CID (`Cid`) representing the starting node of the tree to be generated.
    ///
    /// # Returns
    /// - `Ok(Node)`: On success, returns the root node of the constructed tree with its structure
    ///   properly set up.
    /// - `Err(ParseError)`: Returns an error if tree construction fails due to missing blocks or
    ///   an invalid root CID.
    ///
    /// # Errors
    /// - `ParseError::MissingRoot(root)`: If the root CID is not found in the provided block data.
    /// - Any other `ParseError` variants that occur during deserialization or tree construction.
    ///
    /// # Algorithm
    /// 1. Retrieves the internal slice of blocks and prepares to map blocks by their CID.
    /// 2. Uses `CarReader` to parse the CAR blocks, extracting their CIDs and underlying raw data.
    /// 3. Builds a `HashMap` (`block_map`) associating each CID with its corresponding block data.
    /// 4. Attempts to load the root `Node` from the `block_map`, returning an error if the root node
    ///    cannot be constructed.
    /// 5. Ensures that height information of all nodes is properly computed via `ensure_heights`.
    /// 6. On success, returns the fully constructed tree.
    ///
    /// # Dependencies
    /// - `Cid`: Represents the Content Identifier (CID) for nodes in the tree.
    /// - `CarReader`: Handles the parsing of CAR file data.
    /// - `Node`: Represents individual nodes in the tree and provides `load` and `ensure_heights` methods.
    /// - `ParseError`: Enum encapsulating errors that could occur during the tree construction process.
    ///
    /// # Example
    /// ```rust
    /// let root_cid: Cid = // Obtain the root CID from somewhere
    /// match your_struct.tree(root_cid) {
    ///     Ok(tree) => {
    ///         println!("Successfully built tree: {:?}", tree);
    ///     }
    ///     Err(e) => {
    ///         eprintln!("Failed to build tree: {:?}", e);
    ///     }
    /// }
    /// ```
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
            SubscribeReposCommitOperation::Create { path, cid: expected } => {
                let Some(found) = self.remove(path.as_str(), -1)? else {
                    tracing::debug!(%expected, "unable to invert create: not found");
                    return Ok(false);
                };
                if found == *expected {
                    Ok(true)
                } else {
                    tracing::debug!(%expected, %found, "unable to invert create");
                    Ok(false)
                }
            }
            SubscribeReposCommitOperation::Update { path, cid: expected, prev_data } => {
                #[expect(clippy::unwrap_used)]
                let Some(found) = self.insert(path.as_str(), prev_data.unwrap(), -1)? else {
                    tracing::debug!(%expected, "unable to invert update: not found");
                    return Ok(false);
                };
                if found == *expected {
                    Ok(true)
                } else {
                    tracing::debug!(%expected, %found, "unable to invert update");
                    Ok(false)
                }
            }
            SubscribeReposCommitOperation::Delete { path, prev_data } => {
                #[expect(clippy::unwrap_used)]
                let Some(found) = self.insert(path.as_str(), prev_data.unwrap(), -1)? else {
                    return Ok(true);
                };
                tracing::debug!(%found, "unable to invert delete");
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

    /// Determines the height (or depth), in levels, of a key within a Merkle Search Tree (MST),
    /// starting from the "bottom" of the tree at height 0. The height is calculated based on
    /// the leading zero bits of the SHA-256 hash of the provided key.
    ///
    /// # Parameters
    /// - `key`: A byte slice representing the key whose height in the MST is to be determined.
    ///
    /// # Returns
    /// An `i8` value representing the height of the key in the tree.
    ///
    /// # Details
    /// The function utilizes the following methodology:
    ///  - It computes the SHA-256 hash of the input `key`.
    ///  - Iterates over each byte of the hash, counting the leading pairs of zero bits.
    ///  - The count translates to the height (`0` for no leading zero bits, increasing by 1 for every
    ///    leading pair of zero bits).
    ///
    /// Additional notes about this specific MST implementation:
    /// - The tree has a fanout value of 16 (uses 4 bits or 2 bits per level).
    /// - The function stops counting once it encounters a non-zero leading bit pair.
    ///
    /// # Examples
    /// ```
    /// use sha2::{Digest, Sha256};
    ///
    /// let key = b"example_key";
    /// let height = height_for_key(key);
    /// println!("Height: {height}");
    /// ```
    ///
    /// For the key `"example_key"` and fanout of 16, the `height_for_key` function calculates
    /// the height in the MST by processing the SHA-256 hash of the key.
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

    /// Serializes a `Vec<u8>` as an IPLD (Interplanetary Linked Data) `Bytes` variant.
    ///
    /// # Parameters
    /// - `t`: A reference to a `Vec<u8>` that represents the data to be serialized.
    /// - `s`: A serializer instance that implements the `Serializer` trait, used to perform the serialization.
    ///
    /// # Returns
    /// - A `Result` containing the serialized data (`S::Ok`) if successful, or an error (`S::Error`) if the serialization fails.
    ///
    /// # Notes
    /// - This function expects ownership of the data in the `Vec<u8>` before serialization, which is why it clones the vector before wrapping it in the `Ipld::Bytes` variant.
    /// - The function is annotated with `#[expect(clippy::ptr_arg)]` to explicitly suppress the `clippy` lint warning about passing a `&Vec<u8>` instead of a `&[u8]`. This is likely done intentionally, as the clone operation requires ownership of the `Vec<u8>`.
    ///
    /// # Example
    /// ```
    /// use serde::Serialize;
    /// use serde_json::to_string;
    ///
    /// let data = vec![1, 2, 3, 4];
    /// let serialized = serialize(&data, to_string).expect("Serialization failed");
    /// ```
    ///
    /// # Implementation Detail
    /// This function wraps the vector in an `Ipld::Bytes` variant before serializing it. `Ipld` is assumed to be a custom enum that represents different data types for IPLD storage.
    #[expect(clippy::ptr_arg)]
    pub fn serialize<S: Serializer>(t: &Vec<u8>, s: S) -> Result<S::Ok, S::Error> {
        Ipld::Bytes(t.clone()).serialize(s)
    }

    /// Deserializes a vector of bytes (`Vec<u8>`) from a given deserializer.
    ///
    /// This function utilizes the `serde` library for deserialization. It attempts to deserialize
    /// an `Ipld` (InterPlanetary Linked Data) structure from the input deserializer, and expects
    /// the `Ipld` to contain a `Bytes` variant. If the deserialized `Ipld` does not match the
    /// expected variant, an error is returned.
    ///
    /// # Type Parameters
    /// - `'de`: A lifetime parameter required by the `Deserializer` trait.
    /// - `D`: The specific type implementing the `Deserializer` trait, which will supply the data
    ///   to be deserialized.
    ///
    /// # Arguments
    /// - `d`: A deserializer implementing the `Deserializer` trait, used to parse the serialized data
    ///   into an `Ipld` structure.
    ///
    /// # Returns
    /// - `Ok(Vec<u8>)`: If the deserialization succeeds, the function returns a `Vec<u8>` containing
    ///   the bytes extracted from the `Ipld` structure.
    /// - `Err(D::Error)`: If the deserialization fails or the `Ipld` structure does not match the
    ///   expected `Bytes` variant, the function returns an appropriate error.
    ///
    /// # Errors
    /// - Returns a custom error if the deserialized `Ipld` is not the `Bytes` variant, including debugging
    ///   information about the `Ipld`'s kind.
    /// - Errors may also arise due to the inability to properly deserialize data through the given
    ///   deserializer.
    ///
    /// # Example
    /// ```rust
    /// use serde::de::Deserializer;
    ///
    /// // Assuming `DeserializationContext` implements `Deserializer`
    /// let deserializer: DeserializationContext = ...;
    /// let result: Result<Vec<u8>, _> = deserialize(deserializer);
    ///
    /// match result {
    ///     Ok(bytes) => {
    ///         println!("Successfully deserialized bytes: {:?}", bytes);
    ///     }
    ///     Err(err) => {
    ///         eprintln!("Error deserializing: {:?}", err);
    ///     }
    /// }
    /// ```
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let ipld = Ipld::deserialize(d)?;
        let Ipld::Bytes(key_suffix) = ipld else {
            return Err(D::Error::custom(format!("expected ipld bytes, got: {:?}", ipld.kind())));
        };
        Ok(key_suffix)
    }
}
