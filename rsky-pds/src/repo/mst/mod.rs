use crate::common;
use crate::common::ipld;
use crate::repo::block_map::BlockMap;
use crate::repo::cid_set::CidSet;
use crate::repo::error::DataStoreError;
use crate::repo::parse;
use crate::storage::{Ipld, ObjAndBytes, SqlRepoReader};
use anyhow::{anyhow, Result};
use libipld::Cid;
use serde::Deserialize as DeserializeTrait;
use std::mem;

struct NodeIter<'a> {
    entries: &'a [NodeEntry<'a>], // Contains the remaining children of a node,
    // The iterator of the parent node, if present
    // It must be wrapped in a Box because a struct in Rust can’t contain itself
    // without indirection
    parent: Option<Box<NodeIter<'a>>>,
    this: Option<&'a NodeEntry<'a>>,
}

impl Default for NodeIter<'_> {
    fn default() -> Self {
        NodeIter {
            entries: &[],
            parent: None,
            this: None,
        }
    }
}

// We want to traverse (i.e. iterate over) this kind of tree depth-first. This means that
// when a node has multiple children, we first traverse the first child and all its descendants
// before moving on to the second child.
impl<'a> Iterator for NodeIter<'a> {
    type Item = &'a NodeEntry<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.entries.get(0) {
            // We first check if children is empty. If that’s the case, we try to continue
            // iterating the parent node. If there is no parent node, we return None.
            None => {
                match self.this {
                    Some(NodeEntry::MST(_)) => {
                        let this = self.this;
                        self.this = None;
                        this
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
                let leaf = self.entries.get(0);
                self.entries = &self.entries[1..];
                leaf
            }
            // If it is a NodeEntry::MST, we create a new iterator for the child entries.
            // The parent field is set to self, and self is replaced with the newly created iterator
            Some(NodeEntry::MST(mut subtree)) => {
                let this = self.entries.get(0);
                self.entries = &self.entries[1..];

                // start iterating the child trees
                *self = NodeIter {
                    entries: subtree.get_entries().unwrap().as_slice(),
                    parent: Some(Box::new(mem::take(self))),
                    this,
                };
                self.next()
            }
        }
    }
}

// Alternative implementation of iterator
struct NodeIterReachable<'a> {
    entries: &'a [NodeEntry<'a>],
    parent: Option<Box<NodeIterReachable<'a>>>,
    this: Option<&'a NodeEntry<'a>>,
}

impl Default for NodeIterReachable<'_> {
    fn default() -> Self {
        NodeIterReachable {
            entries: &[],
            parent: None,
            this: None,
        }
    }
}

impl<'a> Iterator for NodeIterReachable<'a> {
    type Item = Result<&'a NodeEntry<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.entries.get(0) {
            None => match self.this {
                Some(NodeEntry::MST(_)) => {
                    let this = self.this.unwrap();
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
                let leaf = self.entries.get(0).unwrap();
                self.entries = &self.entries[1..];
                Some(Ok(leaf))
            }
            Some(NodeEntry::MST(mut subtree)) => {
                let this = self.entries.get(0);
                self.entries = &self.entries[1..];
                let entries = subtree.get_entries();
                match entries {
                    Err(e) => {
                        match e.downcast_ref() {
                            Some(DataStoreError::MissingBlock(_)) => self.next(), // Don't iterate
                            _ => return Some(Err(e)),
                        }
                    }
                    _ => {
                        *self = NodeIterReachable {
                            entries: entries.unwrap().as_slice(),
                            parent: Some(Box::new(mem::take(self))),
                            this,
                        };
                        self.next()
                    }
                }
            }
        }
    }
}

// treeEntry are elements of nodeData's Entries.
#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
pub struct TreeEntry {
    pub p: u8,          // count of characters shared with previous path/key in tree
    pub k: Vec<u8>,     // remaining part of path/key (appended to "previous key")
    pub v: Cid,         // CID pointer at this path/key
    pub t: Option<Cid>, // [optional] pointer to lower-level subtree to the "right" of this path/key entry
}

// MST tree node as gets serialized to CBOR. Note that the CBOR fields are all
// single-character.
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

// nodeEntry is a node in the MST.
//
// Following the Typescript implementation, this is basically a flexible
// "TreeEntry" (aka "leaf") which might also be the "Left" pointer on a
// NodeData (aka "tree").
#[derive(Clone)]
pub enum NodeEntry<'a> {
    MST(MST<'a>),
    Leaf(Leaf),
}

impl<'a> NodeEntry<'a> {
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

    fn iter(&self) -> NodeIter<'_> {
        match self {
            NodeEntry::MST(_) => NodeIter {
                entries: std::slice::from_ref(self),
                parent: None,
                this: Some(self),
            },
            NodeEntry::Leaf(_) => NodeIter {
                entries: std::slice::from_ref(self),
                parent: None,
                this: None,
            },
        }
    }

    fn iter_reachable(&self) -> NodeIterReachable<'_> {
        match self {
            NodeEntry::MST(_) => NodeIterReachable {
                entries: std::slice::from_ref(self),
                parent: None,
                this: Some(self),
            },
            NodeEntry::Leaf(_) => NodeIterReachable {
                entries: std::slice::from_ref(self),
                parent: None,
                this: None,
            },
        }
    }
}

impl<'a> IntoIterator for &'a NodeEntry<'a> {
    type Item = &'a NodeEntry<'a>;

    type IntoIter = NodeIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

pub struct CidAndBytes {
    pub cid: Cid,
    pub bytes: Vec<u8>,
}

type BlockWriter = Vec<CidAndBytes>;

pub struct UnstoredBlocks {
    root: Cid,
    blocks: BlockMap,
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
    pub storage: SqlRepoReader<'a>,
}

impl<'a> MST<'a> {
    pub fn new(
        storage: SqlRepoReader,
        pointer: Cid,
        entries: Option<Vec<NodeEntry>>,
        layer: Option<u32>,
    ) -> MST<'a> {
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
    ) -> Result<MST<'a>> {
        let entries = entries.unwrap_or(Vec::new());
        let pointer = util::cid_for_entries(&entries)?;
        Ok(MST::new(storage, pointer, Some(entries), layer))
    }

    pub fn from_data(
        storage: SqlRepoReader,
        data: NodeData,
        layer: Option<u32>,
    ) -> Result<MST<'a>> {
        let entries = util::deserialize_node_data(&storage, &data, layer)?;
        let pointer = ipld::cid_for_cbor(&data)?;
        Ok(MST::new(storage, pointer, Some(entries), layer))
    }

    // This is poorly named in both implementations, because it is lazy
    // this is really a *lazy* load, doesn't actually touch storage
    pub fn load(storage: SqlRepoReader, cid: Cid, layer: Option<u32>) -> Result<MST<'a>> {
        Ok(MST::new(storage, cid, None, layer))
    }

    // Immutability
    // -------------------

    // We never mutate an MST, we just return a new MST with updated values
    pub fn new_tree(&mut self, entries: Vec<NodeEntry>) -> Result<MST<'a>> {
        let mut mst = MST::new(
            self.storage.clone(),
            self.pointer,
            Some(entries),
            self.layer,
        );
        mst.outdated_pointer = true;
        Ok(mst)
    }

    // === "Getters (lazy load)" ===

    // "We don't want to load entries of every subtree, just the ones we need"
    pub fn get_entries(&mut self) -> Result<&Vec<NodeEntry>> {
        // if we are "hydrated", entries are available
        if let Some(entries) = &self.entries {
            return Ok(entries);
        };
        // otherwise this is a virtual/pointer struct, and we need to hydrate from
        // block store before returning entries
        let data: NodeData = self
            .storage
            .read_obj(&self.pointer, |obj| matches!(obj, Ipld::Node(_)))?
            .node();

        // can compute the layer on the first KeySuffix, because
        // for the first entry that field is a complete key
        let leaf = &data.e[0];
        let layer = Some(util::leading_zeros_on_hash(&leaf.k)?);

        self.entries = Some(util::deserialize_node_data(&self.storage, &data, layer)?);

        Ok(&self.entries.clone().unwrap())
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
        let mut outdated: Vec<&MST> = entries
            .iter()
            .filter_map(|e| match e {
                NodeEntry::MST(e) if e.outdated_pointer => Some(e),
                _ => None,
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
            bytes: common::struct_to_cbor(data)?,
        })
    }

    // In most cases, we get the layer of a node from a hint on creation
    // In the case of the topmost node in the tree, we look for a key in the node & determine the layer
    // In the case where we don't find one, we recurse down until we do.
    // If we still can't find one, then we have an empty tree and the node is layer 0
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
        let already_has = self.storage.has(pointer)?;
        if already_has {
            return Ok(UnstoredBlocks {
                root: pointer,
                blocks,
            });
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
            blocks,
        })
    }

    // Adds a new leaf for the given key/value pair
    // Throws if a leaf with that key already exists
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
                    NodeEntry::Leaf(_) => Ok(self.splice_in(NodeEntry::Leaf(new_leaf), index)?),
                    // else we try to split the subtree around the key
                    NodeEntry::MST(mut m) => {
                        let split_sub_tree = m.split_around(key)?;
                        Ok(self.replace_with_split(
                            index - 1,
                            split_sub_tree.0,
                            new_leaf,
                            split_sub_tree.1,
                        )?)
                    }
                }
            } else {
                // If we're on far left we can just splice in
                Ok(self.splice_in(NodeEntry::Leaf(new_leaf), index)?)
            }
        } else if key_zeros < layer {
            // it belongs on a lower layer
            let index = self.find_gt_or_equal_leaf_index(key)?;
            let prev_node = self.at_index(index - 1)?;
            if let Some(NodeEntry::MST(mut p)) = prev_node {
                // if entry before is a tree, we add it to that tree
                let new_subtree = p.add(key, value, Some(key_zeros))?;
                Ok(self.update_entry(index - 1, NodeEntry::MST(new_subtree))?)
            } else {
                let mut sub_tree = self.create_child()?;
                let new_sub_tree = sub_tree.add(key, value, Some(key_zeros))?;
                Ok(self.splice_in(NodeEntry::MST(new_sub_tree), index)?)
            }
        } else {
            // it belongs on a higher layer & we must push the rest of the tree down
            let split = self.split_around(key)?;
            // if the newly added key has >=2 more leading zeros than the current highest layer
            // then we need to add in structural nodes in between as well
            let mut left: Option<MST> = split.0;
            let mut right: Option<MST> = split.1;
            let layer = self.get_layer()?;
            let extra_layers_to_add = key_zeros - layer;
            // intentionally starting at 1, since first layer is taken care of by split
            for _ in 1..extra_layers_to_add {
                if let Some(mut l) = left {
                    left = Some(l.create_parent()?);
                }
                if let Some(mut r) = right {
                    right = Some(r.create_parent()?);
                }
            }
            let mut updated: Vec<NodeEntry> = Vec::new();
            if let Some(l) = left {
                updated.push(NodeEntry::MST(l));
            }
            updated.push(NodeEntry::Leaf(Leaf {
                key: key.clone(),
                value,
            }));
            if let Some(r) = right {
                updated.push(NodeEntry::MST(r));
            }
            let mut new_root = MST::create(self.storage.clone(), Some(updated), Some(key_zeros))?;
            new_root.outdated_pointer = true;
            Ok(new_root)
        };
    }

    // Gets the value at the given key
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

    // Edits the value at the given key
    // Throws if the given key does not exist
    pub fn update(&mut self, key: &String, value: Cid) -> Result<MST> {
        util::ensure_valid_mst_key(key)?;
        let index = self.find_gt_or_equal_leaf_index(key)?;
        let found = self.at_index(index)?;
        if let Some(NodeEntry::Leaf(f)) = found {
            if f.key == *key {
                return Ok(self.update_entry(
                    index,
                    NodeEntry::Leaf(Leaf {
                        key: key.clone(),
                        value,
                    }),
                )?);
            }
        }
        let prev = self.at_index(index - 1)?;
        if let Some(NodeEntry::MST(mut p)) = prev {
            let updated_tree = p.update(key, value)?;
            return Ok(self.update_entry(index - 1, NodeEntry::MST(updated_tree))?);
        }
        Err(anyhow!("Could not find a record with key: {}", key))
    }

    // Deletes the value at the given key
    pub fn delete(&mut self, key: &String) -> Result<MST> {
        let mut altered = self.delete_recurse(key)?;
        Ok(altered.trim_top()?)
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
                    (Some(NodeEntry::MST(mut p)), Some(NodeEntry::MST(mut n))) => {
                        let merged = p.append_merge(n)?;
                        let mut new_tree_entries: Vec<NodeEntry> = Vec::new();
                        new_tree_entries
                            .append(&mut self.slice(Some(0), Some(index - 1))?.to_vec());
                        new_tree_entries.push(NodeEntry::MST(merged));
                        new_tree_entries.append(&mut self.slice(Some(index + 2), None)?.to_vec());
                        Ok(self.new_tree(new_tree_entries)?)
                    }
                    (_, _) => Ok(self.remove_entry(index)?),
                };
            }
        }
        // else recurse down to find it
        let prev = self.at_index(index - 1)?;
        return if let Some(NodeEntry::MST(mut p)) = prev {
            let mut subtree = p.delete_recurse(key)?;
            let subtree_entries = subtree.get_entries()?;
            if subtree_entries.len() == 0 {
                Ok(self.remove_entry(index - 1)?)
            } else {
                Ok(self.update_entry(index - 1, NodeEntry::MST(subtree))?)
            }
        } else {
            Err(anyhow!("Could not find a record with key: {}", key))
        };
    }

    // Simple Operations
    // -------------------

    // update entry in place
    pub fn update_entry<'b>(&mut self, index: usize, entry: NodeEntry) -> Result<MST<'b>> {
        let mut update = Vec::new();
        for e in self.slice(Some(0), Some(index))? {
            update.push(e.clone());
        }
        update.push(entry);
        for e in self.slice(Some(index + 1), None)? {
            update.push(e.clone());
        }
        Ok(self.new_tree(update)?)
    }

    // remove entry at index
    pub fn remove_entry<'b>(&mut self, index: usize) -> Result<MST<'b>> {
        let mut updated = Vec::new();
        updated.append(&mut self.slice(Some(0), Some(index))?.to_vec());
        updated.append(&mut self.slice(Some(index + 1), None)?.to_vec());

        Ok(self.new_tree(updated)?)
    }

    // append entry to end of the node
    pub fn append(&mut self, entry: NodeEntry) -> Result<MST> {
        let mut entries = self.get_entries()?;
        entries.push(entry);
        Ok(self.new_tree(entries.clone())?)
    }

    // prepend entry to end of the node
    pub fn prepend(&mut self, entry: NodeEntry) -> Result<MST> {
        let mut entries = self.get_entries()?;
        entries.splice(0..0, vec![entry]);
        Ok(self.new_tree(entries.clone())?)
    }

    // returns entry at index
    pub fn at_index(&mut self, index: usize) -> Result<Option<&NodeEntry>> {
        let entries = self.get_entries()?;
        Ok(entries.get(index))
    }

    // returns a slice of the node
    pub fn slice(&mut self, start: Option<usize>, end: Option<usize>) -> Result<&[NodeEntry]> {
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

    // inserts entry at index
    pub fn splice_in(&mut self, entry: NodeEntry, index: usize) -> Result<MST> {
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

    // replaces an entry with [ Some(tree), Leaf, Some(tree) ]
    pub fn replace_with_split<'b>(
        &mut self,
        index: usize,
        left: Option<MST>,
        leaf: Leaf,
        right: Option<MST>,
    ) -> Result<MST<'b>> {
        let update = self.slice(Some(0), Some(index))?;
        let mut update = update.to_vec();
        if let Some(l) = left {
            update.push(NodeEntry::MST(l));
        }
        update.push(NodeEntry::Leaf(leaf));
        if let Some(r) = right {
            update.push(NodeEntry::MST(r));
        }
        let remainder = self.slice(Some(index + 1), None)?;
        let remainder = &mut remainder.to_vec();
        update.append(remainder);
        Ok(self.new_tree(update)?)
    }

    // if the topmost node in the tree only points to another tree, trim the top and return the subtree
    pub fn trim_top(&mut self) -> Result<MST> {
        let entries = self.get_entries()?;
        return if entries.len() == 1 {
            match &entries[0] {
                NodeEntry::MST(mut n) => Ok(n.trim_top()?),
                _ => Ok(self.clone()),
            }
        } else {
            Ok(self.clone())
        };
    }

    // Subtree & Splits
    // -------------------

    // Recursively splits a subtree around a given key
    pub fn split_around<'b>(&mut self, key: &String) -> Result<(Option<MST<'b>>, Option<MST<'b>>)> {
        let index = self.find_gt_or_equal_leaf_index(key)?;
        // split tree around key
        let left_data = self.slice(Some(0), Some(index))?;
        let right_data = self.slice(Some(index), None)?;
        let mut left = self.new_tree(left_data.to_vec())?;
        let mut right = self.new_tree(right_data.to_vec())?;

        // if the far right of the left side is a subtree,
        // we need to split it on the key as well
        let last_in_left = left_data.last();
        if let Some(NodeEntry::MST(mut last)) = last_in_left {
            left = left.remove_entry(left_data.len() - 1)?;
            let split = last.split_around(key)?;
            if let Some(s0) = split.0 {
                left = left.append(NodeEntry::MST(s0))?;
            }
            if let Some(s1) = split.1 {
                right = left.append(NodeEntry::MST(s1))?;
            }
        }

        let mut left_output: Option<MST> = None;
        match left.get_entries()?.len() {
            0 => left_output = None,
            _ => left_output = Some(left),
        };
        let mut right_output: Option<MST> = None;
        match right.get_entries()?.len() {
            0 => right_output = None,
            _ => right_output = Some(right),
        };
        Ok((left_output, right_output))
    }

    // The simple merge case where every key in the right tree is greater than every key in the left tree
    // (used primarily for deletes)
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
            (Some(NodeEntry::MST(mut l)), Some(NodeEntry::MST(mut r))) => {
                let merged = l.append_merge(r)?;
                new_tree_entries.append(&mut self_entries[0..self_entries.len() - 1].to_vec());
                new_tree_entries.push(NodeEntry::MST(merged));
                new_tree_entries.append(&mut to_merge_entries[0..1].to_vec());
                Ok(self.new_tree(new_tree_entries)?)
            }
            (_, _) => {
                new_tree_entries.append(&mut self_entries);
                new_tree_entries.append(&mut to_merge_entries);
                Ok(self.new_tree(new_tree_entries)?)
            }
        };
    }

    // Create relatives
    // -------------------

    pub fn create_child<'b>(&mut self) -> Result<MST<'b>> {
        let layer = self.get_layer()?;
        Ok(MST::create(
            self.storage.clone(),
            Some(Vec::new()),
            Some(layer - 1),
        )?)
    }

    pub fn create_parent<'b>(&mut self) -> Result<MST<'b>> {
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

    // finds index of first leaf node that is greater than or equal to the value
    pub fn find_gt_or_equal_leaf_index(&mut self, key: &String) -> Result<usize> {
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
            .position(|entry| entry.key >= *key);
        // if we can't find, we're on the end
        if let Some(i) = maybe_index {
            Ok(i)
        } else {
            Ok(entries.len())
        }
    }

    // List operations (partial tree traversal)
    // -------------------

    // Walk tree starting at key
    // @Rudy Note: This may be suboptimal since we always traverse the tree even though external
    // controls might stop earlier.
    pub fn walk_leaves_from(&mut self, key: &String) -> impl Iterator<Item = Leaf> {
        let mut iter: Vec<Leaf> = Vec::new();
        let index = self.find_gt_or_equal_leaf_index(key).unwrap();
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
        count: usize,
        after: Option<String>,
        before: Option<String>,
    ) -> Result<Vec<Leaf>> {
        let mut vals: Vec<Leaf> = Vec::new();
        let after = after.unwrap_or("".to_owned());
        for leaf in self.walk_leaves_from(&after) {
            if leaf.key == after {
                continue;
            }
            if vals.len() >= count {
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

    // Walk full tree & emit nodes, consumer can bail at any point by returning None
    pub fn walk(self) -> NodeIter<'a> {
        NodeEntry::MST(self).iter()
    }

    // Walk full tree & emit nodes, consumer can bail at any point by returning None
    pub fn paths(self) -> Result<Vec<Vec<&'a NodeEntry<'a>>>> {
        let mut paths: Vec<Vec<&'a NodeEntry<'a>>> = Vec::new();
        for entry in self.walk() {
            match entry {
                NodeEntry::Leaf(_) => paths.push(vec![entry]),
                NodeEntry::MST(m) => {
                    let mut sub_paths = m.clone().paths()?;
                    sub_paths
                        .into_iter()
                        .map(|mut p| {
                            let mut path: Vec<&'a NodeEntry<'a>> = vec![&entry.clone()];
                            path.append(&mut p);
                            paths.push(path)
                        })
                        .for_each(drop);
                }
            }
        }
        Ok(paths)
    }

    // Walks tree & returns all nodes
    pub fn all_nodes(self) -> Result<Vec<&'a NodeEntry<'a>>> {
        let mut nodes: Vec<&NodeEntry> = Vec::new();
        for entry in self.walk() {
            nodes.push(entry);
        }
        Ok(nodes)
    }

    // Walks tree & returns all cids
    pub fn all_cids(self) -> Result<CidSet> {
        let mut cids = CidSet::new(None);
        for entry in self.walk() {
            match entry {
                NodeEntry::Leaf(leaf) => cids = cids.add(leaf.value),
                NodeEntry::MST(m) => {
                    let subtree_cids = m.clone().all_cids()?;
                    cids = cids.add_set(subtree_cids);
                }
            }
        }
        cids = cids.add(self.clone().get_pointer()?);
        Ok(cids)
    }

    // Walks tree & returns all leaves
    pub fn leaves(self) -> Result<Vec<&'a Leaf>> {
        let mut leaves: Vec<&'a Leaf> = Vec::new();
        for entry in self.walk() {
            if let NodeEntry::Leaf(leaf) = entry {
                leaves.push(leaf);
            }
        }
        Ok(leaves)
    }

    // Returns total leaf count
    pub fn leaf_count(self) -> Result<usize> {
        let leaves = self.leaves()?;
        Ok(leaves.len())
    }

    // Reachable tree traversal
    // -------------------

    // Walk reachable branches of tree & emit nodes, consumer can bail at any point
    // by returning false
    pub fn walk_reachable(self) -> NodeIterReachable<'a> {
        NodeEntry::MST(self).iter_reachable()
    }

    pub fn reachable_leaves(self) -> Result<Vec<&'a Leaf>> {
        let mut leaves: Vec<&'a Leaf> = Vec::new();
        for entry in self.walk_reachable() {
            if let Ok(NodeEntry::Leaf(leaf)) = entry {
                leaves.push(leaf);
            }
        }
        Ok(leaves)
    }

    // Sync Protocol
    pub fn write_to_car_stream(
        &mut self,
        car: &mut BlockWriter,
    ) -> Result<()> {
        let mut leaves = CidSet::new(None);
        let mut to_fetch = CidSet::new(None);
        to_fetch = to_fetch.add(self.get_pointer()?);
        while to_fetch.size() > 0 {
            let mut next_layer = CidSet::new(None);
            let fetched = self
                .storage
                .get_blocks(&mut self.storage.conn, to_fetch.to_list())?;
            if fetched.missing.len() > 0 {
                return Err(anyhow::Error::new(DataStoreError::MissingBlocks(
                    "mst node".to_owned(),
                    fetched.missing,
                )));
            }
            for cid in to_fetch.to_list() {
                let found: ObjAndBytes =
                    parse::get_and_parse_by_kind(&fetched.blocks, cid, |obj| {
                        matches!(obj, Ipld::Node(_))
                    })?;
                car.push(CidAndBytes {
                    cid,
                    bytes: found.bytes,
                });
                let node_date: NodeData = found.obj.node();
                let entries = util::deserialize_node_data(&self.storage, &node_date, None)?;
                
                for entry in entries {
                    match entry {
                        NodeEntry::Leaf(l) => leaves = leaves.add(l.value),
                        NodeEntry::MST(mut m) => next_layer = next_layer.add(m.get_pointer()?)
                    }
                }
            }
            to_fetch = next_layer;
        }
        let leaf_data = self.storage.get_blocks(&mut self.storage.conn, leaves.to_list())?;
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

    pub fn cids_for_path(&mut self, key: &String) -> Result<Vec<Cid>> {
        let mut cids: Vec<Cid> = vec![self.get_pointer()?];
        let index = self.find_gt_or_equal_leaf_index(key)?;
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

pub mod util;
