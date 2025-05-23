use crate::types::CidAndBytes;
use anyhow::Result;
use lexicon_cid::Cid;
use rsky_common;
use rsky_common::ipld;
use serde::Serialize;
use std::collections::BTreeMap;
use std::str::FromStr;

// Thinly wraps a Vec<u8>
// The #[serde(transparent)] attribute ensures that during (de)serialization
// this newtype is treated the same as the underlying Vec<u8>.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Bytes(#[serde(with = "serde_bytes")] pub Vec<u8>);

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct BlockMap {
    pub map: BTreeMap<String, Bytes>,
}

impl BlockMap {
    pub fn new() -> Self {
        BlockMap {
            map: BTreeMap::new(),
        }
    }

    pub fn add<T: Serialize>(&mut self, value: T) -> Result<Cid> {
        let cid = ipld::cid_for_cbor(&value)?;
        self.set(
            cid,
            rsky_common::struct_to_cbor(&value)?, //bytes
        );
        Ok(cid)
    }

    pub fn set(&mut self, cid: Cid, bytes: Vec<u8>) -> () {
        self.map.insert(cid.to_string(), Bytes(bytes));
        ()
    }

    pub fn get(&self, cid: Cid) -> Option<&Vec<u8>> {
        self.map.get(&cid.to_string()).map(|bytes| &bytes.0)
    }
    pub fn delete(&mut self, cid: Cid) -> Result<()> {
        self.map.remove(&cid.to_string());
        Ok(())
    }

    pub fn get_many(&mut self, cids: Vec<Cid>) -> Result<BlocksAndMissing> {
        let mut missing: Vec<Cid> = Vec::new();
        let mut blocks = BlockMap::new();
        for cid in cids {
            let got = self.map.get(&cid.to_string()).map(|bytes| &bytes.0);
            if let Some(bytes) = got {
                blocks.set(cid, bytes.clone());
            } else {
                missing.push(cid);
            }
        }
        Ok(BlocksAndMissing { blocks, missing })
    }

    pub fn has(&self, cid: Cid) -> bool {
        self.map.contains_key(&cid.to_string())
    }

    pub fn clear(&mut self) -> () {
        self.map.clear()
    }

    // Not really using. Issues with closures
    pub fn for_each(&self, cb: impl Fn(&Vec<u8>, Cid) -> ()) -> Result<()> {
        for (key, val) in self.map.iter() {
            cb(&val.0, Cid::from_str(&key)?);
        }
        Ok(())
    }

    pub fn entries(&self) -> Result<Vec<CidAndBytes>> {
        let mut entries: Vec<CidAndBytes> = Vec::new();
        for (cid, bytes) in self.map.iter() {
            entries.push(CidAndBytes {
                cid: Cid::from_str(cid)?,
                bytes: bytes.0.clone(),
            });
        }
        Ok(entries)
    }

    pub fn cids(&self) -> Result<Vec<Cid>> {
        Ok(self.entries()?.into_iter().map(|e| e.cid).collect())
    }

    pub fn add_map(&mut self, to_add: BlockMap) -> Result<()> {
        let results = for (cid, bytes) in to_add.map.iter() {
            self.set(Cid::from_str(cid)?, bytes.0.clone());
        };
        Ok(results)
    }

    pub fn size(&self) -> usize {
        self.map.len()
    }

    pub fn byte_size(&self) -> Result<usize> {
        let mut size = 0;
        for (_, bytes) in self.map.iter() {
            size += bytes.0.len();
        }
        Ok(size)
    }

    pub fn equals(&self, other: BlockMap) -> Result<bool> {
        if self.size() != other.size() {
            return Ok(false);
        }
        for entry in self.entries()? {
            let other_bytes = other.get(entry.cid);
            if let Some(o) = other_bytes {
                if &entry.bytes != o {
                    return Ok(false);
                }
            } else {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

// Helper function for the iterator conversion.
fn to_cid_and_bytes((cid_str, bytes): (String, Bytes)) -> CidAndBytes {
    // We assume the key is always valid; otherwise, you could handle the error.
    let cid = Cid::from_str(&cid_str).expect("BlockMap contains an invalid CID string");
    CidAndBytes {
        cid,
        bytes: bytes.0,
    }
}

impl IntoIterator for BlockMap {
    type Item = CidAndBytes;
    // Using the iterator returned by BTreeMap's into_iter, then mapping with a function pointer.
    type IntoIter = std::iter::Map<
        std::collections::btree_map::IntoIter<String, Bytes>,
        fn((String, Bytes)) -> CidAndBytes,
    >;

    fn into_iter(self) -> Self::IntoIter {
        self.map.into_iter().map(to_cid_and_bytes)
    }
}

#[derive(Debug)]
pub struct BlocksAndMissing {
    pub blocks: BlockMap,
    pub missing: Vec<Cid>,
}
