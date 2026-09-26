//! A JSON document rendered as PostgreSQL renders a `jsonb` value as text.
//!
//! Go serves stored JSON documents (an event type's schema, say) as the
//! bytes pgx reads back from a `jsonb` column: PostgreSQL's canonical text
//! form, with `": "` after a key, `", "` between members, and object keys in
//! `jsonb` order (shorter keys first, then bytewise). Where Rust serves such
//! a document as a string, it renders it the same way, so the two platforms
//! answer with the same text.

use serde_json::Value;

/// `value` as PostgreSQL's `jsonb` text output.
pub fn jsonb_text(value: &Value) -> String {
    let mut out = String::new();
    write(value, &mut out);
    out
}

fn write(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_by(|a, b| {
                a.len()
                    .cmp(&b.len())
                    .then_with(|| a.as_bytes().cmp(b.as_bytes()))
            });
            out.push('{');
            for (i, key) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&Value::String(key.clone()).to_string());
                out.push_str(": ");
                write(&map[key], out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write(item, out);
            }
            out.push(']');
        }
        scalar => out.push_str(&scalar.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn renders_as_postgres_does() {
        assert_eq!(
            jsonb_text(&json!({"type": "object"})),
            r#"{"type": "object"}"#
        );
        assert_eq!(
            jsonb_text(&json!({"type": "object", "$schema": "x", "required": ["a", "b"]})),
            r#"{"type": "object", "$schema": "x", "required": ["a", "b"]}"#
        );
        assert_eq!(
            jsonb_text(&json!({"bb": 1, "a": null, "c": true})),
            r#"{"a": null, "c": true, "bb": 1}"#
        );
        assert_eq!(jsonb_text(&json!([])), "[]");
        assert_eq!(jsonb_text(&json!({})), "{}");
    }
}
