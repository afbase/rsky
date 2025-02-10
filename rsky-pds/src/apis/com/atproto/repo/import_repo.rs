use crate::car::read_car;
use crate::common::tid::TID;
use crate::db::establish_connection;
use crate::models::RepoRoot;
use crate::repo::parse::get_and_parse_record;
use crate::repo::types::{Lex, RecordWriteDescript};
use crate::repo::{ActorStore, Repo};
use crate::storage::types::RepoStorage;
use crate::storage::Ipld;
use aws_config::SdkConfig;
use chrono::Utc;
use diesel::*;
use libipld::Cid;
use rocket::http::ext::IntoCollection;
use rocket::State;
use rsky_syntax::aturi::AtUri;
use crate::repo::sync::consumer::verify_diff;
async fn inner_import_repo(
    actor_store: &State<ActorStore>,
    s3_config: &State<SdkConfig>,
    incoming_car: Vec<u8>,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    let rev = TID::next_str(None);
    let did = actor_store.did;

    // Read Stream
    if let Ok(mut car) = read_car(incoming_car).await {
        if car.roots.length != 1 {
            Err("expected one root")
        }
        // @TODO balance risk of a race in the case of a long retry
        use crate::schema::pds::repo_root::dsl as RepoRootSchema;
        let conn = &mut establish_connection()?;
        let res = RepoRootSchema::repo_root
        .limit(1)
        .select(RepoRoot::as_select())
        .first(conn)?;
        let res_cid = Cid::try_from(res.cid)?;
        let mut repo = Repo::load(storage, Some(res_cid)).await;
        let mut diff = verify_diff(repo, &mut car.blocks, car.roots[0], None, None, None).await?;
        diff.commit.rev = rev.unwrap();
        let _k = actor_store.storage.read().await.apply_commit(diff.commit, None).await?;
        for write in diff.writes {
            let uri = AtUri::make(did, write.collection, write.rkey)?;
            if let RecordWriteDescript::Delete(_) = write {
                actor_store.record.delete_record(&uri).await?;
            } else {
                let  parsed = get_and_parse_record(&car.blocks, write.cid)?;
                let parsed_record = parsed.record;
                let lex_parsed_record = Lex::Map(parsed_record.clone());
                let  _index_record = actor_store.record.index_record(
                    uri,
                    write.cid,
                    Some(parsed_record),
                    write.action(),
                    rev,
                Some(now)).await?;
                // @TODO needs to use rsky/rsky-pds/src/repo/types.rs -> Lex instead of
                // use rsky_lexicon::serialize::LexValue;
                let record_blobs = find_blob_refs(&lex_parsed_record, 0);
                actor_store.blob.insert_blobs(uri, record_blobs)
            }
        }

    }

}

#[rocket::post("/xrpc/com.atproto.repo.importRepo")]
pub async fn import_repo(
    actor_store: &State<ActorStore>,
    s3_config: &State<SdkConfig>,
    incoming_car: Vec<u8>,
) {
    unimplemented!()
}

// @TODO Need to fix this implementation.  Map is not quite right.
pub fn find_blob_refs(val: &Lex, layer: usize) -> Vec<BlobRef> {
    if layer > 32 {
        return vec![];
    }
    
    match val {
        Lex::Ipld(ipld_val) => {
            match ipld_val {
                Ipld::Link(_) | Ipld::Bytes(_) => {
                    vec![]
                },
                Ipld::List(vec) => {
                    vec.iter()
                    .flat_map(|item| find_blob_refs(&Lex::Ipld(item.clone()), layer + 1))
                    .collect()
                },
                Ipld::Map(btreemap) => {
                    btreemap.values()
                    .flat_map(|item| find_blob_refs(&Lex::Ipld(item.clone()), layer + 1))
                    .collect()
                },
                _ => vec![]
            }
        },
        Lex::Blob(_) => vec![],
        Lex::List(_) => vec![],
        Lex::Map(_) => vec![],
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use std::collections::HashMap;
//     use libipld::Cid;
//     use rsky_common_web::ipld::IpldValue;

//     // Helper function to create a test blob ref
//     fn create_test_blob() -> BlobRef {
//         let cid = Cid::try_from("bafyreie5737gdxlw5i64vxljttuk6tp6h6kcgvqicxr2xg7j6fpd6k4dii").unwrap();
//         BlobRef::new(cid, "image/jpeg".to_string(), 1024)
//     }

//     // Positive Tests

//     #[test]
//     fn test_find_single_blob() {
//         let blob_ref = create_test_blob();
//         let value = LexValue::Blob(blob_ref.clone());
        
//         let found = find_blob_refs(&value, 0);
//         assert_eq!(found.len(), 1);
//         assert_eq!(found[0], blob_ref);
//     }

//     #[test]
//     fn test_find_blob_in_array() {
//         let blob_ref = create_test_blob();
//         let value = LexValue::Array(vec![
//             LexValue::Ipld(IpldValue::String("test".to_string())),
//             LexValue::Blob(blob_ref.clone()),
//         ]);
        
//         let found = find_blob_refs(&value, 0);
//         assert_eq!(found.len(), 1);
//         assert_eq!(found[0], blob_ref);
//     }

//     #[test]
//     fn test_find_blobs_in_object() {
//         let blob_ref1 = create_test_blob();
//         let blob_ref2 = create_test_blob();
        
//         let mut map = HashMap::new();
//         map.insert("blob1".to_string(), LexValue::Blob(blob_ref1.clone()));
//         map.insert("blob2".to_string(), LexValue::Blob(blob_ref2.clone()));
//         let value = LexValue::Object(map);
        
//         let found = find_blob_refs(&value, 0);
//         assert_eq!(found.len(), 2);
//         assert!(found.contains(&blob_ref1));
//         assert!(found.contains(&blob_ref2));
//     }

//     #[test]
//     fn test_find_blob_deeply_nested() {
//         let blob_ref = create_test_blob();
        
//         // Create a nested structure: object -> array -> object -> blob
//         let mut inner_map = HashMap::new();
//         inner_map.insert("blob".to_string(), LexValue::Blob(blob_ref.clone()));
        
//         let array = LexValue::Array(vec![LexValue::Object(inner_map)]);
        
//         let mut outer_map = HashMap::new();
//         outer_map.insert("nested".to_string(), array);
//         let value = LexValue::Object(outer_map);
        
//         let found = find_blob_refs(&value, 0);
//         assert_eq!(found.len(), 1);
//         assert_eq!(found[0], blob_ref);
//     }

//     #[test]
//     fn test_find_blob_in_ipld_structure() {
//         let blob_ref = create_test_blob();
        
//         // Create an IPLD structure containing a blob
//         let mut ipld_map = HashMap::new();
//         ipld_map.insert(
//             "data".to_string(), 
//             IpldValue::Array(vec![IpldValue::String("test".to_string())])
//         );
        
//         let mut outer_map = HashMap::new();
//         outer_map.insert("ipld".to_string(), LexValue::Ipld(IpldValue::Object(ipld_map)));
//         outer_map.insert("blob".to_string(), LexValue::Blob(blob_ref.clone()));
        
//         let value = LexValue::Object(outer_map);
        
//         let found = find_blob_refs(&value, 0);
//         assert_eq!(found.len(), 1);
//         assert_eq!(found[0], blob_ref);
//     }

//     // Negative Tests

//     #[test]
//     fn test_no_blobs_in_simple_values() {
//         let value = LexValue::Ipld(IpldValue::String("test".to_string()));
//         assert!(find_blob_refs(&value, 0).is_empty());
        
//         let value = LexValue::Ipld(IpldValue::Number(42.0));
//         assert!(find_blob_refs(&value, 0).is_empty());
        
//         let value = LexValue::Ipld(IpldValue::Bool(true));
//         assert!(find_blob_refs(&value, 0).is_empty());
        
//         let value = LexValue::Ipld(IpldValue::Null);
//         assert!(find_blob_refs(&value, 0).is_empty());
//     }

//     #[test]
//     fn test_no_blobs_in_special_ipld_values() {
//         let cid = Cid::try_from("bafyreie5737gdxlw5i64vxljttuk6tp6h6kcgvqicxr2xg7j6fpd6k4dii").unwrap();
//         let value = LexValue::Ipld(IpldValue::Cid(cid));
//         assert!(find_blob_refs(&value, 0).is_empty());
        
//         let value = LexValue::Ipld(IpldValue::Bytes(vec![1, 2, 3]));
//         assert!(find_blob_refs(&value, 0).is_empty());
//     }

//     #[test]
//     fn test_max_depth_limit() {
//         let blob_ref = create_test_blob();
//         let mut value = LexValue::Blob(blob_ref);
        
//         // Nest the blob beyond the depth limit
//         for _ in 0..33 {  // More than the 32 layer limit
//             value = LexValue::Array(vec![value]);
//         }
        
//         assert!(find_blob_refs(&value, 0).is_empty());
//     }

//     #[test]
//     fn test_empty_structures() {
//         let value = LexValue::Array(vec![]);
//         assert!(find_blob_refs(&value, 0).is_empty());
        
//         let value = LexValue::Object(HashMap::new());
//         assert!(find_blob_refs(&value, 0).is_empty());
        
//         let value = LexValue::Ipld(IpldValue::Array(vec![]));
//         assert!(find_blob_refs(&value, 0).is_empty());
        
//         let value = LexValue::Ipld(IpldValue::Object(HashMap::new()));
//         assert!(find_blob_refs(&value, 0).is_empty());
//     }
// }