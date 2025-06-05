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
    /// Inverts a given `SubscribeReposCommitOperation` for the current state.
    ///
    /// This function attempts to revert a `SubscribeReposCommitOperation` for the 
    /// current state of the repository. It supports the following types of operations:
    /// `Create`, `Update`, and `Delete`. Depending on the operation type, this function
    /// either removes, updates, or re-inserts data, verifying correctness by checking the
    /// operation's expected values (`cid` or `prev_data`).
    ///
    /// # Parameters
    /// - `op`: A reference to a `SubscribeReposCommitOperation` which represents the operation
    ///   to invert. This could be one of the following variants:
    ///   - `Create { path, cid }`: Reverts a create operation. It attempts to remove the given
    ///     `path` and checks if the expected `cid` matches the found data.
    ///   - `Update { path, cid, prev_data }`: Reverts an update operation by re-inserting the
    ///     previous data (`prev_data`) into the repository and verifying the `cid`.
    ///   - `Delete { path, prev_data }`: Reverts a delete operation by re-inserting the previous
    ///     data (`prev_data`) into the repository.
    ///
    /// # Returns
    /// - `Ok(true)`: Indicates that the inversion succeeded, and the repository state has been 
    ///   successfully restored for the operation.
    /// - `Ok(false)`: Indicates that the inversion could not be performed due to mismatched data
    ///   or missing entries. In such cases, the operation's state could not be reversed as desired.
    /// - `Err(InvertError)`: If an error occurs during the inversion process, such as issues with
    ///   repository state management, an `InvertError` is returned.
    ///
    /// # Errors
    /// - Returns an `InvertError` if there is a failure in performing an operation on the repository
    ///   such as insert or remove.
    ///
    /// # Notes
    /// - Logging is used extensively to provide debug information, including cases where inversion 
    ///   fails, mismatched data, or missing entries.
    /// - This function uses Rust's `let-else` constructs to efficiently handle optional values.
    /// - Use of `unwrap` is expected in certain controlled cases (e.g., when `prev_data` is guaranteed
    ///   to exist in `Update` or `Delete` operations).
    ///
    /// # Examples
    ///
    /// ## Reverting a Create Operation
    /// ```rust
    /// let mut repository = Repository::new();
    /// let op = SubscribeReposCommitOperation::Create { path: "file.txt".to_string(), cid: "abc123".to_string() };
    /// repository.invert(&op).expect("Inversion should succeed");
    /// ```
    ///
    /// ## Reverting an Update Operation
    /// ```rust
    /// let mut repository = Repository::new();
    /// let old_data = Some("previous content".to_string());
    /// let op = SubscribeReposCommitOperation::Update { path: "file.txt".to_string(), cid: "abc123".to_string(), prev_data: old_data };
    /// repository.invert(&op).expect("Inversion should succeed");
    /// ```
    ///
    /// ## Reverting a Delete Operation
    /// ```rust
    /// let mut repository = Repository::new();
    /// let prev_data = Some("deleted content".to_string());
    /// let op = SubscribeReposCommitOperation::Delete { path: "file.txt".to_string(), prev_data };
    /// repository.invert(&op).expect("Inversion should succeed");
    /// ```
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

    /// Computes and returns the root CID (Content Identifier) of the tree structure.
    ///
    /// This method attempts to lazily retrieve a previously computed CID if the tree is marked as a 
    /// stub and not marked as dirty (i.e., it has not been modified since the last computation). 
    /// If a valid CID is available in this state, it is returned without further computation.
    ///
    /// If the tree is marked as dirty or if the CID is not available, the method will recursively 
    /// encode the tree nodes to compute the CID. This process involves writing data blocks associated
    /// with the tree in order to generate the CID.
    ///
    /// ### Side Effects:
    /// - The tree's "dirty" state is cleared (i.e., any dirty flags are reset).
    ///
    /// ### Returns:
    /// - `Ok(Cid)` if the root CID is successfully computed or retrieved.
    /// - `Err(InvertError)` if an error occurs during the encoding or computation process.
    ///
    /// ### Examples:
    /// ```rust
    /// let mut tree = MyTree::new();
    /// // Modify the tree, compute, or retrieve its root CID.
    /// let cid = tree.root()?;
    /// println!("Root CID: {}", cid);
    /// ```
    ///
    /// ### Note:
    /// - This method is critical for persistently representing the state of the tree.
    /// - Ensure the tree data structure is properly initialized before calling this method.
    ///
    /// # Errors:
    /// This function returns `InvertError` if an issue arises during the `write_blocks` operation.
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

    /// Writes all blocks for a tree-like data structure, recursively ensuring that child nodes are written
    /// before their parent nodes. This process computes and sets the Content Identifier (CID) for the current
    /// node and its children.
    ///
    /// # Behavior
    ///
    /// - If the tree is partially valid or incomplete (indicated by `self.stub`), the method will immediately
    ///   return with an `InvertError::InvalidTree`.
    /// - The method traverses all child nodes defined in `self.entries` and recursively calls `write_blocks`
    ///   on any child that is marked as "dirty" or has a child marked as "dirty".
    /// - When a node is successfully processed, its CID is computed using the SHA-256 hashing algorithm and
    ///   the `DagCborCodec`.
    ///
    /// # Returns
    ///
    /// - On success, returns the computed `Cid` for the current node.
    /// - On failure, propagates errors encountered during serialization, multihash wrapping, or child node processing.
    ///
    /// # Errors
    ///
    /// - Returns `InvertError::InvalidTree` if the method is called on a stub tree.
    /// - Propagates serialization or hashing errors if they occur during the computation of the CID.
    ///
    /// # Side Effects
    ///
    /// - Clears the "dirty" flag on each processed node or entry.
    /// - Updates the `cid` of the node and its children, if applicable.
    /// - Computes and sets the current node's CID.
    ///
    /// # Example
    ///
    /// ```rust
    /// // Before calling `write_blocks`, ensure your tree structure is properly populated.
    /// match tree.write_blocks() {
    ///     Ok(cid) => println!("CID for the tree: {}", cid),
    ///     Err(err) => eprintln!("Failed to write tree blocks: {:?}", err),
    /// }
    /// ```
    ///
    /// # Implementation Details
    ///
    /// - The method first processes all child nodes present in `self.entries` before calculating the current
    ///   node's CID.
    /// - For `Value` entries, the "dirty" flag is cleared but no further processing is performed.
    /// - For `Child` entries:
    ///   - If the child exists and is either dirty or has a dirty child, `write_blocks` is called recursively.
    ///   - The computed CID of the child replaces the existing CID in the entry.
    ///   - The "dirty" flag is reset.
    /// - The current node's data (`NodeData`) is serialized using `DagCborCodec` and hashed with SHA-256 to compute its CID.
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

    /// Inserts a value into the tree at the specified path with a given height.
    /// If the key already exists with the exact value, the operation is a no-op, the tree is not marked as dirty, and the value is returned as the previous value.
    ///
    /// # Arguments
    ///
    /// * `path` - The string path representing the key where the value will be inserted or updated.
    /// * `val` - The value (of type `Cid`) to be inserted or updated at the specified key.
    /// * `height` - The height of the tree at which the key resides. If it's negative, the height will be calculated based on the key.
    ///
    /// # Returns
    ///
    /// A `Result` that contains:
    /// * `Ok(Some(Cid))` - If the operation was an update and the key already had a value, the previous value is returned.
    /// * `Ok(None)` - If the operation was an insert of a new key or involved tree restructuring.
    /// * `Err(InvertError)` - If there was an error, such as attempting to manipulate a partial tree.
    ///
    /// # Behavior
    ///
    /// 1. If the node is a stub (i.e., a placeholder for an uninitialized or incomplete subtree), an error `InvertError::PartialTree` is returned.
    /// 2. If the height is less than `0`, it is recalculated based on the key.
    /// 3. If the height of the key exceeds the tree's height, a parent node might need to be added, which could involve splitting the current node.
    /// 4. If the height is less than the current node's height, the `insert_child` function is called to descend further.
    /// 5. If the height matches the current tree height, it performs the following:
    ///    - Locates the key using `find_value`.
    ///    - If the key already exists with the same value, the operation is skipped, and the value is returned as a no-op.
    ///    - If the key exists with a different value, it updates the key and marks the tree as dirty, returning the previous value.
    ///    - If the key does not exist, it finds the appropriate position for insertion using `find_insertion_index`.
    ///    - Handles tree splits when necessary, restructuring nodes as required.
    /// 6. Includes a covering proof for the mutation process through `prove_mutation`.
    ///
    /// # Errors
    ///
    /// The following errors may be encountered:
    /// * `InvertError::PartialTree` - When attempting to perform an operation on a stub or incomplete tree structure.
    /// * Errors returned by `find_insertion_index` or `prove_mutation`.
    ///
    /// # Implementation Details
    ///
    /// - The function modifies the tree structure when necessary (e.g., splitting nodes, adding parent nodes).
    /// - Updates to existing values mark those nodes, and the entire tree, as dirty to reflect the modification.
    /// - Uses helper functions like `insert_parent`, `insert_child`, `find_value`, `find_insertion_index`, and `prove_mutation` for modularity.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use your_module::{YourTree, Cid, InvertError};
    ///
    /// let mut tree = YourTree::new();
    /// let path = "/some/key";
    /// let value = Cid::new();
    /// let height = 3;
    ///
    /// match tree.insert(path, value, height) {
    ///     Ok(Some(prev)) => println!("Updated key with previous value: {:?}", prev),
    ///     Ok(None) => println!("Inserted a new key-value pair."),
    ///     Err(e) => println!("Error occurred: {:?}", e),
    /// }
    /// ```
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

    /// Inserts a parent node into the tree structure at the specified path with the given value
    /// (`Cid`) and height.
    ///
    /// # Parameters
    ///
    /// - `path`: The string slice representing the path at which the parent node should be inserted.
    /// - `val`: The `Cid` value to be inserted into the node.
    /// - `height`: The height of the parent node being inserted, represented as an `i8`.
    ///
    /// # Returns
    ///
    /// Returns a `Result` containing:
    /// - `Ok(Some<Cid>)`, indicating a successful insertion and an optional `Cid` if a value was replaced.
    /// - `Ok(None)`, if the insertion was successful without replacing any value.
    /// - `Err(InvertError)`, if an error occurred during the insertion process.
    ///
    /// # Behavior
    ///
    /// - If the current node is empty (`self.entries.is_empty()`), the node is replaced directly
    ///   with default values, the `dirty` flag is set to `true`, and the specified `height` is applied.
    ///
    /// - If the current node is not empty, a new layer is pushed onto the tree:
    ///     - A temporary node (`this`) is created, containing the current node's height incremented by 1.
    ///     - The current node is swapped with the temporary node.
    ///     - The current node then appends a new child entry with default values (`NodeEntry::Child`).
    ///
    /// - The method concludes by invoking the `insert` method to handle the actual insertion
    ///   of the value at the specified path. Any necessary "splits" in the tree will be
    ///   handled by the `insert` method.
    ///
    /// # Errors
    ///
    /// This function may return an `InvertError` if the insertion fails. The exact nature
    /// of the error depends on the specific implementation of the `insert` method.
    ///
    /// # Example
    ///
    /// ```
    /// // Assuming `tree` is a mutable instance of a tree-like structure.
    /// let mut tree = Tree::new(); // Assuming Tree::new() initializes a default tree structure.
    /// let path = "example_path";
    /// let height = 2;
    /// let cid = Cid::new(); // Assuming Cid::new() generates a valid Cid.
    ///
    /// match tree.insert_parent(path, cid, height) {
    ///     Ok(Some(replaced_cid)) => {
    ///         println!("Insertion successful; replaced CID: {:?}", replaced_cid);
    ///     }
    ///     Ok(None) => {
    ///         println!("Insertion successful; no value was replaced.");
    ///     }
    ///     Err(e) => {
    ///         eprintln!("Insertion failed with error: {:?}", e);
    ///     }
    /// }
    /// ```
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


    /// Inserts a child node into the current node's entries based on the specified path.
    ///
    /// This function performs the following operations:
    /// 1. Checks if there is an existing child node at the specified path.
    ///    - If a matching child exists, delegates the insertion to that child node.
    ///    - If the value being inserted already exists, it is treated as a no-op and returns the value.
    /// 2. If no such child exists, determines the appropriate insertion index for a new child node.
    ///    - Creates a new child node and inserts it into the current node's entries.
    ///    - Handles the scenario where the child is not a direct child, potentially resulting in a recursive insertion.
    ///
    /// # Arguments
    /// - `path`: A string slice containing the key path for the child node.
    /// - `val`: A `Cid` representing the value to be inserted.
    /// - `height`: An `i8` specifying the height of the node being inserted (used to ensure the tree height correctness).
    ///
    /// # Returns
    /// - `Ok(Some(Cid))`: If a value already existed at the path and was replaced, returns the previous value.
    /// - `Ok(None)`: If a new child node was successfully inserted.
    /// - `Err(InvertError)`: If an error occurs during the insertion, such as:
    ///     - `InvertError::PartialTree`: Indicates a partial tree structure during the insertion attempt.
    ///     - `InvertError::UnexpectedSplit`: Indicates an unexpected split operation during insertion.
    ///
    /// # Errors
    /// - Returns `InvertError::PartialTree` if the method attempts to insert into a partial tree where the associated child node is `None`.
    /// - Returns `InvertError::UnexpectedSplit` if an unexpected split occurs when finding the insertion index.
    ///
    /// # Panics
    /// - This function may panic if the internal node structure encounters an unexpected state where an index does not resolve to a valid child node type (`NodeEntry::Child`).
    ///
    /// # Example
    /// ```rust
    /// let mut node = YourNodeType::default();
    /// let path = "/example/path";
    /// let value = SomeCidValue; // Replace with an actual Cid instance.
    ///
    /// match node.insert_child(path, value, 1) {
    ///     Ok(Some(prev)) => println!("Replaced existing value: {:?}", prev),
    ///     Ok(None) => println!("Inserted new child successfully."),
    ///     Err(err) => eprintln!("Error during insertion: {:?}", err),
    /// }
    /// ```
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

    ///
    /// Splits the current node at the given `path`, returning two new nodes resulting
    /// from the split. The method ensures that the tree remains balanced after the operation.
    ///
    /// # Parameters
    /// - `self`: The node instance to split. Ownership of the instance assumed by the function.
    /// - `path`: A string slice (`&str`) which determines the split point.
    ///
    /// # Returns
    /// - `Result<(Self, Self), InvertError>`: On success, returns a tuple containing two
    ///   new nodes (`Self`) - the left and right nodes after the split. If the operation
    ///   fails, an `InvertError` is returned.
    ///
    /// # Errors
    /// - `InvertError::EmptySplit`: Returned if the current node does not contain any entries
    ///   (i.e., an empty node).
    /// - `InvertError::PartialTree`: Returned if a recursive split is attempted on a child node
    ///   that is not fully resolved (e.g., it is `None`).
    /// - Other errors propagated from the `find_insertion_index` method.
    ///
    /// # Behavior
    /// 1. If the node is empty, the function immediately returns an `InvertError::EmptySplit`.
    /// 2. Determines the insertion index in the node's entries for the split point using
    ///    `find_insertion_index`.
    ///     - If `split` is `false`, performs a simple split based on the calculated index
    ///       by delegating to `split_entries`.
    /// 3. If recursive splitting is required (`split == true`):
    ///     - Splits the entries into left and right parts at the calculated index.
    ///     - Recursively splits a child node (`NodeEntry::Child`).
    ///     - Reconstructs the left and right nodes with updated entries and children.
    /// 4. Constructs new nodes with the split results and marks them as dirty for further processing.
    ///
    /// # Examples
    /// ```rust
    /// let path = "example/split/path";
    ///
    /// match node.split(path) {
    ///     Ok((left, right)) => {
    ///         // Handle the split nodes
    ///         println!("Split successful!");
    ///     },
    ///     Err(e) => {
    ///         // Handle the error
    ///         eprintln!("Split failed: {:?}", e);
    ///     },
    /// }
    /// ```
    ///
    /// # Notes
    /// - The method includes a defensive check for empty nodes, which might
    ///   be removed in future optimizations.
    /// - Recursive splits modify child nodes and create new wrapper nodes with updated entries.
    /// - The returned nodes inherit the height of the original node, maintaining the tree structure.
    ///

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

    /// Splits the current structure's `entries` into two parts at the given index and returns them
    /// as two separate instances of the same structure. If the index provided is invalid or results
    /// in an empty part, an error is returned.
    ///
    /// # Parameters
    ///
    /// * `idx` - The index at which the `entries` should be split. Must be between 1 and `len - 1`
    ///   (exclusive), where `len` is the total number of entries.
    ///
    /// # Returns
    ///
    /// * `Ok((Self, Self))` - A tuple containing two instances of the structure:
    ///   - `left`: Entries from the beginning up to (but not including) the split index.
    ///   - `right`: Entries from the split index to the end.
    /// * `Err(InvertError)` - Returns an error if the index is invalid or if it causes one of the
    ///   resulting parts to become empty:
    ///   - `InvertError::SplittingEnds`: If `idx` is 0 or exceeds the maximum valid index.
    ///   - `InvertError::SplitEmptyLegs(idx, len)`: If the resulting `left` or `right` side is empty.
    ///
    /// # Behavior
    ///
    /// * The `entries` of the current structure are split into two separate collections.
    /// * Both resulting structures maintain the same `height` as the original, and are marked as
    ///   `dirty`. Other properties of the instances are reset to their default values unless
    ///   explicitly defined during initialization.
    ///
    /// # Examples
    ///
    /// ```rust
    /// // Assuming `InvertError` and the structure are properly defined:
    /// match structure.split_entries(2) {
    ///     Ok((left, right)) => {
    ///         assert_eq!(left.entries, vec![1, 2]);
    ///         assert_eq!(right.entries, vec![3, 4, 5]);
    ///     }
    ///     Err(e) => panic!("Split failed: {:?}", e),
    /// }
    /// ```
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

    /// Removes a child node from the tree at a given path and height.
    ///
    /// This function attempts to locate and remove a child node identified by the provided
    /// `path`. It operates recursively, descending into the tree to find the child node
    /// and performing the removal operation. If the target node is empty after removal,
    /// it is also removed from the tree structure.
    ///
    /// # Parameters
    ///
    /// - `&mut self`: A mutable reference to the current node.
    /// - `path: &str`: The path to the child node that needs to be removed.
    /// - `height: i8`: The height of the current node relative to the tree structure.
    ///
    /// # Returns
    ///
    /// - `Ok(Some(Cid))`: If a child node is successfully removed, the function returns
    ///   the `Cid` of the removed node wrapped in `Some`.
    /// - `Ok(None)`: If the specified `path` does not exist in the tree or the removal
    ///   results in no changes, `None` is returned.
    /// - `Err(InvertError)`: An error is returned in cases where the operation cannot
    ///   be completed, example being:
    ///     - The tree structure is incomplete or partially defined (`InvertError::PartialTree`).
    ///
    /// # Workflow
    ///
    /// 1. The method looks up the index of the child node by searching for the `path`
    ///    in the current node's entries.
    /// 2. If the child pointer is not found, it means the key is not in the tree, and
    ///    the function immediately returns `Ok(None)`.
    /// 3. If the child pointer is found but the tree is incomplete, the function returns
    ///    an error (`InvertError::PartialTree`).
    /// 4. If the specified child node does not exist or is already removed, the function
    ///    returns `Ok(None)`.
    /// 5. If the child node is found and successfully removed:
    ///     - The function marks the current node as "dirty."
    ///     - If the child node is still non-empty after the operation, the function returns
    ///       `Some(Cid)` indicating the prior state.
    ///     - If the child node becomes empty, it is removed from the current node's
    ///       entry list and `Some(Cid)` is still returned.
    /// 6. In edge cases where the tree structure is inconsistent due to an implementation
    ///    error, the code uses the `unreachable!()` macro to assert unreachable states.
    ///
    /// # Debugging
    ///
    /// - Assertions ensure that modified child nodes are marked as dirty before returning.
    /// - Assumptions about tree consistency rely on correct logic elsewhere in the implementation.
    ///
    /// # Errors
    ///
    /// - The function returns an `InvertError::PartialTree` if the tree is incomplete
    ///   and an intermediate node does not provide the necessary information to proceed.
    ///
    /// # Notes
    ///
    /// - Modification of the tree's structure (such as removing entries) will mark the node
    ///   as dirty for future operations, signaling that the tree state has changed.
    ///
    /// # Example
    ///
    /// ```
    /// // Assuming tree and path are properly initialized
    /// let result = tree.remove_child("some/path", 3);
    /// match result {
    ///     Ok(Some(cid)) => println!("Child removed with CID: {:?}", cid),
    ///     Ok(None) => println!("No child found at the specified path."),
    ///     Err(err) => eprintln!("Error during removal: {:?}", err),
    /// }
    /// ```
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

    /// Merges the entries of another instance with the same type into the current instance.
    ///
    /// # Arguments
    ///
    /// * `other` - Another instance of the same type whose entries should be merged
    ///              into the current instance.
    ///
    /// # Returns
    ///
    /// * `Ok(())` - If the merging process completes successfully.
    /// * `Err(InvertError)` - If an error occurs during the merging process.
    ///
    /// # Details
    ///
    /// This method takes another instance of the type (`other`) and appends its entries
    /// to the current instance's entries. It sets the `dirty` flag to `true` and
    /// maintains the height of the current instance.
    ///
    /// If the last entry in the original entries (`self.entries[idx - 1]`) and the first entry
    /// of the `other` instance (`self.entries[idx]`) represent child nodes, the method
    /// recursively merges their respective child entries. This ensures that the hierarchical
    /// structure of the entries is maintained.
    ///
    /// # Errors
    ///
    /// This method might return an `InvertError` during recursive merging if there are issues
    /// accessing or modifying child nodes in the entries.
    ///
    /// # Example
    ///
    /// ```rust
    /// let mut instance1 = MyType::default();
    /// let instance2 = MyType::default();
    /// instance1.merge(instance2).expect("Merge failed");
    /// ```
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

    /// Attempts to prove a mutation within a structured data tree, making it possible to generate
    /// invertible operation diffs. This method traverses the tree, checking if the provided `path`
    /// aligns with or falls within a node or sub-tree. The traversal operates in a depth-first manner
    /// to ensure all relevant nodes are inspected.
    ///
    /// # Arguments
    ///
    /// * `path` - A string slice representing the path to check within the tree structure. The path
    /// is compared against keys in the tree to determine the proper location and outcome.
    ///
    /// # Returns
    ///
    /// * `Ok(())` - If the mutation can be proven or if the relevant operation concludes successfully.
    /// * `Err(InvertError::PartialTree)` - If the tree structure is incomplete at any point where
    /// deeper traversal is necessary.
    ///
    /// # Behavior
    ///
    /// 1. Iterates through the `entries` of the current node in the tree.
    /// 2. Matches each `NodeEntry` to determine its type:
    ///    - **`NodeEntry::Value`**: Compares its `key` with the `path`. If the `path` is before
    ///      the key's value in lexicographical order, the function returns `Ok(())` as it means no
    ///      further traversal is needed.
    ///    - **`NodeEntry::Child`**: If a later value exists, compares the `path` to decide whether
    ///      to skip the child or not. If traversal into the child is required, checks the specific
    ///      sub-tree's comparison result:
    ///        - **`Ordering::Less`**: Returns `Ok(())` if the `path` is less than this sub-tree in
    ///          lexicographical order.
    ///        - **`Ordering::Equal`**: Recursively calls `prove_mutation` on the child tree.
    ///        - **`Ordering::Greater`**: Continues the loop, inspecting the next entry.
    /// 3. Returns `Err(InvertError::PartialTree)` if an expected child node or sub-tree is missing.
    /// 4. Resolves successfully with `Ok(())` if no conflicting or incomplete conditions are encountered.
    ///
    /// # Errors
    ///
    /// This function can fail with `InvertError::PartialTree` if the traversal encounters a malformed
    /// or partial tree structure, specifically when a required child is missing.
    ///
    /// # Panics
    ///
    /// This function panics with `unreachable!()` if an unexpected case occurs where a `NodeEntry::Child`
    /// pattern match is invalid. This should not happen under normal operation.
    ///
    /// # Usage
    ///
    /// The primary purpose of this function is within a tree-like data structure, where path-based
    /// mutation proofs and invertible diffs are required. It can be used to validate, calculate, or
    /// represent differences for synchronization or versioning tasks.
    ///
    /// Example (pseudo-contextual):
    /// ```text
    /// let result = tree.prove_mutation("/some/path");
    /// match result {
    ///     Ok(()) => println!("Mutation proven successfully."),
    ///     Err(e) => eprintln!("Error: Failed to prove mutation: {:?}", e),
    /// }
    /// ```
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

    /// Compares the provided key (path) against the keys in this node, and sets the `dirty` flag
    /// on this node and any child nodes traversed during the comparison.
    ///
    /// This method determines the relative ordering of the provided key with respect to the keys
    /// stored in the current node and any relevant child nodes. It handles cases where the node
    /// is empty, where the key is less than, greater than, or equal to the range of keys stored
    /// in the node, and also recursively checks child nodes when needed.
    ///
    /// # Arguments
    /// * `path` - A string slice representing the key to compare to the node's keys.
    ///
    /// # Returns
    /// * `Ok(Ordering::Less)` - Returns `Ordering::Less` if the provided key is lower than
    ///   the range of keys in this node or relevant children.
    /// * `Ok(Ordering::Greater)` - Returns `Ordering::Greater` if the key is higher than
    ///   the range of keys in this node or relevant children.
    /// * `Ok(Ordering::Equal)` - Returns `Ordering::Equal` if the path is within the range
    ///   of the node's keys or relevant children.
    /// * `Err(InvertError::PartialTree)` - If the node or any required child node is a stub
    ///   (incomplete or missing part of the tree).
    /// * `Err(InvertError::EmptyTreeNode)` - If the current node contains no entries.
    ///
    /// # Note
    /// - The `dirty` flag is set on both the current node and any traversed child nodes during the operation.
    /// - The method raises the `InvertError::PartialTree` or `InvertError::EmptyTreeNode` errors
    ///   in cases where incomplete data prevents the comparison.
    /// - If the node contains no entries, this method will return an error, though there is a TODO
    ///   indicating potential reconsideration of behavior in this case.
    ///
    /// # Behavior
    /// - If the `path` key is less than all entries in this node, `Ordering::Less` is returned.
    /// - If the `path` key is greater than all entries in this node, `Ordering::Greater` is returned.
    /// - If the `path` key falls between entries or matches an entry, `Ordering::Equal` is returned.
    /// - The method may recurse into child nodes to verify the relative order of the key.
    ///
    /// # Errors
    /// - Returns an `InvertError::PartialTree` if the node is a stub or an incomplete state.
    /// - Returns an `InvertError::EmptyTreeNode` when no entries exist in the node.
    ///
    /// # Implementation Details
    /// The method first checks boundary conditions to see if the key is less than or greater than
    /// the range of the node's keys. If neither condition is met, it goes through all entries in
    /// the node, comparing the key to determine ordinality. For child entries, it may recurse into
    /// the child node if necessary to resolve the ordering.
    ///
    /// # Example
    /// ```rust
    /// let mut node = Node {
    ///     entries: vec![
    ///         NodeEntry::Value { key: "abc".to_string(), value: Some(value) },
    ///         NodeEntry::Child { child: Some(Box::new(child_node)), range: ... }
    ///     ],
    ///     stub: false,
    ///     dirty: false,
    /// };
    /// let result = node.compare_key("bcd");
    /// assert!(matches!(result, Ok(Ordering::Greater | Less | Equal)));
    /// ```
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

    /// Finds the appropriate insertion index for a specified key within the current tree structure.
    ///
    /// This function determines the correct index at which a new entry (key) should be inserted into
    /// the tree's entries. It also identifies whether the key belongs inside a sub-tree of an existing
    /// child node. The process ensures that the tree maintains proper ordering during insertion.
    ///
    /// ### Behavior:
    /// - If the key "splits" an existing child entry (falls within a specific child sub-tree),
    ///   the index of that child and a flag indicating "inside the child" (true) are returned.
    /// - If the key should be appended (falls outside all existing entries),
    ///   the index returned will be one higher than the current largest index, and the flag will be false.
    /// - If the tree is in a stub state, this function will return an error.
    ///
    /// ### Parameters:
    /// - `path`: A `&str` representing the key whose insertion position is being determined.
    ///
    /// ### Returns:
    /// - `Ok((usize, bool))`:
    ///   - `usize`: The index where the key should be inserted.
    ///   - `bool`: A flag that is `true` if the key falls inside a sub-tree of an existing child node,
    ///     or `false` otherwise.
    /// - `Err(InvertError)`: An error in case of invalid insertion order under certain conditions (e.g., when
    ///   working with a partial tree).
    ///
    /// ### Error Handling:
    /// - Returns `InvertError::PartialTreeInsertionOrderError` if the method is called on an incomplete
    ///   (stub) tree or invalid operations are performed during the search.
    ///
    /// ### Example:
    /// ```rust
    /// let mut tree = /* initialize tree */;
    /// let result = tree.find_insertion_index("key_to_insert");
    /// match result {
    ///     Ok((index, inside_child)) => {
    ///         if inside_child {
    ///             println!("The key should be inserted inside a child at index {}", index);
    ///         } else {
    ///             println!("The key should be inserted at index {}", index);
    ///         }
    ///     }
    ///     Err(e) => eprintln!("Error occurred: {:?}", e),
    /// }
    /// ```
    ///
    /// ### Notes:
    /// - The process sequentially iterates through the entries in the tree to locate the correct position
    ///   for the provided key. It handles both value and child entries.
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

    /// Searches for a value within the entries of the current node that matches the specified `path`.
    ///
    /// This function iterates over the entries in the node and checks if any entry is of type
    /// `NodeEntry::Value` with a `key` matching the provided `path`. If a match is found,
    /// the index of that entry is returned. If no match is found, it returns `None`.
    ///
    /// # Parameters
    /// - `path`: A string slice that represents the path to search for within the node entries.
    ///
    /// # Returns
    /// - `Some(usize)`: The index of the entry in `self.entries` where the match was found.
    /// - `None`: If no entry matches the provided `path`.
    ///
    /// # Examples
    /// ```
    /// let node = Node {
    ///     entries: vec![
    ///         NodeEntry::Value { key: b"sample".to_vec(), value: 42 },
    ///         NodeEntry::Child { child_index: 1 }
    ///     ],
    /// };
    ///
    /// assert_eq!(node.find_value("sample"), Some(0));
    /// assert_eq!(node.find_value("missing"), None);
    /// ```
    fn find_value(&self, path: &str) -> Option<usize> {
        for (i, entry) in self.entries.iter().enumerate() {
            match entry {
                // TODO: could skip early if e.Key is lower
                // can potentially optimize this by checking if key is lower than the current key
                // if so, we can skip the rest of the entries since we know the entries are sorted
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

    /// Loads a node from a `BlockMap` using its content identifier (CID).
    ///
    /// This function attempts to retrieve and reconstruct a node and its child nodes from the provided block map,
    /// decoding the data and creating the necessary hierarchy. If the CID does not correspond to an entry in the
    /// block map, it returns `Ok(None)`. If any parsing errors occur during deserialization, it returns a `ParseError`.
    ///
    /// # Parameters
    /// - `block_map`: A reference to the `BlockMap` which stores the blocks of data, indexed by their CIDs.
    /// - `cid`: The content identifier (CID) that identifies the block to load.
    ///
    /// # Returns
    /// - `Ok(Some(Self))` if the node and its hierarchy were successfully loaded and reconstructed.
    /// - `Ok(None)` if the block corresponding to the CID was not found (allowing "partial" trees to exist).
    /// - `Err(ParseError)` if an error occurred while deserializing or processing the block data.
    ///
    /// # Behavior
    /// - The function starts by checking if the CID exists in the `block_map`. If the block is not found, it
    ///   returns `Ok(None)`.
    /// - If the block is found, it deserializes the block into `NodeData` using `serde_ipld_dagcbor`.
    /// - The `NodeData` is then converted into a `Node`, and each `NodeEntry` is inspected:
    ///   - If the entry corresponds to a child node (with a valid CID), it recursively attempts to load the child
    ///     node.
    ///   - If a child node is successfully loaded, the function adjusts the parent node's height based on the
    ///     height of the child (if applicable) and assigns the loaded child to the parent.
    /// - Finally, it returns the reconstructed node wrapped in `Some`.
    ///
    /// # Notes
    /// - The logic includes a note about handling `height` values. Specifically, if the current node's height
    ///   is less than zero and the child's height is valid (non-negative), the parent node's height is adjusted
    ///   to be one greater than the child's height. This approach is described as "kind of a hack," suggesting
    ///   it works within certain constraints but might benefit from further refactoring or consideration in
    ///   the overall design.
    ///
    /// # Errors
    /// - Returns a `ParseError` if deserialization from the block data using `serde_ipld_dagcbor` fails.
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
    /// Converts `self` into a `Node` object using a given `cid`.
    ///
    /// This method constructs a `Node` from the data within `self`,
    /// initializing the node's entries and height based on the provided data.
    /// Each entry in `self` is transformed into either a `Value` or `Child` entry,
    /// and the constructed node is returned.
    ///
    /// # Parameters
    ///
    /// * `self` - The current instance that contains entries and possible children,
    ///             which will be converted into a `Node`.
    /// * `cid` - The content identifier (`Cid`) to be associated with the resulting `Node`.
    ///
    /// # Returns
    ///
    /// A `Node` object comprising:
    /// * A list of `entries` that contain transformed data from `self`,
    ///   where each entry is either:
    ///   - A `Value` with a key, value, and its dirty state.
    ///   - A `Child`, representing a child CID and additional metadata (child, dirty flag).
    /// * The `height` of the node, computed based on its keys, or left to be set
    ///   later.
    /// * `dirty` flag set to `false`.
    /// * The `cid` parameter passed into the method.
    ///
    /// # Details
    ///
    /// 1. Initializes the `entries` vector with the capacity of the number of entries in `self`.
    /// 2. Adds a `Child` entry with the left child (if any) to the beginning of the `entries` list.
    /// 3. Iterates through each entry in `self.entries`, rebuilding the `key` for
    ///    `NodeEntry::Value` by concatenating the `prefix_len` portion from the previous key
    ///    and the `key_suffix`.
    /// 4. Creates a `Value` entry for each valid key-value pair, and appends it to `entries`.
    /// 5. Appends a `Child` entry for the right child (if any) of the current entry after the `Value`.
    /// 6. Updates `height` using the first key's value or leaves it unset (negative).
    ///    A height adjustment mechanism (`ensure_heights`) should be used after creating intermediate nodes.
    ///
    /// # Warnings
    ///
    /// * `height` may not be set correctly for intermediate nodes due to sequential
    ///   processing of entries. Ensure `ensure_heights` is called after converting to `Node`
    ///   to correct the heights of such nodes.
    /// * Assumes that `entries` in `self` are properly structured for reconstruction during iteration.
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

    /// Converts a `Node` into a specific data structure (`Self`) by processing its entries and organizing
    /// them into a vector of `EntryData`. This function handles both value and child entries within the node
    /// and ensures the resulting structure has the appropriate relationships inferred from the node's contents.
    ///
    /// # Arguments
    ///
    /// * `node` - A reference to a `Node` that will be converted into the target structure.
    ///
    /// # Returns
    ///
    /// * `Ok(Self)` - Returns the constructed instance of `Self` if the conversion is successful.
    /// * `Err(InvertError::MalformedTreeNode)` - Returns an error if the node structure is found to be invalid
    ///   (e.g., a child entry is encountered without a preceding value entry).
    ///
    /// # Steps:
    ///
    /// 1. Creates an initial structure with an empty left child pointer and a pre-allocated vector for entries.
    /// 2. Iterates over `node.entries` to process each `NodeEntry`:
    ///    - For `NodeEntry::Value`:
    ///       - Calculates the `prefix_len` (shared prefix length) between the current entry's key and the previous key.
    ///       - Constructs an `EntryData` with the suffix (remainder of the key after the common prefix), the value,
    ///         and a `none` right child.
    ///       - Updates the `prev` key reference.
    ///    - For `NodeEntry::Child`:
    ///       - Assigns the CID of the child to the `left` property if it is the first entry.
    ///       - If it's a future child, ensures it's linked to the last `EntryData` created.
    ///       - If no preceding value entry exists, returns an `InvertError::MalformedTreeNode`.
    /// 3. Returns the constructed data structure if no errors are encountered.
    ///
    /// # Errors
    ///
    /// * Returns `InvertError::MalformedTreeNode` if there is a child entry (`NodeEntry::Child`) without a preceding
    ///   value entry (`NodeEntry::Value`), indicating malformed input.
    ///
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
    /// # Notes
    /// IPLD stands for Interplanetary Linked Data
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let ipld = Ipld::deserialize(d)?;
        let Ipld::Bytes(key_suffix) = ipld else {
            return Err(D::Error::custom(format!("expected ipld bytes, got: {:?}", ipld.kind())));
        };
        Ok(key_suffix)
    }
}
