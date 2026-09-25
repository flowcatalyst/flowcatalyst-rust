//! A structural diff of two [`Normalised`] step records (spec §6): status,
//! then the compared headers, then the body, one [`DiffEntry`] per point of
//! disagreement. Missing on one side is [`ABSENT`], distinct from a JSON
//! `null` (which renders as `null`).

use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;

use crate::normaliser::Normalised;

/// "This pointer resolved to nothing on this side": never a value either
/// side's JSON could contain (guillemets are reserved for sentinels).
pub const ABSENT: &str = "«absent»";

/// `pointer` is `/status`, `/headers/<Name>`, or a JSON Pointer into the body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiffEntry {
    pub pointer: String,
    pub go: String,
    pub rust: String,
}

impl DiffEntry {
    pub fn new(pointer: impl Into<String>, go: impl Into<String>, rust: impl Into<String>) -> Self {
        Self {
            pointer: pointer.into(),
            go: go.into(),
            rust: rust.into(),
        }
    }
}

pub fn compare(go: &Normalised, rust: &Normalised) -> Vec<DiffEntry> {
    let mut out = Vec::new();
    if go.status != rust.status {
        out.push(DiffEntry::new(
            "/status",
            go.status.to_string(),
            rust.status.to_string(),
        ));
    }
    let names: BTreeSet<&String> = go.headers.keys().chain(rust.headers.keys()).collect();
    for name in names {
        let g = go.headers.get(name);
        let r = rust.headers.get(name);
        if g != r {
            out.push(DiffEntry::new(
                format!("/headers/{name}"),
                g.map_or(ABSENT, String::as_str),
                r.map_or(ABSENT, String::as_str),
            ));
        }
    }
    walk(Some(&go.body), Some(&rust.body), "", &mut out);
    out
}

fn walk(a: Option<&Value>, b: Option<&Value>, pointer: &str, out: &mut Vec<DiffEntry>) {
    if let (Some(a), Some(b)) = (a, b) {
        if a == b {
            return;
        }
        match (a, b) {
            (Value::Object(am), Value::Object(bm)) => {
                let keys: BTreeSet<&String> = am.keys().chain(bm.keys()).collect();
                for key in keys {
                    walk(
                        am.get(key),
                        bm.get(key),
                        &format!("{pointer}/{}", escape(key)),
                        out,
                    );
                }
                return;
            }
            (Value::Array(aa), Value::Array(ba)) => {
                for i in 0..aa.len().max(ba.len()) {
                    walk(aa.get(i), ba.get(i), &format!("{pointer}/{i}"), out);
                }
                return;
            }
            _ => {}
        }
    }
    out.push(DiffEntry::new(
        if pointer.is_empty() { "/" } else { pointer },
        a.map_or_else(|| ABSENT.to_string(), render),
        b.map_or_else(|| ABSENT.to_string(), render),
    ));
}

fn render(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn escape(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;
    use serde_json::json;

    fn n(status: u16, body: Value) -> Normalised {
        Normalised {
            status,
            headers: IndexMap::new(),
            body,
        }
    }

    #[test]
    fn equal_records_have_no_diff() {
        assert!(compare(&n(200, json!({"a": [1, 2]})), &n(200, json!({"a": [1, 2]}))).is_empty());
    }

    #[test]
    fn reports_status_headers_members_absent_and_null() {
        let mut go = n(200, json!({"a": 1, "b": null, "arr": [1], "k/x": "v"}));
        let mut rust = n(201, json!({"a": 2, "arr": [1, 2], "k/x": "w"}));
        go.headers
            .insert("Content-Type".into(), "application/json".into());
        rust.headers
            .insert("Cache-Control".into(), "no-store".into());
        go.headers.insert("Cache-Control".into(), "no-store".into());
        let d = compare(&go, &rust);
        assert_eq!(
            d,
            vec![
                DiffEntry::new("/status", "200", "201"),
                DiffEntry::new("/headers/Content-Type", "application/json", ABSENT),
                DiffEntry::new("/a", "1", "2"),
                DiffEntry::new("/arr/1", ABSENT, "2"),
                DiffEntry::new("/b", "null", ABSENT),
                DiffEntry::new("/k~1x", "v", "w"),
            ]
        );
    }

    #[test]
    fn a_type_mismatch_at_the_root_is_one_entry() {
        assert_eq!(
            compare(&n(200, json!("text")), &n(200, json!({"a": 1}))),
            vec![DiffEntry::new("/", "text", "{\"a\":1}")]
        );
    }
}
