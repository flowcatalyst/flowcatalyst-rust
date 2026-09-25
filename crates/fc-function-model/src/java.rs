//! Small helpers that reproduce JDK / Jackson behaviour Java relies on, so
//! parsing decisions land the same way on the platform, the function host
//! and the signature verifier (each of which used to carry its own copy).

use serde_json::Value;

/// `Character.isWhitespace`, the rule `String.isBlank` uses: the ASCII
/// controls `\t \n \x0B \f \r \x1C-\x1F`, and every Unicode space, line or
/// paragraph separator except the no-break ones (U+00A0, U+2007, U+202F).
/// U+0085 is not whitespace to Java.
pub fn is_java_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | '\u{1C}'..='\u{1F}' | ' ' | '\u{1680}'
            | '\u{2000}'..='\u{2006}' | '\u{2008}'..='\u{200A}' | '\u{2028}' | '\u{2029}'
            | '\u{205F}' | '\u{3000}'
    )
}

/// `String.isBlank`: empty, or only [`is_java_whitespace`] characters.
pub fn is_blank(s: &str) -> bool {
    s.chars().all(is_java_whitespace)
}

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
    fn blank_follows_character_is_whitespace() {
        assert!(is_blank(""));
        assert!(is_blank(" \t\n"));
        assert!(is_blank(" \t\u{0B}\u{1C}\u{2003}\u{3000}"));
        assert!(!is_blank("\u{00A0}"));
        assert!(!is_blank("\u{0085}"));
        assert!(!is_blank("\u{2007}"));
        assert!(!is_blank("\u{202F}"));
        assert!(!is_blank(" x "));
        assert!(!is_blank("x"));
    }

    /// The explicit table agrees with the definition the host and the
    /// verifier used to carry (`char::is_whitespace` minus the no-break
    /// spaces and U+0085, plus `\x1C-\x1F`), over every `char`.
    #[test]
    fn whitespace_table_matches_the_unicode_definition() {
        let by_definition = |c: char| {
            matches!(c, '\u{1C}'..='\u{1F}')
                || (c.is_whitespace()
                    && !matches!(c, '\u{A0}' | '\u{2007}' | '\u{202F}' | '\u{85}'))
        };
        for c in (0..=0x10FFFF).filter_map(char::from_u32) {
            assert_eq!(is_java_whitespace(c), by_definition(c), "{:?}", c);
        }
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
