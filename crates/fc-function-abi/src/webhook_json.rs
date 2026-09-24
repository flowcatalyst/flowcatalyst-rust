//! A port of Java `function-api/src/main/java/io/flowcatalyst/function/WebhookJson.java`:
//! the minimal, strict, recursive-descent JSON reader behind
//! [`crate::Webhook`]. It works on UTF-16 code units, as Java's does on
//! `char`s, so offsets in messages and the handling of escaped surrogates
//! match Java's exactly.
//!
//! Policy (Java's, pinned by `WebhookJsonTest`): nesting deeper than
//! [`MAX_DEPTH`] is refused, and a duplicate object key is refused rather than
//! "last wins". Only ASCII hex digits are accepted in `\u` escapes (Java's
//! `Character.digit` would also take other Unicode digits, which no platform
//! payload contains).

use indexmap::IndexMap;

use crate::java::utf16_to_string;
use crate::WebhookFormatError;

pub(crate) const MAX_DEPTH: usize = 64;

/// A parsed value. Every variant knows the span of source it came from, so
/// [`Value::raw`] can recover the original text of a subtree.
pub(crate) enum Value {
    Obj(IndexMap<Vec<u16>, Value>, Span),
    Arr(Span),
    Str(Vec<u16>, Span),
    Num(Span),
    Bool(bool),
    Null,
}

#[derive(Clone, Copy)]
pub(crate) struct Span {
    start: usize,
    end: usize,
}

impl Value {
    /// The exact source text this value was parsed from (Java `JsonValue.raw`).
    pub(crate) fn raw(&self, src: &[u16]) -> String {
        match self {
            Value::Obj(_, s) | Value::Arr(s) | Value::Str(_, s) | Value::Num(s) => {
                utf16_to_string(&src[s.start..s.end])
            }
            Value::Bool(true) => "true".into(),
            Value::Bool(false) => "false".into(),
            Value::Null => "null".into(),
        }
    }
}

fn err(message: String) -> WebhookFormatError {
    WebhookFormatError(message)
}

pub(crate) fn parse(src: &[u16]) -> Result<Value, WebhookFormatError> {
    let mut p = Parser { s: src, pos: 0 };
    p.skip_whitespace();
    let value = p.parse_value(0)?;
    p.skip_whitespace();
    if !p.at_end() {
        return Err(err(format!(
            "trailing garbage after JSON value at offset {}",
            p.pos
        )));
    }
    Ok(value)
}

struct Parser<'a> {
    s: &'a [u16],
    pos: usize,
}

impl Parser<'_> {
    fn at_end(&self) -> bool {
        self.pos >= self.s.len()
    }

    fn skip_whitespace(&mut self) {
        while let Some(&c) = self.s.get(self.pos) {
            if c == u16::from(b' ')
                || c == u16::from(b'\t')
                || c == u16::from(b'\n')
                || c == u16::from(b'\r')
            {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Result<u16, WebhookFormatError> {
        self.s
            .get(self.pos)
            .copied()
            .ok_or_else(|| err(format!("unexpected end of input at offset {}", self.pos)))
    }

    fn expect(&mut self, c: u8) -> Result<(), WebhookFormatError> {
        if self.s.get(self.pos) != Some(&u16::from(c)) {
            return Err(err(format!(
                "expected '{}' at offset {}",
                char::from(c),
                self.pos
            )));
        }
        self.pos += 1;
        Ok(())
    }

    fn parse_value(&mut self, depth: usize) -> Result<Value, WebhookFormatError> {
        if depth > MAX_DEPTH {
            return Err(err(format!("JSON nesting exceeds max depth {MAX_DEPTH}")));
        }
        let c = self.peek()?;
        match c {
            0x7B => self.parse_object(depth),
            0x5B => self.parse_array(depth),
            0x22 => self.parse_string().map(|(v, span)| Value::Str(v, span)),
            0x74 => self.parse_literal("true", Value::Bool(true)),
            0x66 => self.parse_literal("false", Value::Bool(false)),
            0x6E => self.parse_literal("null", Value::Null),
            _ if c == u16::from(b'-') || is_digit(c) => self.parse_number(),
            _ => Err(err(format!(
                "unexpected character '{}' at offset {}",
                utf16_to_string(&[c]),
                self.pos
            ))),
        }
    }

    fn parse_object(&mut self, depth: usize) -> Result<Value, WebhookFormatError> {
        let start = self.pos;
        self.expect(b'{')?;
        let mut members = IndexMap::new();
        self.skip_whitespace();
        if !self.at_end() && self.peek()? == u16::from(b'}') {
            self.pos += 1;
            return Ok(Value::Obj(
                members,
                Span {
                    start,
                    end: self.pos,
                },
            ));
        }
        loop {
            self.skip_whitespace();
            if self.at_end() || self.peek()? != u16::from(b'"') {
                return Err(err(format!("expected string key at offset {}", self.pos)));
            }
            let (key, _) = self.parse_string()?;
            self.skip_whitespace();
            self.expect(b':')?;
            self.skip_whitespace();
            let value = self.parse_value(depth + 1)?;
            if members.contains_key(&key) {
                return Err(err(format!(
                    "duplicate key '{}' at offset {}",
                    utf16_to_string(&key),
                    self.pos
                )));
            }
            members.insert(key, value);
            self.skip_whitespace();
            let n = self.peek()?;
            if n == u16::from(b',') {
                self.pos += 1;
            } else if n == u16::from(b'}') {
                self.pos += 1;
                break;
            } else {
                return Err(err(format!("expected ',' or '}}' at offset {}", self.pos)));
            }
        }
        Ok(Value::Obj(
            members,
            Span {
                start,
                end: self.pos,
            },
        ))
    }

    fn parse_array(&mut self, depth: usize) -> Result<Value, WebhookFormatError> {
        let start = self.pos;
        self.expect(b'[')?;
        self.skip_whitespace();
        if !self.at_end() && self.peek()? == u16::from(b']') {
            self.pos += 1;
            return Ok(Value::Arr(Span {
                start,
                end: self.pos,
            }));
        }
        loop {
            self.skip_whitespace();
            self.parse_value(depth + 1)?;
            self.skip_whitespace();
            let n = self.peek()?;
            if n == u16::from(b',') {
                self.pos += 1;
            } else if n == u16::from(b']') {
                self.pos += 1;
                break;
            } else {
                return Err(err(format!("expected ',' or ']' at offset {}", self.pos)));
            }
        }
        Ok(Value::Arr(Span {
            start,
            end: self.pos,
        }))
    }

    fn parse_string(&mut self) -> Result<(Vec<u16>, Span), WebhookFormatError> {
        let start = self.pos;
        self.expect(b'"')?;
        let mut out = Vec::new();
        loop {
            let Some(&c) = self.s.get(self.pos) else {
                return Err(err(format!(
                    "unterminated string starting at offset {start}"
                )));
            };
            self.pos += 1;
            if c == u16::from(b'"') {
                break;
            }
            if c == u16::from(b'\\') {
                let Some(&e) = self.s.get(self.pos) else {
                    return Err(err(format!("unterminated escape at offset {}", self.pos)));
                };
                self.pos += 1;
                let decoded = match u8::try_from(e).ok() {
                    Some(b'"') => u16::from(b'"'),
                    Some(b'\\') => u16::from(b'\\'),
                    Some(b'/') => u16::from(b'/'),
                    Some(b'b') => 0x08,
                    Some(b'f') => 0x0C,
                    Some(b'n') => 0x0A,
                    Some(b'r') => 0x0D,
                    Some(b't') => 0x09,
                    Some(b'u') => self.parse_unicode_escape()?,
                    _ => {
                        return Err(err(format!(
                            "invalid escape '\\{}' at offset {}",
                            utf16_to_string(&[e]),
                            self.pos - 1
                        )))
                    }
                };
                out.push(decoded);
            } else if c < 0x20 {
                return Err(err(format!(
                    "unescaped control character at offset {}",
                    self.pos - 1
                )));
            } else {
                out.push(c);
            }
        }
        Ok((
            out,
            Span {
                start,
                end: self.pos,
            },
        ))
    }

    fn parse_unicode_escape(&mut self) -> Result<u16, WebhookFormatError> {
        if self.pos + 4 > self.s.len() {
            return Err(err(format!("truncated \\u escape at offset {}", self.pos)));
        }
        let hex = &self.s[self.pos..self.pos + 4];
        let mut value = 0u16;
        for &h in hex {
            let Some(d) = char::from_u32(u32::from(h)).and_then(|c| c.to_digit(16)) else {
                return Err(err(format!(
                    "invalid \\u escape '{}' at offset {}",
                    utf16_to_string(hex),
                    self.pos
                )));
            };
            value = value * 16 + d as u16;
        }
        self.pos += 4;
        Ok(value)
    }

    fn parse_number(&mut self) -> Result<Value, WebhookFormatError> {
        let start = self.pos;
        let digit_here = |p: &Self| p.s.get(p.pos).is_some_and(|&c| is_digit(c));
        if self.peek()? == u16::from(b'-') {
            self.pos += 1;
        }
        if !digit_here(self) {
            return Err(err(format!("invalid number at offset {start}")));
        }
        if self.peek()? == u16::from(b'0') {
            self.pos += 1;
            if digit_here(self) {
                return Err(err(format!("leading zero in number at offset {start}")));
            }
        } else {
            while digit_here(self) {
                self.pos += 1;
            }
        }
        if self.s.get(self.pos) == Some(&u16::from(b'.')) {
            self.pos += 1;
            if !digit_here(self) {
                return Err(err(format!("invalid fraction at offset {start}")));
            }
            while digit_here(self) {
                self.pos += 1;
            }
        }
        if matches!(self.s.get(self.pos), Some(&c) if c == u16::from(b'e') || c == u16::from(b'E'))
        {
            self.pos += 1;
            if matches!(self.s.get(self.pos), Some(&c) if c == u16::from(b'+') || c == u16::from(b'-'))
            {
                self.pos += 1;
            }
            if !digit_here(self) {
                return Err(err(format!("invalid exponent at offset {start}")));
            }
            while digit_here(self) {
                self.pos += 1;
            }
        }
        Ok(Value::Num(Span {
            start,
            end: self.pos,
        }))
    }

    fn parse_literal(&mut self, literal: &str, value: Value) -> Result<Value, WebhookFormatError> {
        let end = self.pos + literal.len();
        let matches = end <= self.s.len()
            && self.s[self.pos..end]
                .iter()
                .zip(literal.bytes())
                .all(|(&c, b)| c == u16::from(b));
        if !matches {
            return Err(err(format!("invalid literal at offset {}", self.pos)));
        }
        self.pos = end;
        Ok(value)
    }
}

fn is_digit(c: u16) -> bool {
    (u16::from(b'0')..=u16::from(b'9')).contains(&c)
}

/// Java `WebhookJsonTest`: one row per parsing rule.
#[cfg(test)]
mod tests {
    use super::*;

    fn units(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn nested_arrays(depth: usize) -> String {
        format!("{}0{}", "[".repeat(depth), "]".repeat(depth))
    }

    #[test]
    fn rows() {
        let rows: Vec<(&str, String, bool)> = vec![
            ("escape: quote", r#""a\"b""#.into(), true),
            ("escape: backslash", r#""a\\b""#.into(), true),
            ("escape: solidus", r#""a\/b""#.into(), true),
            ("escape: backspace", r#""a\bb""#.into(), true),
            ("escape: formfeed", r#""a\fb""#.into(), true),
            ("escape: newline", r#""a\nb""#.into(), true),
            ("escape: carriage-return", r#""a\rb""#.into(), true),
            ("escape: tab", r#""a\tb""#.into(), true),
            ("escape: unicode basic", r#""\u0041""#.into(), true),
            (
                "escape: unicode surrogate pair",
                r#""\ud83d\ude00""#.into(),
                true,
            ),
            ("escape: unrecognised letter", r#""a\xb""#.into(), false),
            ("escape: truncated \\u", r#""\u12""#.into(), false),
            (
                "escape: \\u with non-hex digits",
                r#""\u12zz""#.into(),
                false,
            ),
            (
                "escape: unescaped control character",
                "\"a\u{0007}b\"".into(),
                false,
            ),
            ("number: leading zero", "01".into(), false),
            ("number: zero alone is fine", "0".into(), true),
            ("number: zero then fraction is fine", "0.5".into(), true),
            ("number: bare minus", "-".into(), false),
            ("number: minus then non-digit", "-a".into(), false),
            ("number: negative integer", "-5".into(), true),
            (
                "number: fraction with no digit after dot",
                "1.".into(),
                false,
            ),
            ("number: incomplete exponent (1e)", "1e".into(), false),
            (
                "number: incomplete exponent with sign (1e+)",
                "1e+".into(),
                false,
            ),
            ("number: valid exponent", "1e10".into(), true),
            ("number: valid negative exponent", "1E-10".into(), true),
            ("string: unterminated", "\"abc".into(), false),
            ("string: empty is fine", "\"\"".into(), true),
            ("depth: 64 nested arrays accepted", nested_arrays(64), true),
            ("depth: 65 nested arrays rejected", nested_arrays(65), false),
            ("trailing garbage after object", "{} x".into(), false),
            ("trailing garbage: extra closing brace", "{}}".into(), false),
            ("no trailing garbage is fine", "{}".into(), true),
            (
                "object: duplicate key rejected",
                r#"{"a":1,"a":2}"#.into(),
                false,
            ),
            ("object: missing colon", r#"{"a" 1}"#.into(), false),
            ("object: missing comma", r#"{"a":1 "b":2}"#.into(), false),
            ("object: non-string key", "{1:2}".into(), false),
            ("object: empty", "{}".into(), true),
            ("object: well-formed", r#"{"a":1,"b":2}"#.into(), true),
            ("array: missing comma", "[1 2]".into(), false),
            ("array: empty", "[]".into(), true),
            ("array: well-formed", "[1,2,3]".into(), true),
            ("literal: true", "true".into(), true),
            ("literal: false", "false".into(), true),
            ("literal: null", "null".into(), true),
            ("literal: malformed (tru)", "tru".into(), false),
            ("empty input", "".into(), false),
            ("whitespace-only input", "   ".into(), false),
        ];
        assert_eq!(rows.len(), 47);
        for (rule, json, accepted) in rows {
            assert_eq!(parse(&units(&json)).is_ok(), accepted, "{rule}");
        }
    }

    fn string_value(json: &str) -> String {
        match parse(&units(json)).ok().unwrap() {
            Value::Str(v, _) => utf16_to_string(&v),
            _ => panic!("not a string"),
        }
    }

    #[test]
    fn escaped_quote_round_trips_to_the_literal_character() {
        assert_eq!(string_value(r#""say \"hi\"""#), "say \"hi\"");
    }

    #[test]
    fn basic_unicode_escape_resolves_to_its_character() {
        assert_eq!(string_value(r#""\u0041""#), "A");
    }

    #[test]
    fn surrogate_pair_resolves_to_one_code_point() {
        assert_eq!(string_value(r#""\ud83d\ude00""#), "\u{1F600}");
    }

    #[test]
    fn duplicate_key_is_rejected_never_last_wins() {
        let e = parse(&units(r#"{"a":1,"a":2}"#)).err().unwrap();
        assert!(e.to_string().contains("duplicate"), "{e}");
    }

    #[test]
    fn raw_captures_the_exact_source_substring_of_a_member_value() {
        let src = units(r#"{"data":{"x": 1,  "y":[1,2]}}"#);
        let Value::Obj(members, _) = parse(&src).ok().unwrap() else {
            panic!("not an object")
        };
        assert_eq!(members[&units("data")].raw(&src), r#"{"x": 1,  "y":[1,2]}"#);
    }
}
