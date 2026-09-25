//! Small helpers that reproduce JDK / Jackson behaviour the Java host relies
//! on, so parsing decisions land the same way on both hosts.

use serde_json::Value;

/// `Character.isWhitespace`, the rule `String.isBlank` uses.
pub(crate) fn is_java_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n'
            | '\u{000B}'
            | '\u{000C}'
            | '\r'
            | '\u{001C}'
            | '\u{001D}'
            | '\u{001E}'
            | '\u{001F}'
    ) || (c.is_whitespace()
        && !matches!(c, '\u{00A0}' | '\u{2007}' | '\u{202F}')
        && c != '\u{0085}')
}

/// `String.isBlank`.
pub(crate) fn is_blank(s: &str) -> bool {
    s.chars().all(is_java_whitespace)
}

/// Jackson 3's `JsonNode.asString(default)`: a string node's value, a number
/// or boolean node's text, and `default` for anything else (missing, null,
/// object, array).
pub(crate) fn as_string<'a>(node: Option<&'a Value>, default: Option<&'a str>) -> Option<String> {
    match node {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Number(n)) => Some(n.to_string()),
        Some(Value::Bool(b)) => Some(b.to_string()),
        _ => default.map(str::to_owned),
    }
}

/// Iterating a Jackson node: an array's elements, an object's values, and
/// nothing for a scalar or a missing node.
pub(crate) fn elements(node: Option<&Value>) -> Vec<&Value> {
    match node {
        Some(Value::Array(items)) => items.iter().collect(),
        Some(Value::Object(map)) => map.values().collect(),
        _ => Vec::new(),
    }
}

/// `isIntegralNumber() && canConvertToInt()`.
pub(crate) fn as_java_int(node: Option<&Value>) -> Option<i32> {
    match node {
        Some(Value::Number(n)) if n.is_i64() || n.is_u64() => {
            n.as_i64().and_then(|v| i32::try_from(v).ok())
        }
        _ => None,
    }
}

/// `String.length()`, in UTF-16 code units.
pub(crate) fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn blank_follows_java() {
        assert!(is_blank(""));
        assert!(is_blank(" \t\n"));
        assert!(!is_blank("\u{00A0}"));
        assert!(!is_blank("x"));
    }

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
    fn java_int_rejects_floats_and_overflow() {
        assert_eq!(as_java_int(Some(&json!(3))), Some(3));
        assert_eq!(as_java_int(Some(&json!(3.0))), None);
        assert_eq!(as_java_int(Some(&json!(3_000_000_000_i64))), None);
        assert_eq!(as_java_int(Some(&json!("3"))), None);
    }
}
