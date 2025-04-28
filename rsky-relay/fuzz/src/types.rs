// fuzz/src/types.rs

use arbitrary::{Arbitrary, Result, Unstructured};
use cid::{Cid, multihash};
use cid::multihash::{Code, MultihashDigest};
use ipld_core::codec::Codec;
use serde_ipld_dagcbor::codec::DagCborCodec;
use cid::multihash::Hasher;
use rsky_relay::validator::event::SubscribeReposCommitOperation;

/// A valid path for a repository record
#[derive(Debug, Clone)]
pub struct ValidPath(pub String);

impl Arbitrary<'_> for ValidPath {
    fn arbitrary(u: &mut Unstructured<'_>) -> Result<Self> {
        // Choose a namespace
        let namespace = u.choose(&[
            "app.bsky.feed",
            "app.bsky.graph",
            "app.bsky.actor",
            "com.example.record",
            "test.namespace",
        ])?;
        
        // Choose a record type
        let record_type = u.choose(&[
            "post",
            "like",
            "follow",
            "profile",
            "record",
        ])?;
        
        // Generate a valid record ID (base32 chars)
        let id_len = u.int_in_range(5..=20)?;
        let id_chars: Vec<char> = (0..id_len)
            .map(|_| u.choose(b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567").map(|&b| b as char))
            .collect::<Result<_>>()?;
        
        let path = format!("{}/{}/{}", namespace, record_type, id_chars.into_iter().collect::<String>());
        Ok(ValidPath(path))
    }
}

/// A wrapper around Cid that can be arbitrarily generated
#[derive(Debug, Clone, Copy)]
pub struct ArbitraryCid(pub Cid);

impl Arbitrary<'_> for ArbitraryCid {
    fn arbitrary(u: &mut Unstructured<'_>) -> Result<Self> {
        // Generate a pseudo-random sequence for the hash
        let len = u.int_in_range(16..=32)?;
        let bytes: Vec<u8> = (0..len).map(|_| u.arbitrary()).collect::<Result<_>>()?;
        
        let mut hasher = multihash::Sha2_256::default();
        hasher.update(&bytes);
        let mh = Code::Sha2_256.wrap(hasher.finalize()).unwrap();
        let cid = Cid::new_v1(<DagCborCodec as Codec<()>>::CODE, mh);
        
        Ok(ArbitraryCid(cid))
    }
}

/// A tree operation (Create, Update, Delete)
#[derive(Debug, Clone)]
pub enum TreeOperation {
    Create {
        path: ValidPath,
        cid: ArbitraryCid,
    },
    Update {
        path: ValidPath,
        cid: ArbitraryCid,
        prev_cid: ArbitraryCid,
    },
    Delete {
        path: ValidPath,
        prev_cid: ArbitraryCid,
    },
}

impl Arbitrary<'_> for TreeOperation {
    fn arbitrary(u: &mut Unstructured<'_>) -> Result<Self> {
        let path = ValidPath::arbitrary(u)?;
        let cid = ArbitraryCid::arbitrary(u)?;
        let prev_cid = ArbitraryCid::arbitrary(u)?;
        
        match u.int_in_range(0..=2)? {
            0 => Ok(TreeOperation::Create { 
                path,
                cid,
            }),
            1 => Ok(TreeOperation::Update { 
                path,
                cid,
                prev_cid,
            }),
            _ => Ok(TreeOperation::Delete { 
                path,
                prev_cid,
            }),
        }
    }
}

impl TreeOperation {
    pub fn to_commit_operation(&self) -> SubscribeReposCommitOperation {
        match self {
            TreeOperation::Create { path, cid } => {
                SubscribeReposCommitOperation::Create {
                    path: path.0.clone(),
                    cid: cid.0,
                }
            },
            TreeOperation::Update { path, cid, prev_cid } => {
                SubscribeReposCommitOperation::Update {
                    path: path.0.clone(),
                    cid: cid.0,
                    prev_data: Some(prev_cid.0),
                }
            },
            TreeOperation::Delete { path, prev_cid } => {
                SubscribeReposCommitOperation::Delete {
                    path: path.0.clone(),
                    prev_data: Some(prev_cid.0),
                }
            },
        }
    }
}

/// A sequence of tree operations to apply
#[derive(Debug, Clone, Arbitrary)]
pub struct TreeOperationSequence {
    pub operations: Vec<TreeOperation>,
}