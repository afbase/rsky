# Lexicon and Blob Reference Implementation Notes

This document explains how the Rust implementation differs from the TypeScript [implementation](https://github.com/bluesky-social/atproto/tree/bfa178ca62612884e5b4e0a4304bed74d923ae7c/packages/lexicon/src) by leveraging Rust's trait system.

## Lexicon Serialization

### TypeScript Functions
The TypeScript implementation uses explicit functions:
- `lexToIpld()`
- `ipldToLex()`
- `lexToJson()`
- `stringifyLex()`
- `jsonToLex()`
- `jsonStringToLex()`

### Rust Traits for Lexicon
The Rust implementation replaces these with trait implementations:
- `From<IpldValue> for LexValue` - replaces `ipldToLex`
- `From<LexValue> for IpldValue` - replaces `lexToIpld`
- `From<&LexValue> for serde_json::Value` - replaces `lexToJson`
- `From<&serde_json::Value> for LexValue` - replaces `jsonToLex`
- `Serialize for LexValue` - replaces `stringifyLex`
- `Deserialize for LexValue` - replaces `jsonStringToLex`

### Rust Traits for Blobs
The Rust implementation replaces methods with traits:
- `TryFrom<JsonBlobRef> for BlobRef` - replaces `fromJsonRef`
- `From<&BlobRef> for TypedJsonBlobRef` - replaces `ipld()`
- `TryFrom<&HashMap<String, IpldValue>>` - replaces `asBlobRef`
- `Serialize/Deserialize for JsonBlobRef` - replaces `toJSON()`

### Blob Reference Example Usage
```rust
// Converting from JsonBlobRef (replaces fromJsonRef)
let json_ref = JsonBlobRef::Typed(typed_ref);
let blob_ref = BlobRef::try_from(json_ref)?;

// Converting to TypedJsonBlobRef (replaces ipld())
let typed_json: TypedJsonBlobRef = (&blob_ref).into();

// Converting from IPLD HashMap (replaces asBlobRef)
let blob_ref = BlobRef::try_from(&ipld_map)?;

// Serializing (replaces toJSON)
let json_string = serde_json::to_string(&blob_ref)?;
```

## Example Usage

### Lexicon Conversions
```rust
use serde_json::json;

// Create a LexValue
let lex_value = LexValue::Ipld(IpldValue::String("test".to_string()));

// Convert to IPLD
let ipld: IpldValue = lex_value.clone().into();

// Convert to JSON Value
let json_value: serde_json::Value = (&lex_value).into();

// Convert from JSON Value
let json = json!({ "test": "value" });
let from_json = LexValue::from(&json);
```

### String Serialization
```rust
// Convert LexValue to JSON string
let lex_value = LexValue::Ipld(IpldValue::String("test".to_string()));
let json_string = serde_json::to_string(&lex_value).unwrap();

// Parse JSON string to LexValue
let parsed: LexValue = serde_json::from_str(&json_string).unwrap();
```

### Complex Blob Example
```rust
use std::collections::HashMap;
use libipld::Cid;

// Create a blob reference
let cid = Cid::try_from("bafyreie5737gdxlw5i64vxljttuk6tp6h6kcgvqicxr2xg7j6fpd6k4dii").unwrap();
let blob_ref = BlobRef::new(cid, "image/jpeg".to_string(), 1024);

// Convert to TypedJsonBlobRef
let typed_json: TypedJsonBlobRef = (&blob_ref).into();

// Serialize to JSON
let json_string = serde_json::to_string(&typed_json).unwrap();
println!("Serialized: {}", json_string);

// Deserialize back to BlobRef
let deserialized: JsonBlobRef = serde_json::from_str(&json_string).unwrap();
let blob_ref_again = BlobRef::try_from(deserialized).unwrap();
```