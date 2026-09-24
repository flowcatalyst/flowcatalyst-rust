//! Audit logs never store passwords or secrets.
//!
//! The one redaction rule, applied to a command document before it is
//! written to an audit row anywhere — the platform's unit of work, the SDK
//! ingest backstop, and fc-sdk's outbox. Owner spec:
//! `docs/spec/audit-redaction.md` in the Java repo (flowcatalyst-javalin,
//! 2026-09-24); the shared cases are `docs/spec/audit-redaction-vectors.json`
//! and every implementation (Java, Go, Rust, TypeScript, Laravel) runs them.
//!
//! - A key is **secret** when, lower-cased with `_` and `-` removed, it ends
//!   with `password`, `passwordhash`, `secret`, `secretref`, `passphrase` or
//!   `token`, or equals `apikey`, `privatekey`, `authorization` or `cookie`.
//! - A secret key's value becomes the string `"***"` whatever its type,
//!   except `null` and booleans, which are kept.
//! - A command may also declare **masked fields** ([`AuditMasked`]):
//!   top-level field names masked the same way even though the name rule
//!   would keep them.
//! - Objects and arrays are walked; everything else is untouched.

use serde::Serialize;
use serde_json::{Map, Value};

/// What a secret value is replaced with.
pub const MASK: &str = "***";

/// A normalised key ending with one of these is secret.
const SECRET_SUFFIXES: &[&str] = &[
    "password",
    "passwordhash",
    "secret",
    "secretref",
    "passphrase",
    "token",
];

/// A normalised key equal to one of these is secret.
const SECRET_EXACT: &[&str] = &["apikey", "privatekey", "authorization", "cookie"];

/// A command's declared masked fields: top-level field names (as they appear
/// in the serialised JSON) that [`redact`] masks even though the name rule
/// alone would keep them, e.g. a platform-config `value` that is only secret
/// when the command's own `valueType` says so.
///
/// The default is none; a command with nothing to declare implements the
/// trait with an empty body.
pub trait AuditMasked {
    fn audit_masked_fields(&self) -> &'static [&'static str] {
        &[]
    }
}

impl<T: AuditMasked + ?Sized> AuditMasked for &T {
    fn audit_masked_fields(&self) -> &'static [&'static str] {
        (**self).audit_masked_fields()
    }
}

/// An already-built JSON document declares nothing; the name rule applies.
impl AuditMasked for Value {}

/// Redact `value` by the rule, masking the top-level fields named in
/// `masked` too. Never mutates its input; returns a fresh document.
pub fn redact(value: &Value, masked: &[&str]) -> Value {
    redact_node(value, masked, true)
}

/// [`redact`] for a stored or ingested audit document, which may also be a
/// JSON *string* whose text is itself a JSON object or array: the
/// TypeScript, Laravel and fc-sdk outbox DTOs send `operationData` that way,
/// and the platform stores it as a JSONB string. Such a string is parsed,
/// redacted and re-encoded — only when redaction changed something, so an
/// unaffected document comes back exactly as it went in.
pub fn redact_document(value: &Value, masked: &[&str]) -> Value {
    if let Value::String(text) = value {
        let trimmed = text.trim_start();
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            if let Ok(inner) = serde_json::from_str::<Value>(text) {
                let redacted = redact(&inner, masked);
                if redacted != inner {
                    return Value::String(redacted.to_string());
                }
            }
        }
        return value.clone();
    }
    redact(value, masked)
}

/// Serialise `command` for an audit row and redact it, including the
/// command's own [`AuditMasked`] fields.
pub fn redacted_command_json<C>(command: &C) -> Result<Value, serde_json::Error>
where
    C: Serialize + AuditMasked + ?Sized,
{
    let value = serde_json::to_value(command)?;
    Ok(redact(&value, command.audit_masked_fields()))
}

/// Whether `key` names a secret under the rule.
pub fn is_secret_key(key: &str) -> bool {
    let normalized: String = key
        .chars()
        .filter(|c| *c != '_' && *c != '-')
        .flat_map(char::to_lowercase)
        .collect();
    SECRET_EXACT.contains(&normalized.as_str())
        || SECRET_SUFFIXES.iter().any(|s| normalized.ends_with(s))
}

fn redact_node(node: &Value, masked: &[&str], top_level: bool) -> Value {
    match node {
        Value::Object(fields) => {
            let mut out = Map::with_capacity(fields.len());
            for (key, value) in fields {
                let secret = is_secret_key(key) || (top_level && masked.contains(&key.as_str()));
                let redacted = if secret {
                    mask_value(value)
                } else {
                    redact_node(value, masked, false)
                };
                out.insert(key.clone(), redacted);
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| redact_node(item, masked, false))
                .collect(),
        ),
        leaf => leaf.clone(),
    }
}

/// A secret field's value: `"***"` whatever its type, except `null` and
/// booleans, which are kept.
fn mask_value(value: &Value) -> Value {
    match value {
        Value::Null | Value::Bool(_) => value.clone(),
        _ => Value::String(MASK.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Every case of `docs/spec/audit-redaction-vectors.json` (this repo's
    /// copy of the Java repo's canonical file).
    #[test]
    fn every_shared_vector() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/spec/audit-redaction-vectors.json"
        );
        let bytes = std::fs::read(path).expect("read vectors");
        let cases: Vec<Value> = serde_json::from_slice(&bytes).expect("parse vectors");
        assert!(!cases.is_empty());
        for case in &cases {
            let name = case["name"].as_str().expect("name");
            let masked: Vec<&str> = case["masked"]
                .as_array()
                .expect("masked")
                .iter()
                .map(|m| m.as_str().expect("masked name"))
                .collect();
            let actual = redact(&case["input"], &masked);
            assert_eq!(actual, case["expected"], "vector `{name}`");
        }
    }

    #[test]
    fn never_mutates_its_input() {
        let input =
            json!({"password": "hunter2", "nested": {"token": "t"}, "list": [{"secret": 1}]});
        let before = input.clone();
        let out = redact(&input, &["nested"]);
        assert_eq!(input, before);
        assert_ne!(out, input);
    }

    #[test]
    fn masked_fields_are_top_level_only() {
        let input =
            json!({"value": "top", "nested": {"value": "inner"}, "rows": [{"value": "row"}]});
        let out = redact(&input, &["value"]);
        assert_eq!(
            out,
            json!({"value": "***", "nested": {"value": "inner"}, "rows": [{"value": "row"}]})
        );
    }

    /// The shared vectors' boolean case (`enforcePasswordComplexity`) is not
    /// a secret name at all, so they do not pin this; these do.
    #[test]
    fn a_secret_named_boolean_is_kept() {
        let input = json!({"requirePassword": false, "rotate_token": true, "nested": {"clientSecret": true}});
        assert_eq!(redact(&input, &[]), input);
    }

    #[test]
    fn a_masked_null_or_boolean_is_kept() {
        let out = redact(&json!({"value": null, "flag": true}), &["value", "flag"]);
        assert_eq!(out, json!({"value": null, "flag": true}));
    }

    #[test]
    fn a_top_level_array_is_walked_and_declares_no_masks() {
        let out = redact(&json!([{"value": "v", "apiKey": "k"}]), &["value"]);
        assert_eq!(out, json!([{"value": "v", "apiKey": "***"}]));
    }

    #[test]
    fn non_container_documents_are_returned_unchanged() {
        assert_eq!(redact(&json!("password"), &[]), json!("password"));
        assert_eq!(redact(&Value::Null, &["value"]), Value::Null);
    }

    #[test]
    fn key_normalisation() {
        for key in [
            "PASSWORD",
            "new-password",
            "API_KEY",
            "Api-Key",
            "Cookie",
            "idToken",
        ] {
            assert!(is_secret_key(key), "{key} should be secret");
        }
        for key in [
            "passwordPolicy",
            "tokenType",
            "apiKeyId",
            "X_API_KEY",
            "set-cookie",
            "secretKeys",
            "value",
        ] {
            assert!(!is_secret_key(key), "{key} should not be secret");
        }
    }

    #[test]
    fn redact_document_reaches_into_a_json_encoded_string() {
        let stored = Value::String(r#"{"email":"a@b.c","password":"hunter2"}"#.to_string());
        let out = redact_document(&stored, &[]);
        let inner: Value = serde_json::from_str(out.as_str().expect("still a string")).unwrap();
        assert_eq!(inner, json!({"email": "a@b.c", "password": "***"}));

        let masked = Value::String(r#"{"value":"sk","valueType":"SECRET"}"#.to_string());
        let inner: Value =
            serde_json::from_str(redact_document(&masked, &["value"]).as_str().unwrap()).unwrap();
        assert_eq!(inner["value"], "***");
    }

    #[test]
    fn redact_document_leaves_an_unaffected_document_exactly_as_it_was() {
        for doc in [
            Value::String("{ \"b\": 1,  \"a\": [true] }".to_string()),
            Value::String("not json {".to_string()),
            Value::String("{not json".to_string()),
            Value::String("password".to_string()),
            json!({"name": "kept"}),
            Value::Null,
        ] {
            assert_eq!(redact_document(&doc, &[]), doc);
        }
        assert_eq!(
            redact_document(&json!({"token": "t"}), &[]),
            json!({"token": "***"})
        );
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Declares {
        value: &'static str,
        new_password: &'static str,
    }

    impl AuditMasked for Declares {
        fn audit_masked_fields(&self) -> &'static [&'static str] {
            &["value"]
        }
    }

    #[test]
    fn redacted_command_json_applies_the_commands_declared_masks() {
        let json = redacted_command_json(&Declares {
            value: "sk_live",
            new_password: "p",
        })
        .unwrap();
        assert_eq!(json, json!({"value": "***", "newPassword": "***"}));
    }
}
