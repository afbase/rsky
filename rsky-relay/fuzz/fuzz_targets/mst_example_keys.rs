#![no_main]
use libfuzzer_sys::fuzz_target;
use arbitrary::Arbitrary;
use std::collections::HashMap;
use cid::{Cid, multihash};
use cid::multihash::{Code, MultihashDigest};
use serde_ipld_dagcbor::codec::DagCborCodec;
use ipld_core::codec::Codec;
use cid::multihash::Hasher;
use rsky_relay::validator::types::{Node, NodeEntry};

// Helper function to create a deterministic CID from a key
fn create_cid_from_key(key: &str) -> Cid {
    let mut hasher = multihash::Sha2_256::default();
    hasher.update(key.as_bytes());
    let mh = Code::Sha2_256.wrap(hasher.finalize()).unwrap();
    Cid::new_v1(<DagCborCodec as Codec<()>>::CODE, mh)
}

#[derive(Debug, Arbitrary)]
struct TreeOpsConfig {
    // What percentage of keys to insert (0-100)
    insert_percentage: u8,
    // What percentage of inserted keys to update (0-100)
    update_percentage: u8, 
    // What percentage of inserted keys to delete (0-100)
    delete_percentage: u8,
    // Random seed for operation ordering
    seed: u64,
}

fuzz_target!(|input: TreeOpsConfig| {
    // Read the example keys
    let keys_str = include_str!("../atproto-interop-tests/mst/example_keys.txt");
    let example_keys: Vec<&str> = keys_str.lines().collect();
    
    // Create a tree
    let mut node = Node::default();
    let mut inserted_keys = HashMap::new();
    
    // Use normalized percentages (0-100)
    let insert_pct = input.insert_percentage.min(100);
    let update_pct = input.update_percentage.min(100);
    let delete_pct = input.delete_percentage.min(100);
    
    // Insert keys based on insert percentage
    for key in &example_keys {
        // Use a deterministic but varying decision based on the key and seed
        let hash_val = key.bytes().fold(input.seed as u32, |acc, b| acc.wrapping_add(b as u32));
        if hash_val % 100 < insert_pct as u32 {
            let cid = create_cid_from_key(key);
            if node.insert(key, cid, -1).is_ok() {
                inserted_keys.insert(key.to_string(), cid);
            }
        }
    }
    
    // Update some of the inserted keys
    let keys_to_update: Vec<String> = inserted_keys.keys()
        .filter(|&k| {
            let hash_val = k.bytes().fold(input.seed.wrapping_add(1) as u32, |acc, b| acc.wrapping_add(b as u32));
            hash_val % 100 < update_pct as u32
        })
        .cloned()
        .collect();
    
    for key in &keys_to_update {
        // Create a different CID for the update
        let new_cid = create_cid_from_key(&format!("updated_{}", key));
        if node.insert(key, new_cid, -1).is_ok() {
            inserted_keys.insert(key.to_string(), new_cid);
        }
    }
    
    // Delete some of the inserted keys
    let keys_to_delete: Vec<String> = inserted_keys.keys()
        .filter(|&k| {
            let hash_val = k.bytes().fold(input.seed.wrapping_add(2) as u32, |acc, b| acc.wrapping_add(b as u32));
            hash_val % 100 < delete_pct as u32
        })
        .cloned()
        .collect();
    
    for key in &keys_to_delete {
        if node.remove(key, -1).is_ok() {
            inserted_keys.remove(key);
        }
    }
    
    // Verify tree integrity by computing the root
    if let Ok(_root_cid) = node.root() {
        // Verify we can find all the keys we expect
        for (key, cid) in &inserted_keys {
            let idx = node.find_value(key);
            assert!(idx.is_some(), "Couldn't find key that should be in the tree: {}", key);
            
            if let Some(idx) = idx {
                if let NodeEntry::Value { value, .. } = &node.entries[idx] {
                    assert_eq!(value, cid, "Key {} has wrong value", key);
                } else {
                    panic!("Found entry is not a Value");
                }
            }
        }
        
        // Verify integrity with some splits and merges
        if node.entries.len() >= 2 {
            // Find a middle key to split at
            if let Some(middle_key) = example_keys.get(example_keys.len() / 2) {
                if let Ok((mut left, right)) = node.split(middle_key) {
                    // Test merging
                    let _ = left.merge(right);
                }
            }
        }
    }
});