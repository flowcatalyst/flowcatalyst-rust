//! Small helpers that reproduce JDK / Jackson behaviour Java's verifier
//! relies on, so parsing decisions land the same way in Rust.

use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use base64::Engine;
use serde_json::Value;

/// `java.util.Base64.getDecoder()`: the standard alphabet, padding optional,
/// no whitespace, lenient about trailing bits.
pub(crate) const JAVA_BASE64: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    GeneralPurposeConfig::new()
        .with_decode_padding_mode(DecodePaddingMode::Indifferent)
        .with_decode_allow_trailing_bits(true),
);

pub(crate) fn b64_decode(text: &str) -> Result<Vec<u8>, base64::DecodeError> {
    JAVA_BASE64.decode(text)
}

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

/// `new String(bytes, UTF_8)`: malformed sequences become U+FFFD.
pub(crate) fn utf8_lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
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
    fn base64_padding_is_optional() {
        assert_eq!(b64_decode("YQ").unwrap(), b"a");
        assert_eq!(b64_decode("YQ==").unwrap(), b"a");
        assert!(b64_decode("Y Q==").is_err());
    }
}
