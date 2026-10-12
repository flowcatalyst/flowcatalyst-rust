//! Undoing the SDK outbox writers' JSON encoding of a structured member at
//! batch ingest (Go `sdk.UnwrapEncodedJSON`, `8bf67d1`; Java
//! `EncodedJson.unwrap`; owner ruling 2026-10-12).
//!
//! The Java, TypeScript, Laravel, Rust and framework outbox writers put an
//! event's `data` and an audit entry's `operationData` into the batch item as
//! a JSON string holding the serialised value (`"data":"{\"a\":1}"`). Stored
//! as sent, subscribers got a string, a `dataOnly` delivery body was a quoted
//! string, and the audit redaction could not see inside it.
//!
//! Applied by `POST /api/events/batch` (`data`) and `POST
//! /api/audit-logs/batch` (`operationData`, before redaction) only. The
//! singular `POST /api/events`, dispatch-job payloads and function events are
//! not unwrapped.

use serde_json::Value;

/// JSON's insignificant whitespace (RFC 8259 section 2). Only these are
/// trimmed: `str::trim` would also drop a vertical tab, a form feed and the
/// Unicode spaces, which Go keeps.
const JSON_SPACE: [char; 4] = [' ', '\t', '\n', '\r'];

/// `value`, with one level of SDK encoding removed.
///
/// A string is replaced by the value its text holds only when that text,
/// less surrounding JSON whitespace, starts with `{` or `[` and is exactly
/// one valid JSON value: an encoded object or array. Everything else is
/// returned as sent: an object, array, number, boolean or `null` (a value
/// sent as itself is never touched), and every other string: `"hello"`,
/// `""`, `"42"`, `"true"`, `"null"` (Java's encoding of a null map), an
/// encoded string (`"\"x\""`, a value encoded twice), and text that only
/// looks like JSON (`"{not json"`, `"[1,"`, `"{} trailing"`). The result is
/// never a string that was unwrapped, so nothing is unwrapped twice.
///
/// The accepted cost (Go's and Java's): a writer that means a string whose
/// text is a JSON object or array gets the object; such a value has to be
/// wrapped in an object to arrive as text.
///
/// The text is parsed by `serde_json`, as the request body is, so the result
/// is what the same value sent as itself becomes: numbers are `i64`, `u64`
/// or `f64` (no `arbitrary_precision`), nesting is limited to serde_json's
/// 127 levels, and text serde_json refuses (a lone UTF-16 surrogate escape,
/// a number beyond `f64`) stays a string. Go keeps the inner bytes verbatim.
pub fn unwrap_encoded_json(value: Value) -> Value {
    let Value::String(text) = &value else {
        return value;
    };
    let inner = text.trim_matches(JSON_SPACE);
    if !inner.starts_with(['{', '[']) {
        return value;
    }
    serde_json::from_str::<Value>(inner).unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// `raw` (a JSON document) parsed as a request body would be.
    fn parse(raw: &str) -> Value {
        serde_json::from_str(raw).unwrap_or_else(|e| panic!("{raw}: {e}"))
    }

    fn unwrapped(raw: &str) -> Value {
        unwrap_encoded_json(parse(raw))
    }

    /// Go's table (`encoded_json_test.go`), less its two cases on raw input
    /// that is not JSON (`` and `"abc`), which cannot reach the handler once
    /// the body has been parsed. Each result is also unchanged by a second
    /// pass.
    #[test]
    fn gos_cases() {
        let cases: [(&str, &str, &str); 23] = [
            // The SDK writers' encoding of an object or array: unwrapped once.
            (
                "encoded object (Java, TS, Laravel, fc-sdk, framework)",
                r#""{\"orderId\":\"ord_1\",\"total\":1500}""#,
                r#"{"orderId":"ord_1","total":1500}"#,
            ),
            ("encoded empty object", r#""{}""#, "{}"),
            ("encoded empty array (PHP json_encode([]))", r#""[]""#, "[]"),
            ("encoded list", r#""[1,{\"a\":true}]""#, r#"[1,{"a":true}]"#),
            (
                "encoded object with surrounding whitespace",
                " \n\"  {\\\"a\\\":1} \\n\" ",
                r#"{"a":1}"#,
            ),
            (
                "encoded nested string members stay strings",
                r#""{\"note\":\"{\\\"x\\\":1}\"}""#,
                r#"{"note":"{\"x\":1}"}"#,
            ),
            (
                "unicode escapes",
                "\"{\\\"name\\\":\\\"Zo\\u00eb ✓\\\"}\"",
                r#"{"name":"Zoë ✓"}"#,
            ),
            // Values sent as themselves: never touched.
            ("object", r#"{"a":1}"#, r#"{"a":1}"#),
            ("array", "[1,2]", "[1,2]"),
            ("number", "42", "42"),
            ("boolean", "true", "true"),
            ("null", "null", "null"),
            // Strings meant as strings: kept.
            ("plain string", r#""hello""#, r#""hello""#),
            ("empty string", r#""""#, r#""""#),
            ("numeric text", r#""42""#, r#""42""#),
            ("boolean text", r#""true""#, r#""true""#),
            ("null text (Java's null map)", r#""null""#, r#""null""#),
            (
                "encoded string (two levels) kept as sent",
                r#""\"{\\\"a\\\":1}\"""#,
                r#""\"{\\\"a\\\":1}\"""#,
            ),
            (
                "looks like an object, is not JSON",
                r#""{not json""#,
                r#""{not json""#,
            ),
            ("truncated array", r#""[1,""#, r#""[1,""#),
            (
                "object followed by more text",
                r#""{} trailing""#,
                r#""{} trailing""#,
            ),
            ("two values", r#""{}{}""#, r#""{}{}""#),
            ("brace in the middle", r#""total {a}""#, r#""total {a}""#),
        ];
        for (name, input, want) in cases {
            let got = unwrapped(input);
            assert_eq!(got, parse(want), "{name}: {input}");
            assert_eq!(unwrap_encoded_json(got.clone()), got, "{name}: second pass");
        }
    }

    /// Only JSON whitespace is trimmed, as in Go: a vertical tab or form
    /// feed before or after the value keeps the string.
    #[test]
    fn only_json_whitespace_is_trimmed() {
        for kept in ["\u{b}{}", "{}\u{c}", "\u{a0}{}", "\u{2003}[]"] {
            let value = Value::String(kept.to_string());
            assert_eq!(unwrap_encoded_json(value.clone()), value, "{kept:?}");
        }
        assert_eq!(
            unwrap_encoded_json(Value::String("\t\r\n {\"a\":1}\n".into())),
            json!({"a": 1})
        );
    }

    /// The text is parsed strictly: what serde_json refuses as a body is
    /// kept as a string.
    #[test]
    fn non_json_text_is_kept() {
        for kept in [
            "{'a':1}",
            "{a:1}",
            "[1,]",
            "{\"a\":1,}",
            "[NaN]",
            "[01]",
            "{\"a\":1} // c",
            "[\"\u{1}\"]",
        ] {
            let value = Value::String(kept.to_string());
            assert_eq!(unwrap_encoded_json(value.clone()), value, "{kept:?}");
        }
    }

    /// Rust-specific edges, against Go (which keeps the inner bytes): the
    /// result is serde_json's parse of the text, exactly what the same value
    /// sent as itself becomes.
    #[test]
    fn the_text_is_parsed_as_a_body_is() {
        // Precision: no `arbitrary_precision`, so an integer beyond u64 and
        // a long fraction are f64, as they are when sent as themselves.
        let text = "{\"big\":123456789012345678901234567890,\"frac\":0.1234567890123456789}";
        let got = unwrap_encoded_json(Value::String(text.into()));
        assert_eq!(got, parse(text));
        assert!(got["big"].is_f64(), "{got}");
        assert!(got["frac"].is_f64(), "{got}");
        assert_eq!(got["big"].as_f64(), Some(1.2345678901234568e29));

        // A number beyond f64 is refused by serde_json: kept as a string
        // (Go unwraps it; sent as itself it fails the body here).
        let value = Value::String("[1e400]".into());
        assert_eq!(unwrap_encoded_json(value.clone()), value);

        // A lone surrogate escape is refused by serde_json: kept (Go's
        // decoder replaces it with U+FFFD).
        let value = Value::String(r#"{"a":"\ud800"}"#.into());
        assert_eq!(unwrap_encoded_json(value.clone()), value);

        // A duplicate key keeps the last value, as JSONB does.
        assert_eq!(
            unwrap_encoded_json(Value::String(r#"{"a":1,"a":2}"#.into())),
            json!({"a": 2})
        );
    }

    /// serde_json's recursion limit applies, as to the body itself: 127
    /// levels parse, 128 do not, and deeper text stays a string (Go's limit
    /// is 10,000).
    #[test]
    fn nesting_beyond_the_parser_limit_is_kept() {
        let nested = |depth: usize| format!("{}{}", "[".repeat(depth), "]".repeat(depth));
        let at_limit = Value::String(nested(127));
        assert!(unwrap_encoded_json(at_limit).is_array());
        assert!(serde_json::from_str::<Value>(&nested(128)).is_err());
        let beyond = Value::String(nested(128));
        assert_eq!(unwrap_encoded_json(beyond.clone()), beyond);
    }
}
