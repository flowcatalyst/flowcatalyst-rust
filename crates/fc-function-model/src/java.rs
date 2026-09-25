//! Small helpers that reproduce JDK / Jackson behaviour Java relies on, so
//! parsing decisions land the same way on the platform, the function host
//! and the signature verifier (each of which used to carry its own copy).
//! `is_blank` itself lives in `fc-function-abi`, which the guest shares.

use serde_json::Value;

/// `Character.isWhitespace` and `String.isBlank`: the guest ABI's, the one
/// copy.
pub use fc_function_abi::java::{is_blank, is_java_whitespace};

/// Jackson 3's `JsonNode.asString(default)`: a string node's value, a number
/// or boolean node's text, and `default` for anything else (missing, null,
/// object, array).
pub fn as_string<'a>(node: Option<&'a Value>, default: Option<&'a str>) -> Option<String> {
    match node {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Number(n)) => Some(n.to_string()),
        Some(Value::Bool(b)) => Some(b.to_string()),
        _ => default.map(str::to_owned),
    }
}

/// Iterating a Jackson node: an array's elements, an object's values, and
/// nothing for a scalar or a missing node.
pub fn elements(node: Option<&Value>) -> Vec<&Value> {
    match node {
        Some(Value::Array(items)) => items.iter().collect(),
        Some(Value::Object(map)) => map.values().collect(),
        _ => Vec::new(),
    }
}

/// `isIntegralNumber() && canConvertToInt()`.
pub fn as_java_int(node: Option<&Value>) -> Option<i32> {
    match node {
        Some(Value::Number(n)) if n.is_i64() || n.is_u64() => {
            n.as_i64().and_then(|v| i32::try_from(v).ok())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn as_string_follows_jackson() {
        let v = json!({"s": "a", "n": 12, "b": true, "o": {}});
        assert_eq!(as_string(v.get("s"), None).as_deref(), Some("a"));
        assert_eq!(as_string(v.get("n"), None).as_deref(), Some("12"));
        assert_eq!(as_string(v.get("b"), None).as_deref(), Some("true"));
        assert_eq!(as_string(v.get("o"), Some("d")).as_deref(), Some("d"));
        assert_eq!(as_string(v.get("missing"), None), None);
    }

    #[test]
    fn elements_of_arrays_and_objects() {
        let v = json!({"a": [1, 2], "o": {"x": 1}, "s": "no"});
        assert_eq!(elements(v.get("a")).len(), 2);
        assert_eq!(elements(v.get("o")).len(), 1);
        assert!(elements(v.get("s")).is_empty());
        assert!(elements(None).is_empty());
    }

    #[test]
    fn java_int_rejects_floats_and_overflow() {
        assert_eq!(as_java_int(Some(&json!(3))), Some(3));
        assert_eq!(as_java_int(Some(&json!(3.0))), None);
        assert_eq!(as_java_int(Some(&json!(3_000_000_000_i64))), None);
        assert_eq!(as_java_int(Some(&json!("3"))), None);
    }
}
