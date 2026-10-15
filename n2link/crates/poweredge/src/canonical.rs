//! Canonical JSON (sorted object keys, no whitespace) and SHA-256 digests.
//!
//! Implemented by hand so the result never depends on serde_json's `preserve_order`
//! feature, which other crates in the workspace may enable through feature unification.

use serde_json::Value;
use sha2::{Digest, Sha256};

pub fn to_canonical_string(value: &Value) -> String {
    let mut out = String::new();
    write(value, &mut out);
    out
}

fn write(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (index, key) in keys.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String((*key).clone()).to_string());
                out.push(':');
                write(&map[*key], out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write(item, out);
            }
            out.push(']');
        }
        scalar => out.push_str(&scalar.to_string()),
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn digest(value: &Value) -> String {
    sha256_hex(to_canonical_string(value).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn key_order_does_not_change_the_digest() {
        let a: Value = serde_json::from_str(r#"{"b":1,"a":{"y":[1,2],"x":"s"}}"#).unwrap();
        let b: Value = serde_json::from_str(r#"{"a":{"x":"s","y":[1,2]},"b":1}"#).unwrap();
        assert_eq!(to_canonical_string(&a), r#"{"a":{"x":"s","y":[1,2]},"b":1}"#);
        assert_eq!(digest(&a), digest(&b));
    }

    #[test]
    fn array_order_changes_the_digest() {
        assert_ne!(digest(&json!([1, 2])), digest(&json!([2, 1])));
    }
}
