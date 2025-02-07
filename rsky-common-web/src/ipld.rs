use libipld::Cid;
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use serde::de::Deserializer;
use serde::ser::Serializer;
use base64::{Engine as _, engine::general_purpose::STANDARD as base64};
use serde_json::{Value, Map};

/// Represents IPLD-specific value types with support for CID and bytes
/// This enum implements the core IPLD data model for the atproto ecosystem
#[derive(Debug, Clone)]
pub enum IpldValue {
    /// Boolean values (true/false)
    Bool(bool),
    /// Numeric values (stored as f64 for compatibility, but floats are not allowed in atproto)
    Number(f64),
    /// UTF-8 encoded string values
    String(String),
    /// Null value
    Null,
    /// Ordered sequence of IPLD values
    Array(Vec<IpldValue>),
    /// Key-value mapping where keys are strings and values are IPLD values
    Object(HashMap<String, IpldValue>),
    /// Content-addressed identifier (CID) for linking to other IPLD data
    Cid(Cid),
    /// Raw binary data
    Bytes(Vec<u8>),
}

// Convert from serde_json::Value to IpldValue
impl From<Value> for IpldValue {
    fn from(val: Value) -> Self {
        match val {
            Value::Null => IpldValue::Null,
            Value::Bool(b) => IpldValue::Bool(b),
            Value::Number(n) => IpldValue::Number(n.as_f64().unwrap_or_default()),
            Value::String(s) => {
                // Try to parse as CID first - strings that are valid CIDs get special treatment
                if let Ok(cid) = Cid::try_from(s.as_str()) {
                    IpldValue::Cid(cid)
                } else {
                    IpldValue::String(s)
                }
            },
            Value::Array(arr) => {
                IpldValue::Array(arr.into_iter().map(IpldValue::from).collect())
            },
            Value::Object(obj) => {
                // Handle special DAG-JSON encodings
                if obj.len() == 1 {
                    // CIDs are encoded as {"$link": "cid-string"}
                    if let Some(Value::String(link)) = obj.get("$link") {
                        if let Ok(cid) = Cid::try_from(link.as_str()) {
                            return IpldValue::Cid(cid);
                        }
                    }
                    // Bytes are encoded as {"$bytes": "base64-string"}
                    if let Some(Value::String(bytes)) = obj.get("$bytes") {
                        if let Ok(decoded) = base64.decode(bytes) {
                            return IpldValue::Bytes(decoded);
                        }
                    }
                }
                
                // Regular object - recursively convert all values
                IpldValue::Object(
                    obj.into_iter()
                        .map(|(k, v)| (k, IpldValue::from(v)))
                        .collect()
                )
            }
        }
    }
}

// Convert from IpldValue to serde_json::Value
impl From<IpldValue> for Value {
    fn from(val: IpldValue) -> Self {
        match val {
            IpldValue::Null => Value::Null,
            IpldValue::Bool(b) => Value::Bool(b),
            IpldValue::Number(n) => {
                serde_json::Number::from_f64(n)
                    .map(Value::Number)
                    .unwrap_or(Value::Null)
            },
            IpldValue::String(s) => Value::String(s),
            IpldValue::Array(arr) => {
                Value::Array(arr.into_iter().map(Value::from).collect())
            },
            IpldValue::Object(obj) => {
                let map: Map<String, Value> = obj
                    .into_iter()
                    .map(|(k, v)| (k, Value::from(v)))
                    .collect();
                Value::Object(map)
            },
            IpldValue::Cid(cid) => {
                let mut map = Map::new();
                map.insert("$link".to_string(), Value::String(cid.to_string()));
                Value::Object(map)
            },
            IpldValue::Bytes(bytes) => {
                let mut map = Map::new();
                map.insert("$bytes".to_string(), Value::String(base64.encode(bytes)));
                Value::Object(map)
            }
        }
    }
}

// Enable serialization via serde
impl Serialize for IpldValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Convert to serde_json::Value first, then serialize that
        Value::from(self.clone()).serialize(serializer)
    }
}

// Enable deserialization via serde
impl<'de> Deserialize<'de> for IpldValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Deserialize into serde_json::Value first, then convert
        let value = Value::deserialize(deserializer)?;
        Ok(IpldValue::from(value))
    }
}

// Enable equality comparison between IpldValues
impl PartialEq for IpldValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (IpldValue::Bool(a), IpldValue::Bool(b)) => a == b,
            (IpldValue::Number(a), IpldValue::Number(b)) => a == b,
            (IpldValue::String(a), IpldValue::String(b)) => a == b,
            (IpldValue::Null, IpldValue::Null) => true,
            (IpldValue::Array(a), IpldValue::Array(b)) => {
                if a.len() != b.len() {
                    return false;
                }
                a.iter().zip(b.iter()).all(|(a, b)| a == b)
            }
            (IpldValue::Object(a), IpldValue::Object(b)) => {
                if a.len() != b.len() {
                    return false;
                }
                // For objects, compare all key-value pairs
                a.iter().all(|(k, v)| {
                    b.get(k).map_or(false, |bv| v == bv)
                })
            }
            (IpldValue::Cid(a), IpldValue::Cid(b)) => a == b,
            (IpldValue::Bytes(a), IpldValue::Bytes(b)) => a == b,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_ipld_serialization() {
        let cid = Cid::try_from("bafyreie5737gdxlw5i64vxljttuk6tp6h6kcgvqicxr2xg7j6fpd6k4dii").unwrap();
        let value = IpldValue::Object({
            let mut map = HashMap::new();
            map.insert("cid".to_string(), IpldValue::Cid(cid));
            map.insert("bytes".to_string(), IpldValue::Bytes(vec![1, 2, 3]));
            map
        });

        let serialized = serde_json::to_value(&value).unwrap();
        let expected = json!({
            "cid": { "$link": "bafyreie5737gdxlw5i64vxljttuk6tp6h6kcgvqicxr2xg7j6fpd6k4dii" },
            "bytes": { "$bytes": "AQID" }
        });
        
        assert_eq!(serialized, expected);
    }

    #[test]
    fn test_ipld_deserialization() {
        let json = json!({
            "cid": { "$link": "bafyreie5737gdxlw5i64vxljttuk6tp6h6kcgvqicxr2xg7j6fpd6k4dii" },
            "bytes": { "$bytes": "AQID" }
        });

        let value: IpldValue = serde_json::from_value(json).unwrap();
        
        match value {
            IpldValue::Object(map) => {
                assert_eq!(map.len(), 2);
                assert!(matches!(map.get("cid"), Some(IpldValue::Cid(_))));
                assert!(matches!(map.get("bytes"), Some(IpldValue::Bytes(_))));
            }
            _ => panic!("Expected object"),
        }
    }

    #[test]
    fn test_value_conversions() {
        // Test JSON -> IPLD -> JSON roundtrip
        let json = json!({
            "string": "test",
            "number": 42.0,
            "array": [1.0, 2.0, 3.0],
            "object": {"key": "value"},
            "cid": { "$link": "bafyreie5737gdxlw5i64vxljttuk6tp6h6kcgvqicxr2xg7j6fpd6k4dii" }
        });

        let ipld: IpldValue = json.clone().into();
        let roundtrip: Value = ipld.into();

        assert_eq!(json, roundtrip);
    }
}