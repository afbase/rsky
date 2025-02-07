use crate::blob_refs::BlobRef;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use libipld::Cid;
use rsky_common_web::ipld::IpldValue;
use serde::{Deserialize, Serialize};
use serde_json::Map;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum LexValue {
    Ipld(IpldValue),
    Blob(BlobRef),
    Array(Vec<LexValue>),
    Object(HashMap<String, LexValue>),
}

impl PartialEq for LexValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (LexValue::Ipld(a), LexValue::Ipld(b)) => a == b,
            (LexValue::Blob(a), LexValue::Blob(b)) => a == b,
            (LexValue::Array(a), LexValue::Array(b)) => a == b,
            (LexValue::Object(a), LexValue::Object(b)) => {
                if a.len() != b.len() {
                    return false;
                }
                a.iter().all(|(k, v)| b.get(k).map_or(false, |bv| v == bv))
            }
            _ => false,
        }
    }
}

pub type RepoRecord = HashMap<String, LexValue>;

// Convert IpldValue -> LexValue
impl From<IpldValue> for LexValue {
    fn from(val: IpldValue) -> Self {
        match val {
            IpldValue::Object(obj) => {
                // Check if this object might be a blob reference
                if let Ok(blob_ref) = BlobRef::try_from(&obj) {
                    return LexValue::Blob(blob_ref);
                }

                // Regular object
                LexValue::Object(
                    obj.into_iter()
                        .map(|(k, v)| (k, LexValue::from(v)))
                        .collect(),
                )
            }
            IpldValue::Array(arr) => LexValue::Array(arr.into_iter().map(LexValue::from).collect()),
            // Pass through other IPLD values
            val => LexValue::Ipld(val),
        }
    }
}

// Convert LexValue -> IpldValue
impl From<LexValue> for IpldValue {
    fn from(value: LexValue) -> Self {
        match value {
            LexValue::Ipld(ipld) => ipld,
            LexValue::Blob(blob_ref) => {
                let mut map = HashMap::new();
                map.insert("$type".to_string(), IpldValue::String("blob".to_string()));
                map.insert(
                    "ref".to_string(),
                    IpldValue::String(blob_ref.ref_.to_string()),
                );
                map.insert(
                    "mimeType".to_string(),
                    IpldValue::String(blob_ref.mime_type),
                );
                map.insert("size".to_string(), IpldValue::Number(blob_ref.size as f64));
                IpldValue::Object(map)
            }
            LexValue::Array(arr) => {
                IpldValue::Array(arr.into_iter().map(IpldValue::from).collect())
            }
            LexValue::Object(obj) => IpldValue::Object(
                obj.into_iter()
                    .map(|(k, v)| (k, IpldValue::from(v)))
                    .collect(),
            ),
        }
    }
}

// Convert LexValue -> serde_json::Value
impl From<&LexValue> for serde_json::Value {
    fn from(val: &LexValue) -> Self {
        let ipld: IpldValue = val.clone().into();
        match &ipld {
            IpldValue::Bool(b) => serde_json::Value::Bool(*b),
            IpldValue::Number(n) => serde_json::Number::from_f64(*n)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
            IpldValue::String(s) => serde_json::Value::String(s.clone()),
            IpldValue::Null => serde_json::Value::Null,
            IpldValue::Array(arr) => {
                serde_json::Value::Array(arr.iter().map(|v| v.clone().into()).collect())
            }
            IpldValue::Object(obj) => {
                let mut map = Map::new();
                for (k, v) in obj {
                    map.insert(k.clone(), v.clone().into());
                }
                serde_json::Value::Object(map)
            }
            IpldValue::Cid(cid) => {
                let mut map = Map::new();
                map.insert(
                    "$link".to_string(),
                    serde_json::Value::String(cid.to_string()),
                );
                serde_json::Value::Object(map)
            }
            IpldValue::Bytes(bytes) => {
                let mut map = Map::new();
                map.insert(
                    "$bytes".to_string(),
                    serde_json::Value::String(BASE64.encode(bytes)),
                );
                serde_json::Value::Object(map)
            }
        }
    }
}

// Convert serde_json::Value -> LexValue
impl From<&serde_json::Value> for LexValue {
    fn from(val: &serde_json::Value) -> Self {
        match val {
            serde_json::Value::Bool(b) => LexValue::Ipld(IpldValue::Bool(*b)),
            serde_json::Value::Number(n) => {
                LexValue::Ipld(IpldValue::Number(n.as_f64().unwrap_or_default()))
            }
            serde_json::Value::String(s) => {
                if let Ok(cid) = Cid::try_from(s.as_str()) {
                    LexValue::Ipld(IpldValue::Cid(cid))
                } else {
                    LexValue::Ipld(IpldValue::String(s.clone()))
                }
            }
            serde_json::Value::Null => LexValue::Ipld(IpldValue::Null),
            serde_json::Value::Array(arr) => {
                LexValue::Array(arr.iter().map(LexValue::from).collect())
            }
            serde_json::Value::Object(obj) => {
                // First check if this is a blob object
                if let Some(type_val) = obj.get("$type") {
                    if type_val == "blob" {
                        if let (Some(ref_val), Some(mime_type), Some(size)) =
                            (obj.get("ref"), obj.get("mimeType"), obj.get("size"))
                        {
                            if let (Some(ref_str), Some(mime_str), Some(size_num)) =
                                (ref_val.as_str(), mime_type.as_str(), size.as_i64())
                            {
                                if let Ok(cid) = Cid::try_from(ref_str) {
                                    return LexValue::Blob(BlobRef::new(
                                        cid,
                                        mime_str.to_string(),
                                        size_num,
                                    ));
                                }
                            }
                        }
                    }
                }

                if obj.len() == 1 {
                    if let Some(link) = obj.get("$link") {
                        if let Some(link_str) = link.as_str() {
                            if let Ok(cid) = Cid::try_from(link_str) {
                                return LexValue::Ipld(IpldValue::Cid(cid));
                            }
                        }
                    }
                }

                let map: HashMap<String, LexValue> = obj
                    .iter()
                    .map(|(k, v)| (k.clone(), LexValue::from(v)))
                    .collect();
                LexValue::Object(map)
            }
        }
    }
}

impl Serialize for LexValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let value: serde_json::Value = self.into();
        value.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for LexValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        Ok(LexValue::from(&value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libipld::Cid;

    #[test]
    fn test_lex_conversion_roundtrip() {
        let cid =
            Cid::try_from("bafyreie5737gdxlw5i64vxljttuk6tp6h6kcgvqicxr2xg7j6fpd6k4dii").unwrap();

        let test_cases = vec![
            LexValue::Ipld(IpldValue::Cid(cid.clone())),
            LexValue::Ipld(IpldValue::String("test".to_string())),
            LexValue::Array(vec![
                LexValue::Ipld(IpldValue::String("test".to_string())),
                LexValue::Ipld(IpldValue::Number(42.0)),
                LexValue::Ipld(IpldValue::Cid(cid.clone())),
            ]),
            {
                let mut obj = HashMap::new();
                obj.insert(
                    "cid".to_string(),
                    LexValue::Ipld(IpldValue::Cid(cid.clone())),
                );
                obj.insert(
                    "text".to_string(),
                    LexValue::Ipld(IpldValue::String("test".to_string())),
                );
                LexValue::Object(obj)
            },
        ];

        for original in test_cases {
            let json: serde_json::Value = (&original).into();
            let roundtrip = LexValue::from(&json);
            assert_eq!(original, roundtrip);
        }
    }

    #[test]
    fn test_blob_ref_handling() {
        let blob_json = serde_json::json!({
            "$type": "blob",
            "ref": "bafyreie5737gdxlw5i64vxljttuk6tp6h6kcgvqicxr2xg7j6fpd6k4dii",
            "mimeType": "image/jpeg",
            "size": 1024
        });

        let lex_value = LexValue::from(&blob_json);

        match lex_value {
            LexValue::Blob(blob_ref) => {
                assert_eq!(blob_ref.mime_type, "image/jpeg");
                assert_eq!(blob_ref.size, 1024);
                assert_eq!(
                    blob_ref.ref_.to_string(),
                    "bafyreie5737gdxlw5i64vxljttuk6tp6h6kcgvqicxr2xg7j6fpd6k4dii"
                );
            }
            other => panic!("Expected BlobRef, got: {:#?}", other),
        }
    }

    #[test]
    fn test_serde_roundtrip() {
        let original = LexValue::Ipld(IpldValue::String("test".to_string()));
        let serialized = serde_json::to_string(&original).unwrap();
        let deserialized: LexValue = serde_json::from_str(&serialized).unwrap();
        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_invalid_blob_handling() {
        let invalid_blobs = vec![
            serde_json::json!({
                "$type": "blob",
                // Missing ref
                "mimeType": "image/jpeg",
                "size": 1024
            }),
            serde_json::json!({
                "$type": "blob",
                "ref": "not-a-valid-cid",
                "mimeType": "image/jpeg",
                "size": 1024
            }),
            serde_json::json!({
                "$type": "blob",
                "ref": "bafyreie5737gdxlw5i64vxljttuk6tp6h6kcgvqicxr2xg7j6fpd6k4dii",
                // Missing mimeType
                "size": 1024
            }),
        ];

        for blob in invalid_blobs {
            let lex_value = LexValue::from(&blob);
            match lex_value {
                LexValue::Object(_) => (), // Should be converted to regular object
                other => panic!("Expected Object, got: {:#?}", other),
            }
        }
    }

    #[test]
    fn test_nested_array_conversion() {
        let nested = serde_json::json!([
            [1.0, 2.0, 3.0],
            {"a": "b"},
            [{"x": [1.0, 2.0]}]
        ]);

        let lex_value = LexValue::from(&nested);
        let json_again: serde_json::Value = (&lex_value).into();
        assert_eq!(nested, json_again);
    }

    #[test]
    fn test_special_fields() {
        let special = serde_json::json!({
            "$type": "not-a-blob",
            "$special": "field",
            "normal": "field"
        });

        let lex_value = LexValue::from(&special);
        let json_again: serde_json::Value = (&lex_value).into();
        assert_eq!(special, json_again);
    }

    #[test]
    fn test_untyped_blob_ref() {
        let untyped_json = serde_json::json!({
            "cid": "bafyreie5737gdxlw5i64vxljttuk6tp6h6kcgvqicxr2xg7j6fpd6k4dii",
            "mimeType": "image/jpeg"
        });

        let lex_value = LexValue::from(&untyped_json);
        let json_again: serde_json::Value = (&lex_value).into();
        
        if let serde_json::Value::Object(map) = &json_again {
            if let Some(cid_val) = map.get("cid") {
                if let serde_json::Value::Object(link_map) = cid_val {
                    if let Some(link_str) = link_map.get("$link") {
                        assert_eq!(
                            link_str.as_str().unwrap(),
                            "bafyreie5737gdxlw5i64vxljttuk6tp6h6kcgvqicxr2xg7j6fpd6k4dii"
                        );
                        return;
                    }
                }
            }
        }
        panic!("Expected link object with correct CID");
    }

    #[test]
    fn test_bytes_handling() {
        let test_bytes = vec![1, 2, 3, 4];
        let original = LexValue::Ipld(IpldValue::Bytes(test_bytes.clone()));

        let json: serde_json::Value = (&original).into();
        let decoded = BASE64.decode(
            json.as_object()
                .and_then(|obj| obj.get("$bytes"))
                .and_then(|val| val.as_str())
                .expect("Expected base64 string"),
        )
        .expect("Expected valid base64");
        
        assert_eq!(decoded, test_bytes);
    }
}
