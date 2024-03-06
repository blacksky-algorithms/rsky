use libipld::Cid;
use crate::common::ipld;
use anyhow::Result;
use diesel::PgConnection;
use crate::{common, storage};
use crate::repo::block_map::BlockMap;

// treeEntry are elements of nodeData's Entries.
#[derive(Debug, Deserialize, Serialize)]
pub struct TreeEntry {
    pub p: u8, // count of characters shared with previous path/key in tree
    pub k: Vec<u8>, // remaining part of path/key (appended to "previous key")
    pub v: Cid, // CID pointer at this path/key
    pub t: Option<Cid> // [optional] pointer to lower-level subtree to the "right" of this path/key entry
}

// MST tree node as gets serialized to CBOR. Note that the CBOR fields are all
// single-character.
#[derive(Debug, Deserialize, Serialize)]
pub struct NodeData {
    pub l: Option<Cid>, // [optional] pointer to lower-level subtree to the "left" of this path/key
    pub e: Vec<TreeEntry> // ordered list of entries at this node
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Leaf {
    pub key: String, // record key
    pub value: Cid
}

// nodeEntry is a node in the MST.
//
// Following the Typescript implementation, this is basically a flexible
// "TreeEntry" (aka "leaf") which might also be the "Left" pointer on a
// NodeData (aka "tree").
#[derive(Clone)]
pub enum NodeEntry<'a> {
    MST(MST<'a>),
    Leaf(Leaf)
}

impl<'a> NodeEntry<'a> {
    pub fn is_tree(&self) -> bool {
        match self {
            NodeEntry::MST(_) => true,
            _ =>  false
        }
    }

    pub fn is_leaf(&self) -> bool {
        match self {
            NodeEntry::Leaf(_) => true,
            _ =>  false
        }
    }
}

pub struct CidAndBytes {
    pub cid: Cid,
    pub bytes: Vec<u8>
}

pub struct CidAndBlockMap {
    pub cid: Cid,
    pub bytes: Vec<u8>
}

pub struct UnstoredBlocks {
    root: Cid,
    blocks: BlockMap
}

// MST represents a MerkleSearchTree tree node (NodeData type). It can be in
// several levels of hydration: fully hydrated (entries and "pointer" (CID)
// computed); dirty (entries correct, but pointer (CID) not valid); virtual
// (pointer is defined, no entries have been pulled from block store)
//
// MerkleSearchTree values are immutable. Methods return copies with changes.
#[derive(Clone)]
pub struct MST<'a> {
    pub entries: Option<Vec<NodeEntry<'a>>>,
    pub layer: Option<u32>,
    pub pointer: Cid,
    pub outdated_pointer: bool,
    pub conn: &'a PgConnection,
}

impl<'a> MST<'a> {
    pub fn new (
        conn: &PgConnection,
        pointer: Cid,
        entries: Option<Vec<NodeEntry>>,
        layer: Option<u32>
    ) -> MST<'a> {
        MST {
            conn,
            entries,
            layer,
            pointer,
            outdated_pointer: false
        }
    }

    pub fn create (
        conn: &PgConnection,
        entries: Option<Vec<NodeEntry>>,
        layer: Option<u32>
    ) -> Result<MST<'a>> {
        let entries = entries.unwrap_or(Vec::new());
        let pointer = util::cid_for_entries(&entries)?;
        Ok(MST::new(conn, pointer, Some(entries), layer))
    }

    pub fn from_data (
        conn: &mut PgConnection,
        data: NodeData,
        layer: Option<u32>
    ) -> Result<MST<'a>> {
        let entries = util::deserialize_node_data(conn, &data, &layer)?;
        let pointer = ipld::cid_for_cbor(&data)?;
        Ok(MST::new(conn, pointer, Some(entries), layer))
    }

    // This is poorly named in both implementations, because it is lazy
    // this is really a *lazy* load, doesn't actually touch storage
    pub fn load (
        conn: &mut PgConnection,
        cid: Cid,
        layer: Option<u32>
    ) -> Result<MST<'a>> {
        Ok(MST::new(conn, cid, None, layer))
    }

    // Immutability
    // -------------------

    // We never mutate an MST, we just return a new MST with updated values
    pub fn new_tree (
        self,
        entries: Vec<NodeEntry>
    ) -> Result<MST<'a>> {
        let mut mst = MST::new(self.conn, self.pointer, Some(entries), self.layer);
        mst.outdated_pointer = true;
        Ok(mst)
    }

    // === "Getters (lazy load)" ===

    // "We don't want to load entries of every subtree, just the ones we need"
    pub fn get_entries(&mut self) -> Result<&Vec<NodeEntry>> {
        // if we are "hydrated", entries are available
        if let Some(entries) = &self.entries {
            return Ok(entries)
        };
        // otherwise this is a virtual/pointer struct and we need to hydrate from
        // block store before returning entries
        let conn = &mut self.conn;
        let data: NodeData = storage::read_obj(conn, &self.pointer)?;

        // can compute the layer on the first KeySuffix, because
        // for the first entry that field is a complete key
        let leaf = &data.e[0];
        let layer = Some(util::leading_zeros_on_hash(&leaf.k)?);

        self.entries = Some(util::deserialize_node_data(conn, &data, &layer)?);

        Ok(&self.entries.unwrap())
    }

    pub fn get_pointer(&mut self) -> Result<Cid> {
        if !self.outdated_pointer { return Ok(self.pointer); }
        let CidAndBytes { cid, .. } = self.serialize()?;
        self.pointer = cid;
        self.outdated_pointer = false;
        Ok(self.pointer)
    }

    pub fn serialize(&mut self) -> Result<CidAndBytes> {
        let mut entries = self.get_entries()?;
        let mut outdated: Vec<&MST> = entries
            .iter()
            .filter_map(|e| match e {
                NodeEntry::MST(e) if e.outdated_pointer => Some(e),
                _ => None
            })
            .collect::<Vec<_>>();

        if outdated.len() > 0 {
            let _outdated = outdated
                .iter()
                .map(|mut e| e.get_pointer())
                .collect::<Vec<_>>();
            entries = self.get_entries()?;
        }
        let data = util::serialize_node_data(entries)?;
        Ok(CidAndBytes {
            cid: ipld::cid_for_cbor(&data)?,
            bytes: common::struct_to_cbor(data)?
        })
    }

    // In most cases, we get the layer of a node from a hint on creation
    // In the case of the topmost node in the tree, we look for a key in the node & determine the layer
    // In the case where we don't find one, we recurse down until we do.
    // If we still can't find one, then we have an empty tree and the node is layer 0
    pub fn get_layer(&mut self) -> Result<u32> {
        self.layer = self.attempt_get_layer()?;
        if self.layer.is_none() { self.layer = Some(0); }
        Ok(self.layer.unwrap_or(0))
    }

    pub fn attempt_get_layer(&mut self) -> Result<Option<u32>> {
        if self.layer.is_some() { return Ok(self.layer) };
        let entries = self.get_entries()?;
        let mut layer = util::layer_for_entries(&entries)?;
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

    // Return the necessary blocks to persist the MST to repo storage
    pub fn get_unstored_blocks(&mut self) -> Result<UnstoredBlocks> {
        let mut blocks = BlockMap::new();
        let pointer = self.get_pointer()?;
        let conn = &mut self.conn;
        let already_has = storage::has(conn, pointer)?;
        if already_has {
            return Ok(UnstoredBlocks{ root: pointer, blocks });
        }
        let entries = self.get_entries()?;
        let data = util::serialize_node_data(entries)?;
        blocks.add(data)?;
        for entry in entries {
            if let NodeEntry::MST(mut e) = entry {
                let subtree = e.get_unstored_blocks()?;
                blocks.add_map(subtree.blocks)?;
            }
        }
        Ok(UnstoredBlocks {
            root: pointer,
            blocks
        })
    }

    // Adds a new leaf for the given key/value pair
    // Throws if a leaf with that key already exists
    pub fn add (
        &mut self,
        key: String,
        value: Cid,
        known_zeros: Option<u32>
    ) -> Result<MST> {
        util::ensure_valid_mst_key(&key)?;
        let key_zeros: u32;
        if let Some(z) = known_zeros {
            key_zeros = z;
        } else {
            key_zeros = util::leading_zeros_on_hash(&key.into_bytes())?;
        }
        let layer = self.get_layer()?;
        let new_leaf = Leaf { key, value };
        if key_zeros == layer {

        } else if key_zeros < layer {

        } else {

        }
        todo!()
    }

    // Simple Operations
    // -------------------

    // returns entry at index
    pub fn at_index(
        &mut self,
        index: usize
    ) -> Result<Option<&NodeEntry>> {
        let entries = self.get_entries()?;
        Ok(entries.get(index))
    }

    pub fn slice (
        &mut self,
        start: Option<usize>,
        end: Option<usize>
    ) -> Result<&[NodeEntry]> {
        let entries = self.get_entries()?;
        if start.is_some() && end.is_some() {
            Ok(&entries[start.unwrap()..end.unwrap()])
        } else if start.is_some() && end.is_none() {
            Ok(&entries[start.unwrap()..])
        } else if start.is_none() && end.is_some() {
            Ok(&entries[..end.unwrap()])
        } else {
            Ok(&entries[..])
        }
    }

    pub fn splice_in(
        &mut self,
        entry: NodeEntry,
        index: usize
    ) -> Result<MST> {
        let mut update = Vec::new();
        for e in self.slice(Some(0), Some(index))? {
            update.push(e.clone());
        }
        update.push(entry);
        for e in self.slice(Some(index), None)? {
            update.push(e.clone());
        }
        Ok(self.new_tree(update)?)
    }

    // Finding insertion points
    // -------------------

    // finds index of first leaf node that is greater than or equal to the value
    pub fn find_gt_or_equal_leaf_index (
        &mut self,
        key: String
    ) -> Result<usize> {
        let entries = self.get_entries()?;
        let maybe_index = entries
            .into_iter()
            .filter_map(|entry| {
                if let NodeEntry::Leaf(l) = entry {
                    Some(l)
                } else {
                    None
                }
            })
            .position(|entry| entry.key >= key);
        // if we can't find, we're on the end
        if let Some(i) = maybe_index {
            Ok(i)
        } else {
            Ok(entries.len())
        }
    }
}

pub mod util;