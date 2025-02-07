# IPLD Implementation Notes

This document explains key differences between the TypeScript and Rust implementations of [IPLD handling](https://github.com/bluesky-social/atproto/blob/bfa178c/packages/common-web/src/ipld.ts) in the AT Protocol, focusing on idiomatic Rust patterns and type system usage.

## Key Differences

### Type System Approach

**TypeScript Implementation:**
- Uses explicit type definitions (`JsonValue`, `IpldValue`)
- Requires explicit conversion functions:
  - `jsonToIpld()`
  - `ipldToJson()`
  - `ipldEquals()`

**Rust Implementation:**
- Leverages `serde_json::Value` for JSON handling
- Uses Rust traits for conversions and comparisons:
  - `From<Value> for IpldValue`
  - `From<IpldValue> for Value`
  - `PartialEq for IpldValue`

The Rust approach provides several advantages:
1. More idiomatic code using the type system
2. Better type safety through trait bounds
3. Automatic conversions via `From/Into` traits
4. Cleaner integration with the Rust ecosystem

## Code Examples

### Basic Usage

```rust
use serde_json::json;

// Create an IPLD value from JSON
let json_value = json!({
    "name": "example",
    "data": { "$bytes": "AQID" }
});
let ipld_value: IpldValue = json_value.into();

// Convert back to JSON
let json_again: serde_json::Value = ipld_value.into();
```

### Using Trait Implementations

```rust
use std::collections::HashMap;

// Creating an IPLD object with From trait
let mut map = HashMap::new();
map.insert("key".to_string(), IpldValue::String("value".to_string()));
let ipld = IpldValue::Object(map);
let json: serde_json::Value = ipld.into(); // Automatic conversion

// Comparing IPLD values with PartialEq
let ipld1 = IpldValue::String("test".to_string());
let ipld2 = IpldValue::String("test".to_string());
assert_eq!(ipld1, ipld2); // Uses PartialEq trait
```

### Serialization Example

```rust
// Serialize IPLD to string
let ipld = IpldValue::Number(42.0);
let serialized = serde_json::to_string(&ipld).unwrap();
assert_eq!(serialized, "42");

// Deserialize from string
let deserialized: IpldValue = serde_json::from_str(&serialized).unwrap();
assert_eq!(deserialized, ipld);
```