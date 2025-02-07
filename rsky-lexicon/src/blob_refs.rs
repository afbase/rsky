use std::collections::HashMap;
use libipld::Cid;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use serde::ser::SerializeStruct;
use rsky_common_web::ipld::IpldValue;

#[derive(Debug, Error)]
pub enum BlobRefError {
    #[error("Invalid blob reference format")]
    InvalidFormat,
    #[error("Invalid CID: {0}")]
    InvalidCid(#[from] libipld::cid::Error),
    #[error("Missing required field: {0}")]
    MissingField(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypedJsonBlobRef {
    pub type_: String,
    pub ref_: Cid,
    pub mime_type: String,
    pub size: i64,
}

impl Serialize for TypedJsonBlobRef {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("TypedJsonBlobRef", 4)?;
        state.serialize_field("$type", &self.type_)?;
        state.serialize_field("ref", &self.ref_.to_string())?;
        state.serialize_field("mimeType", &self.mime_type)?;
        state.serialize_field("size", &self.size)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for TypedJsonBlobRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Helper {
            #[serde(rename = "$type")]
            type_: String,
            #[serde(rename = "ref")]
            ref_str: String,
            #[serde(rename = "mimeType")]
            mime_type: String,
            size: i64,
        }

        let helper = Helper::deserialize(deserializer)?;
        let ref_ = Cid::try_from(helper.ref_str.as_str())
            .map_err(serde::de::Error::custom)?;

        Ok(TypedJsonBlobRef {
            type_: helper.type_,
            ref_,
            mime_type: helper.mime_type,
            size: helper.size,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UntypedJsonBlobRef {
    pub cid: String,
    pub mime_type: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum JsonBlobRef {
    Typed(TypedJsonBlobRef),
    Untyped(UntypedJsonBlobRef),
}

#[derive(Debug, Clone, PartialEq)]
pub struct BlobRef {
    pub ref_: Cid,
    pub mime_type: String,
    pub size: i64,
    pub original: JsonBlobRef,
}

impl BlobRef {
    pub fn new(ref_: Cid, mime_type: String, size: i64) -> Self {
        let original = JsonBlobRef::Typed(TypedJsonBlobRef {
            type_: "blob".to_string(),
            ref_,
            mime_type: mime_type.clone(),
            size,
        });

        Self {
            ref_,
            mime_type,
            size,
            original,
        }
    }
}

impl TryFrom<JsonBlobRef> for BlobRef {
    type Error = BlobRefError;

    fn try_from(json_ref: JsonBlobRef) -> Result<Self, Self::Error> {
        match json_ref {
            JsonBlobRef::Typed(typed) => Ok(Self {
                ref_: typed.ref_,
                mime_type: typed.mime_type.clone(),
                size: typed.size,
                original: JsonBlobRef::Typed(typed),
            }),
            JsonBlobRef::Untyped(untyped) => {
                let ref_ = Cid::try_from(untyped.cid.as_str())?;
                Ok(Self {
                    ref_,
                    mime_type: untyped.mime_type.clone(),
                    size: -1,
                    original: JsonBlobRef::Untyped(untyped),
                })
            }
        }
    }
}

impl From<&BlobRef> for TypedJsonBlobRef {
    fn from(blob: &BlobRef) -> Self {
        TypedJsonBlobRef {
            type_: "blob".to_string(),
            ref_: blob.ref_,
            mime_type: blob.mime_type.clone(),
            size: blob.size,
        }
    }
}

impl TryFrom<&HashMap<String, IpldValue>> for BlobRef {
    type Error = BlobRefError;

    fn try_from(value: &HashMap<String, IpldValue>) -> Result<Self, Self::Error> {
        if let Some(IpldValue::String(type_)) = value.get("$type") {
            if type_ == "blob" {
                let ref_ = match value.get("ref") {
                    Some(IpldValue::String(s)) => Cid::try_from(s.as_str())
                        .map_err(BlobRefError::InvalidCid)?,
                    Some(IpldValue::Cid(cid)) => *cid,
                    _ => return Err(BlobRefError::MissingField("ref".to_string())),
                };

                let mime_type = match value.get("mimeType") {
                    Some(IpldValue::String(s)) => s.clone(),
                    _ => return Err(BlobRefError::MissingField("mimeType".to_string())),
                };

                let size = match value.get("size") {
                    Some(IpldValue::Number(n)) => *n as i64,
                    _ => return Err(BlobRefError::MissingField("size".to_string())),
                };

                return Ok(BlobRef::new(ref_, mime_type, size));
            }
        }

        if let (Some(IpldValue::String(cid)), Some(IpldValue::String(mime_type))) = 
            (value.get("cid"), value.get("mimeType")) 
        {
            let untyped = UntypedJsonBlobRef {
                cid: cid.clone(),
                mime_type: mime_type.clone(),
            };
            return BlobRef::try_from(JsonBlobRef::Untyped(untyped));
        }

        Err(BlobRefError::InvalidFormat)
    }
}

impl Serialize for JsonBlobRef {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            JsonBlobRef::Typed(typed) => typed.serialize(serializer),
            JsonBlobRef::Untyped(untyped) => untyped.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for JsonBlobRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        
        if let Ok(typed) = serde_json::from_value::<TypedJsonBlobRef>(value.clone()) {
            Ok(JsonBlobRef::Typed(typed))
        } else if let Ok(untyped) = serde_json::from_value::<UntypedJsonBlobRef>(value) {
            Ok(JsonBlobRef::Untyped(untyped))
        } else {
            Err(serde::de::Error::custom("Invalid blob reference format"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_blob_ref_serialization() {
        let cid = Cid::try_from("bafyreie5737gdxlw5i64vxljttuk6tp6h6kcgvqicxr2xg7j6fpd6k4dii").unwrap();
        let blob_ref = BlobRef::new(cid, "image/jpeg".to_string(), 1024);

        let typed_json: TypedJsonBlobRef = (&blob_ref).into();
        let json = serde_json::to_value(&typed_json).unwrap();
        let expected = json!({
            "$type": "blob",
            "ref": "bafyreie5737gdxlw5i64vxljttuk6tp6h6kcgvqicxr2xg7j6fpd6k4dii",
            "mimeType": "image/jpeg",
            "size": 1024
        });

        assert_eq!(json, expected);
    }

    #[test]
    fn test_blob_ref_try_from_json_ref() {
        let typed = TypedJsonBlobRef {
            type_: "blob".to_string(),
            ref_: Cid::try_from("bafyreie5737gdxlw5i64vxljttuk6tp6h6kcgvqicxr2xg7j6fpd6k4dii").unwrap(),
            mime_type: "image/jpeg".to_string(),
            size: 1024,
        };
        let json_ref = JsonBlobRef::Typed(typed);
        
        let blob_ref = BlobRef::try_from(json_ref).unwrap();
        assert_eq!(blob_ref.mime_type, "image/jpeg");
        assert_eq!(blob_ref.size, 1024);
    }

    #[test]
    fn test_untyped_blob_ref() {
        let untyped = UntypedJsonBlobRef {
            cid: "bafyreie5737gdxlw5i64vxljttuk6tp6h6kcgvqicxr2xg7j6fpd6k4dii".to_string(),
            mime_type: "image/jpeg".to_string(),
        };
        let json_ref = JsonBlobRef::Untyped(untyped);
        
        let blob_ref = BlobRef::try_from(json_ref).unwrap();
        assert_eq!(blob_ref.size, -1);
    }
}