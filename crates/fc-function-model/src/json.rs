//! An ordered JSON tree, read the way Java's Jackson reads it.
//!
//! The manifest parser needs two things `serde_json::Value` cannot promise
//! in this crate: object keys in document order (problems are reported in
//! document order, and a crate feature should not decide that), and
//! integers of any size kept exactly (a schedule's `payload` is opaque and
//! stored as given).
//!
//! Reading follows Jackson 3's `readTree` defaults: strict RFC 8259, one
//! value with nothing after it, a repeated key keeps its first position and
//! takes the last value. Writing is serde_json's, with object keys in the
//! tree's order and integers written as their exact text.

use std::fmt;

use indexmap::IndexMap;
use serde::de::Error as _;

/// A JSON value.
#[derive(Debug, Clone, PartialEq)]
pub enum JsonNode {
    Null,
    Bool(bool),
    Number(JsonNumber),
    String(String),
    Array(Vec<JsonNode>),
    /// Keys in document order.
    Object(IndexMap<String, JsonNode>),
}

/// A JSON number, split the way Jackson's tree splits it.
#[derive(Debug, Clone, PartialEq)]
pub enum JsonNumber {
    /// An integral literal (Jackson's `IntNode` / `LongNode` /
    /// `BigIntegerNode`), kept as its canonical decimal text, of any size.
    Integer(String),
    /// A literal with a fraction or an exponent (Jackson's `DoubleNode`).
    Float(f64),
}

/// A document that is not a single valid JSON value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid JSON at byte {offset}: {reason}")]
pub struct JsonParseError {
    pub offset: usize,
    pub reason: &'static str,
}

/// Jackson's default maximum nesting depth.
const MAX_DEPTH: usize = 1000;

impl JsonNode {
    /// Parses one JSON document.
    pub fn parse(text: &str) -> Result<JsonNode, JsonParseError> {
        let mut parser = Parser {
            bytes: text.as_bytes(),
            text,
            pos: 0,
        };
        parser.skip_ws();
        let value = parser.value(0)?;
        parser.skip_ws();
        if parser.pos != parser.bytes.len() {
            return Err(parser.error("trailing content after the JSON value"));
        }
        Ok(value)
    }

    /// An empty object.
    pub fn object() -> JsonNode {
        JsonNode::Object(IndexMap::new())
    }

    /// A string value.
    pub fn string(value: impl Into<String>) -> JsonNode {
        JsonNode::String(value.into())
    }

    /// An integer value.
    pub fn int(value: i64) -> JsonNode {
        JsonNode::Number(JsonNumber::Integer(value.to_string()))
    }

    /// The member `key` of an object; `None` for a missing key or a
    /// non-object (Jackson's `path(key)` answering a missing node).
    pub fn get(&self, key: &str) -> Option<&JsonNode> {
        match self {
            JsonNode::Object(map) => map.get(key),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, JsonNode::Null)
    }

    pub fn is_object(&self) -> bool {
        matches!(self, JsonNode::Object(_))
    }

    /// The text of a string node, and nothing else.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsonNode::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            JsonNode::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[JsonNode]> {
        match self {
            JsonNode::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&IndexMap<String, JsonNode>> {
        match self {
            JsonNode::Object(map) => Some(map),
            _ => None,
        }
    }

    /// The value of an integral number that fits a Java `int` (Jackson's
    /// `isIntegralNumber() && canConvertToInt()`); `None` for anything else,
    /// including `1.0` and `5000000000`.
    pub fn fits_int(&self) -> Option<i32> {
        match self {
            JsonNode::Number(JsonNumber::Integer(text)) => text.parse().ok(),
            _ => None,
        }
    }

    /// Jackson's `asString()` on a scalar: a string's text, `""` for null,
    /// `true`/`false`, and a number's text (an integer exactly, a float as
    /// serde_json writes it). Jackson throws for an
    /// array or object; this answers `""`, so the caller reports the field
    /// as missing rather than failing the whole request.
    pub fn scalar_text(&self) -> String {
        match self {
            JsonNode::String(s) => s.clone(),
            JsonNode::Null | JsonNode::Array(_) | JsonNode::Object(_) => String::new(),
            JsonNode::Bool(b) => b.to_string(),
            JsonNode::Number(JsonNumber::Integer(text)) => text.clone(),
            JsonNode::Number(JsonNumber::Float(f)) => {
                serde_json::Number::from_f64(*f).map_or_else(|| f.to_string(), |n| n.to_string())
            }
        }
    }

    /// The compact JSON text of this tree (see the [`serde::Serialize`]
    /// impl).
    pub fn to_json_string(&self) -> String {
        serde_json::to_string(self).expect("a JsonNode always serialises")
    }
}

impl fmt::Display for JsonNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_json_string())
    }
}

/// Object members in the tree's order. An integer that fits `i64`/`u64` is
/// written as one; a bigger one goes through `serde_json`'s raw-value
/// passthrough so its digits survive (which only `serde_json` understands).
/// A non-finite float (only `1e400`-style literals parse to one) is written
/// as serde_json writes it, `null`.
impl serde::Serialize for JsonNode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::{SerializeMap, SerializeSeq};
        match self {
            JsonNode::Null => serializer.serialize_unit(),
            JsonNode::Bool(b) => serializer.serialize_bool(*b),
            JsonNode::Number(JsonNumber::Float(f)) => serializer.serialize_f64(*f),
            JsonNode::Number(JsonNumber::Integer(text)) => {
                if let Ok(n) = text.parse::<i64>() {
                    serializer.serialize_i64(n)
                } else if let Ok(n) = text.parse::<u64>() {
                    serializer.serialize_u64(n)
                } else {
                    serde_json::value::RawValue::from_string(text.clone())
                        .map_err(serde::ser::Error::custom)?
                        .serialize(serializer)
                }
            }
            JsonNode::String(s) => serializer.serialize_str(s),
            JsonNode::Array(items) => {
                let mut seq = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            JsonNode::Object(map) => {
                let mut out = serializer.serialize_map(Some(map.len()))?;
                for (k, v) in map {
                    out.serialize_entry(k, v)?;
                }
                out.end()
            }
        }
    }
}

/// Read from the raw JSON text, so key order and big integers survive a
/// request DTO. Only works through `serde_json`, like `RawValue` itself.
impl<'de> serde::Deserialize<'de> for JsonNode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = Box::<serde_json::value::RawValue>::deserialize(deserializer)?;
        JsonNode::parse(raw.get()).map_err(D::Error::custom)
    }
}

/// A `serde_json` tree (the function host reads the desired-state document
/// with `serde_json`), node for node. An integral number keeps its exact
/// text and anything else becomes a float, as [`JsonNode::parse`] splits
/// them; object keys keep the order the `Value` holds them in (document
/// order under `serde_json`'s `preserve_order`).
impl From<&serde_json::Value> for JsonNode {
    fn from(value: &serde_json::Value) -> Self {
        match value {
            serde_json::Value::Null => JsonNode::Null,
            serde_json::Value::Bool(b) => JsonNode::Bool(*b),
            serde_json::Value::Number(n) if n.is_f64() => {
                JsonNode::Number(JsonNumber::Float(n.as_f64().unwrap_or(f64::NAN)))
            }
            serde_json::Value::Number(n) => JsonNode::Number(JsonNumber::Integer(n.to_string())),
            serde_json::Value::String(s) => JsonNode::String(s.clone()),
            serde_json::Value::Array(items) => {
                JsonNode::Array(items.iter().map(JsonNode::from).collect())
            }
            serde_json::Value::Object(map) => JsonNode::Object(
                map.iter()
                    .map(|(k, v)| (k.clone(), JsonNode::from(v)))
                    .collect(),
            ),
        }
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    text: &'a str,
    pos: usize,
}

impl Parser<'_> {
    fn error(&self, reason: &'static str) -> JsonParseError {
        JsonParseError {
            offset: self.pos,
            reason,
        }
    }

    fn skip_ws(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.bytes.get(self.pos) {
            self.pos += 1;
        }
    }

    fn value(&mut self, depth: usize) -> Result<JsonNode, JsonParseError> {
        if depth > MAX_DEPTH {
            return Err(self.error("nesting too deep"));
        }
        match self.bytes.get(self.pos) {
            None => Err(self.error("unexpected end of input")),
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => Ok(JsonNode::String(self.string()?)),
            Some(b't') => self.literal("true", JsonNode::Bool(true)),
            Some(b'f') => self.literal("false", JsonNode::Bool(false)),
            Some(b'n') => self.literal("null", JsonNode::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.error("unexpected character")),
        }
    }

    fn literal(&mut self, word: &'static str, value: JsonNode) -> Result<JsonNode, JsonParseError> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(value)
        } else {
            Err(self.error("unrecognised literal"))
        }
    }

    fn object(&mut self, depth: usize) -> Result<JsonNode, JsonParseError> {
        self.pos += 1; // {
        let mut map = IndexMap::new();
        self.skip_ws();
        if self.bytes.get(self.pos) == Some(&b'}') {
            self.pos += 1;
            return Ok(JsonNode::Object(map));
        }
        loop {
            self.skip_ws();
            if self.bytes.get(self.pos) != Some(&b'"') {
                return Err(self.error("expected a string key"));
            }
            let key = self.string()?;
            self.skip_ws();
            if self.bytes.get(self.pos) != Some(&b':') {
                return Err(self.error("expected ':'"));
            }
            self.pos += 1;
            self.skip_ws();
            let value = self.value(depth + 1)?;
            // A repeated key keeps its first position and takes the last
            // value, as Jackson's ObjectNode does.
            map.insert(key, value);
            self.skip_ws();
            match self.bytes.get(self.pos) {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(JsonNode::Object(map));
                }
                _ => return Err(self.error("expected ',' or '}'")),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<JsonNode, JsonParseError> {
        self.pos += 1; // [
        let mut items = Vec::new();
        self.skip_ws();
        if self.bytes.get(self.pos) == Some(&b']') {
            self.pos += 1;
            return Ok(JsonNode::Array(items));
        }
        loop {
            self.skip_ws();
            items.push(self.value(depth + 1)?);
            self.skip_ws();
            match self.bytes.get(self.pos) {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(JsonNode::Array(items));
                }
                _ => return Err(self.error("expected ',' or ']'")),
            }
        }
    }

    fn string(&mut self) -> Result<String, JsonParseError> {
        self.pos += 1; // opening quote
        let mut out = String::new();
        loop {
            let start = self.pos;
            while let Some(&b) = self.bytes.get(self.pos) {
                if b == b'"' || b == b'\\' || b < 0x20 {
                    break;
                }
                self.pos += 1;
            }
            // `start..pos` only ever splits at ASCII bytes, so it is a
            // character boundary of the (valid UTF-8) input.
            out.push_str(&self.text[start..self.pos]);
            match self.bytes.get(self.pos) {
                None => return Err(self.error("unterminated string")),
                Some(b'"') => {
                    self.pos += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.pos += 1;
                    self.escape(&mut out)?;
                }
                Some(_) => return Err(self.error("unescaped control character in string")),
            }
        }
    }

    fn escape(&mut self, out: &mut String) -> Result<(), JsonParseError> {
        let Some(&b) = self.bytes.get(self.pos) else {
            return Err(self.error("unterminated escape"));
        };
        self.pos += 1;
        match b {
            b'"' => out.push('"'),
            b'\\' => out.push('\\'),
            b'/' => out.push('/'),
            b'b' => out.push('\u{08}'),
            b'f' => out.push('\u{0C}'),
            b'n' => out.push('\n'),
            b'r' => out.push('\r'),
            b't' => out.push('\t'),
            b'u' => {
                let unit = self.hex4()?;
                let c = if (0xD800..0xDC00).contains(&unit) {
                    if !self.bytes[self.pos..].starts_with(b"\\u") {
                        return Err(self.error("unpaired surrogate escape"));
                    }
                    self.pos += 2;
                    let low = self.hex4()?;
                    if !(0xDC00..0xE000).contains(&low) {
                        return Err(self.error("unpaired surrogate escape"));
                    }
                    let code = 0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00);
                    char::from_u32(code).ok_or_else(|| self.error("invalid escape"))?
                } else {
                    char::from_u32(unit).ok_or_else(|| self.error("unpaired surrogate escape"))?
                };
                out.push(c);
            }
            _ => return Err(self.error("unrecognised escape")),
        }
        Ok(())
    }

    fn hex4(&mut self) -> Result<u32, JsonParseError> {
        let digits = self
            .bytes
            .get(self.pos..self.pos + 4)
            .ok_or_else(|| self.error("short \\u escape"))?;
        let mut value = 0u32;
        for &d in digits {
            let v = (d as char)
                .to_digit(16)
                .ok_or_else(|| self.error("bad hex digit in \\u escape"))?;
            value = value * 16 + v;
        }
        self.pos += 4;
        Ok(value)
    }

    fn number(&mut self) -> Result<JsonNode, JsonParseError> {
        let start = self.pos;
        let negative = self.bytes[self.pos] == b'-';
        if negative {
            self.pos += 1;
        }
        match self.bytes.get(self.pos) {
            Some(b'0') => {
                self.pos += 1;
                if matches!(self.bytes.get(self.pos), Some(b'0'..=b'9')) {
                    return Err(self.error("leading zero"));
                }
            }
            Some(b'1'..=b'9') => self.digits(),
            _ => return Err(self.error("expected a digit")),
        }
        let mut float = false;
        if self.bytes.get(self.pos) == Some(&b'.') {
            float = true;
            self.pos += 1;
            if !matches!(self.bytes.get(self.pos), Some(b'0'..=b'9')) {
                return Err(self.error("expected a fraction digit"));
            }
            self.digits();
        }
        if let Some(b'e' | b'E') = self.bytes.get(self.pos) {
            float = true;
            self.pos += 1;
            if let Some(b'+' | b'-') = self.bytes.get(self.pos) {
                self.pos += 1;
            }
            if !matches!(self.bytes.get(self.pos), Some(b'0'..=b'9')) {
                return Err(self.error("expected an exponent digit"));
            }
            self.digits();
        }
        let literal = &self.text[start..self.pos];
        if float {
            let value: f64 = literal
                .parse()
                .map_err(|_| self.error("unreadable number"))?;
            Ok(JsonNode::Number(JsonNumber::Float(value)))
        } else {
            // Canonical integer text: only `-0` differs from its literal.
            let canonical = if literal == "-0" { "0" } else { literal };
            Ok(JsonNode::Number(JsonNumber::Integer(canonical.to_string())))
        }
    }

    fn digits(&mut self) {
        while let Some(b'0'..=b'9') = self.bytes.get(self.pos) {
            self.pos += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round(text: &str) -> String {
        JsonNode::parse(text).unwrap().to_json_string()
    }

    #[test]
    fn keeps_document_order_and_last_value_of_a_repeated_key() {
        assert_eq!(round(r#"{"b":1,"a":2,"b":3}"#), r#"{"b":3,"a":2}"#);
    }

    #[test]
    fn numbers_keep_their_value_and_integers_their_digits() {
        let text = "[1e10,1e-5,1e3,0.001,-0.0,123456.789,-0,12345678901234567890123,1.5]";
        let written = round(text);
        assert_eq!(
            JsonNode::parse(&written).unwrap(),
            JsonNode::parse(text).unwrap()
        );
        assert!(written.contains("12345678901234567890123"), "{written}");
    }

    #[test]
    fn strings_round_trip() {
        let all_controls: String = (0u8..0x20).map(char::from).collect();
        let node = JsonNode::String(format!("{all_controls}\"\\/\u{7f}\u{e9}\u{2028}<>&'"));
        assert_eq!(JsonNode::parse(&node.to_json_string()).unwrap(), node);
    }

    #[test]
    fn escapes_decode_including_surrogate_pairs() {
        let node = JsonNode::parse(r#""é😀\/""#).unwrap();
        assert_eq!(node.as_str(), Some("\u{e9}\u{1F600}/"));
        assert!(JsonNode::parse(r#""\ud83d""#).is_err());
    }

    #[test]
    fn rejects_what_jackson_rejects() {
        for bad in [
            "",
            "{",
            "[1,]",
            "{\"a\":1,}",
            "01",
            "1.",
            ".5",
            "+1",
            "NaN",
            "'a'",
            "{a:1}",
            "[1] [2]",
            "\"a\u{1}\"",
            "tru",
            "// c\n1",
        ] {
            assert!(JsonNode::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn fits_int_is_integral_and_in_int_range() {
        let v = |t: &str| JsonNode::parse(t).unwrap().fits_int();
        assert_eq!(v("2147483647"), Some(i32::MAX));
        assert_eq!(v("-2147483648"), Some(i32::MIN));
        assert_eq!(v("2147483648"), None);
        assert_eq!(v("5000000000"), None);
        assert_eq!(v("1.0"), None);
        assert_eq!(v("1e2"), None);
        assert_eq!(v("\"1\""), None);
    }

    #[test]
    fn scalar_text_is_jacksons_as_string() {
        let v = |t: &str| JsonNode::parse(t).unwrap().scalar_text();
        assert_eq!(v("null"), "");
        assert_eq!(v("true"), "true");
        assert_eq!(v("1.5"), "1.5");
        assert_eq!(v("12"), "12");
        assert_eq!(v("\"x\""), "x");
    }

    #[test]
    fn serde_round_trip_keeps_order_and_big_integers() {
        #[derive(serde::Deserialize, serde::Serialize)]
        struct Dto {
            manifest: JsonNode,
        }
        let dto: Dto =
            serde_json::from_str(r#"{"manifest":{"z":1,"a":123456789012345678901234}}"#).unwrap();
        assert_eq!(
            serde_json::to_string(&dto).unwrap(),
            r#"{"manifest":{"z":1,"a":123456789012345678901234}}"#
        );
    }

    #[test]
    fn from_serde_value_splits_numbers_as_parse_does() {
        let text = r#"{"b":1,"a":[true,null,-7,1.5,1e3,"x"],"o":{}}"#;
        let value: serde_json::Value = serde_json::from_str(text).unwrap();
        let node = JsonNode::from(&value);
        let a = node.get("a").unwrap().as_array().unwrap();
        assert_eq!(a[2].fits_int(), Some(-7));
        assert_eq!(a[3], JsonNode::Number(JsonNumber::Float(1.5)));
        assert_eq!(a[4], JsonNode::Number(JsonNumber::Float(1000.0)));
        assert_eq!(node.get("b").unwrap().fits_int(), Some(1));
        assert!(node.get("o").unwrap().is_object());
        // Key order is whatever the Value holds; the content is the same.
        let parsed = JsonNode::parse(text).unwrap();
        let (ours, theirs) = (node.as_object().unwrap(), parsed.as_object().unwrap());
        assert_eq!(ours.len(), theirs.len());
        for (k, v) in theirs {
            assert_eq!(ours.get(k), Some(v), "{k}");
        }
    }
}
