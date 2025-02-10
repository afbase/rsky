use crate::common::sign::sign_without_indexmap;
use crate::common::tid::Ticker;
use crate::repo::data_diff::DataDiff;
use crate::repo::types::{
    Commit, Lex, RecordCreateOrDeleteDescript, RecordPath, RecordUpdateDescript,
    RecordWriteDescript, RepoRecord, UnsignedCommit, VersionedCommit, WriteOpAction,
};
use crate::storage::Ipld;
use anyhow::{bail, Result};
use futures::{stream, Stream, StreamExt, TryStreamExt};
use lexicon_cid::Cid;
use secp256k1::Keypair;
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::convert::{From, TryFrom};
use std::fmt::Display;
use std::str::FromStr;
use tokio::try_join;

pub fn sign_commit(unsigned: UnsignedCommit, keypair: Keypair) -> Result<Commit> {
    let commit_sig = sign_without_indexmap(&unsigned, &keypair.secret_key())?;
    Ok(Commit {
        did: unsigned.did,
        version: unsigned.version,
        data: unsigned.data,
        rev: unsigned.rev,
        prev: unsigned.prev,
        sig: commit_sig.to_vec(),
    })
}

pub fn verify_commit_sig(commit: Commit, did_key: &String) -> Result<bool> {
    let sig = commit.sig;
    let rest = UnsignedCommit {
        did: commit.did,
        rev: commit.rev,
        data: commit.data,
        prev: commit.prev,
        version: commit.version,
    };
    let encoded = serde_ipld_dagcbor::to_vec(&rest)?;
    let hash = Sha256::digest(&*encoded);
    rsky_crypto::verify::verify_signature(did_key, hash.as_ref(), sig.as_slice(), None)
}

pub fn format_data_key<T: FromStr + Display>(collection: T, rkey: T) -> String {
    format!("{collection}/{rkey}")
}

// Implement From<Lex> for Ipld
impl From<Lex> for Ipld {
    fn from(val: Lex) -> Self {
        match val {
            Lex::List(list) => Ipld::List(
                list.into_iter()
                    .map(Ipld::from)
                    .collect::<Vec<Ipld>>(),
            ),
            Lex::Map(map) => {
                let mut to_return: BTreeMap<String, Ipld> = BTreeMap::new();
                for (key, value) in map {
                    to_return.insert(key, Ipld::from(value));
                }
                Ipld::Map(to_return)
            }
            Lex::Blob(blob) => {
                Ipld::Json(serde_json::to_value(blob.original).expect("Issue serializing blob"))
            }
            Lex::Ipld(ipld) => match ipld {
                Ipld::Json(json_val) => match serde_json::from_value::<Cid>(json_val.clone()) {
                    Ok(cid) => Ipld::Link(cid),
                    Err(_) => Ipld::Json(json_val),
                },
                _ => ipld,
            },
        }
    }
}

// Implement From<Ipld> for Lex
impl From<Ipld> for Lex {
    fn from(val: Ipld) -> Self {
        match val {
            Ipld::List(list) => Lex::List(
                list.into_iter()
                    .map(Lex::from)
                    .collect::<Vec<Lex>>(),
            ),
            Ipld::Map(map) => {
                let mut to_return: BTreeMap<String, Lex> = BTreeMap::new();
                for (key, value) in map {
                    to_return.insert(key, Lex::from(value));
                }
                Lex::Map(to_return)
            }
            Ipld::Json(blob)
                if blob.get("$type") == Some(&JsonValue::String("blob".to_string()))
                    || (matches!(blob.get("cid"), Some(&JsonValue::String(_)))
                        && matches!(blob.get("mimeType"), Some(&JsonValue::String(_)))) =>
            {
                Lex::Blob(serde_json::from_value(blob).expect("Issue deserializing blob"))
            }
            _ => Lex::Ipld(val),
        }
    }
}

// Implement TryFrom<Vec<u8>> for Lex
impl TryFrom<Vec<u8>> for Lex {
    type Error = anyhow::Error;

    fn try_from(val: Vec<u8>) -> Result<Self, Self::Error> {
        let obj: Ipld = serde_ipld_dagcbor::from_slice(val.as_slice())?;
        Ok(Lex::from(obj))
    }
}

#[deprecated(
    since = "0.0.1",
    note = "Use `Ipld::from(lex)` instead"
)]
pub fn lex_to_ipld(val: Lex) -> Ipld {
    Ipld::from(val)
}

#[deprecated(
    since = "0.0.1",
    note = "Use `Lex::from(ipld)` instead"
)]
pub fn ipld_to_lex(val: Ipld) -> Lex {
    Lex::from(val)
}

#[deprecated(
    since = "0.0.1",
    note = "Use `Lex::try_from(vec)` instead"
)]
pub fn cbor_to_lex(val: Vec<u8>) -> Result<Lex> {
    Lex::try_from(val)
}

pub fn cbor_to_lex_record(val: Vec<u8>) -> Result<RepoRecord> {
    let parsed = Lex::try_from(val)?;
    match parsed {
        Lex::Map(map) => Ok(map),
        _ => bail!("Lexicon record should be a json object"),
    }
}

pub fn ensure_creates(
    descripts: Vec<RecordWriteDescript>,
) -> Result<Vec<RecordCreateOrDeleteDescript>> {
    let mut creates: Vec<RecordCreateOrDeleteDescript> = Default::default();
    for descript in descripts {
        match descript {
            RecordWriteDescript::Create(create) => creates.push(create),
            _ => bail!("Unexpected action: {}", descript.action()),
        }
    }
    Ok(creates)
}

pub async fn diff_to_write_descripts(diff: &DataDiff) -> Result<Vec<RecordWriteDescript>> {
    let (add_list, update_list, delete_list) = try_join!(
        // Process add_list
        stream::iter(diff.add_list())
            .then(|add| async move {
                let RecordPath { collection, rkey } = parse_data_key(&add.key)?;
                Ok::<RecordWriteDescript, anyhow::Error>(RecordWriteDescript::Create(
                    RecordCreateOrDeleteDescript {
                        action: WriteOpAction::Create,
                        collection,
                        rkey,
                        cid: add.cid,
                    },
                ))
            })
            .try_collect::<Vec<_>>(),
        // Process update_list
        stream::iter(diff.update_list())
            .then(|upd| async move {
                let RecordPath { collection, rkey } = parse_data_key(&upd.key)?;
                Ok::<RecordWriteDescript, anyhow::Error>(RecordWriteDescript::Update(
                    RecordUpdateDescript {
                        action: WriteOpAction::Update,
                        collection,
                        rkey,
                        cid: upd.cid,
                        prev: upd.prev,
                    },
                ))
            })
            .try_collect::<Vec<_>>(),
        // Process delete_list
        stream::iter(diff.delete_list())
            .then(|del| async move {
                let RecordPath { collection, rkey } = parse_data_key(&del.key)?;
                Ok::<RecordWriteDescript, anyhow::Error>(RecordWriteDescript::Delete(
                    RecordCreateOrDeleteDescript {
                        action: WriteOpAction::Delete,
                        collection,
                        rkey,
                        cid: del.cid,
                    },
                ))
            })
            .try_collect::<Vec<_>>()
    )?;
    Ok([add_list, update_list, delete_list].concat())
}

pub fn parse_data_key(key: &String) -> Result<RecordPath> {
    let parts: Vec<&str> = key.split("/").collect();
    if parts.len() != 2 {
        bail!("Invalid record key: `{key:?}`");
    }
    Ok(RecordPath {
        collection: parts[0].to_owned(),
        rkey: parts[1].to_owned(),
    })
}

pub fn ensure_v3_commit(commit: VersionedCommit) -> Commit {
    match commit {
        VersionedCommit::Commit(commit) if commit.version == 3 => commit,
        VersionedCommit::Commit(commit) => Commit {
            did: commit.did,
            version: 3,
            data: commit.data,
            rev: commit.rev,
            prev: commit.prev,
            sig: commit.sig,
        },
        VersionedCommit::LegacyV2Commit(commit) => Commit {
            did: commit.did,
            version: 3,
            data: commit.data,
            rev: commit.rev.unwrap_or(Ticker::new().next(None).0),
            prev: commit.prev,
            sig: commit.sig,
        },
    }
}

pub fn flatten_u8_arrays(chunks: &[Vec<u8>]) -> Vec<u8> {
    let mut result = Vec::with_capacity(chunks.iter().map(|v| v.len()).sum());
    for chunk in chunks {
        result.extend_from_slice(chunk);
    }
    result
}

pub async fn stream_to_buffer<S>(mut stream: S) -> Result<Vec<u8>>
where
    S: Stream<Item = Result<Vec<u8>>> + Unpin,
{
    let mut buffer = Vec::new();
    while let Some(chunk) = stream.next().await {
        buffer.extend_from_slice(&chunk?);
    }
    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_lex_ipld_conversion() {
        // Test simple map
        let mut map = BTreeMap::new();
        map.insert("key".to_string(), Lex::Ipld(Ipld::String("value".to_string())));
        let lex = Lex::Map(map);
        
        let ipld = Ipld::from(lex.clone());
        let lex_back = Lex::from(ipld);
        
        assert_eq!(lex, lex_back);

        // Test list
        let list = Lex::List(vec![
            Lex::Ipld(Ipld::String(String::default())),
            Lex::Ipld(Ipld::Bytes(vec![1,2,3])),
        ]);
        
        let ipld = Ipld::from(list.clone());
        let list_back = Lex::from(ipld);
        
        assert_eq!(list, list_back);

        // Test blob
        let blob_json = json!({
            "$type": "blob",
            "mimeType": "text/plain",
            "data": "Hello, World!"
        });
        let blob = Lex::Blob(serde_json::from_value(blob_json.clone()).unwrap());
        
        let ipld = Ipld::from(blob.clone());
        let blob_back = Lex::from(ipld);
        
        assert_eq!(blob, blob_back);
    }

    #[test]
    fn test_cbor_conversion() -> Result<()> {
        // Create test data
        let mut map = BTreeMap::new();
        map.insert("test".to_string(), Lex::Ipld(Ipld::String("value".to_string())));
        let lex = Lex::Map(map);

        // Convert to CBOR
        let cbor = serde_ipld_dagcbor::to_vec(&Ipld::from(lex.clone()))?;

        // Test TryFrom for Lex
        let lex_from_cbor = Lex::try_from(cbor.clone())?;
        assert_eq!(lex, lex_from_cbor);

        // Test TryFrom for RepoRecord
        let repo_record = cbor_to_lex_record(cbor)?;
        assert_eq!(repo_record.get("test").unwrap(), &Lex::Ipld(Ipld::String("value".to_string())));

        Ok(())
    }

    #[test]
    fn test_invalid_cbor_conversion() {
        // Test invalid CBOR data
        let invalid_data = vec![0xFF, 0xFF, 0xFF];
        assert!(Lex::try_from(invalid_data.clone()).is_err());
        assert!(cbor_to_lex_record(invalid_data).is_err());

        // Test valid CBOR but invalid format for RepoRecord
        let list_cbor = serde_ipld_dagcbor::to_vec(&Ipld::List(vec![])).unwrap();
        assert!(cbor_to_lex_record(invalid_data).is_err());
    }
}
